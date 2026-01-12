package tests

import (
	"context"
	"fmt"
	"sync"

	"github.com/marsevilspirit/nimbis/tests/util"
	. "github.com/onsi/ginkgo/v2"
	. "github.com/onsi/gomega"
	"github.com/redis/go-redis/v9"
)

var _ = Describe("Concurrency Tests", func() {
	var ctx context.Context
	var client *redis.Client

	BeforeEach(func() {
		ctx = context.Background()
		client = util.NewClient()
		Expect(client.FlushDB(ctx).Err()).NotTo(HaveOccurred())
	})

	AfterEach(func() {
		Expect(client.Close()).NotTo(HaveOccurred())
	})

	It("should handle concurrent INCR operations atomically", func() {
		key := "concurrent_incr_key"
		// Increase numbers to ensure race conditions trigger if locking is missing
		const numGoroutines = 50
		const numIncrements = 1000
		expectedValue := int64(numGoroutines * numIncrements)

		// Initialize key to 0
		err := client.Set(ctx, key, 0, 0).Err()
		Expect(err).NotTo(HaveOccurred())

		// Sanity check: INCR should work once
		val, err := client.Incr(ctx, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(val).To(Equal(int64(1)))

		// Reset to 0 for the actual test
		err = client.Set(ctx, key, 0, 0).Err()
		Expect(err).NotTo(HaveOccurred())

		var wg sync.WaitGroup
		wg.Add(numGoroutines)

		// Start concurrent increments
		for i := 0; i < numGoroutines; i++ {
			go func() {
				defer wg.Done()
				// Use a new client per goroutine to simulate distinct clients better,
				// though sharing one is also fine for Go-Redis which is thread-safe.
				// However, creating new clients ensures we are hitting the server concurrently on different cnx if pooled.
				// Note: util.NewClient() creates a new client each time.
				// But to avoid too many connections opening/closing rapidly, using the shared client
				// derived from the pool is standard. Go-Redis client is thread-safe.
				// For stricter "distinct client" simulation let's use the shared client which manages a pool.

				for j := 0; j < numIncrements; j++ {
					err := client.Incr(ctx, key).Err()
					// We don't fail properly inside goroutine with Expect, so just log or ignore.
					// The final value check is what matters.
					// Ideally we should track errors.
					if err != nil {
						fmt.Printf("Error incrementing: %v\n", err)
					}
				}
			}()
		}

		wg.Wait()

		// Verify final value
		val, err = client.Get(ctx, key).Int64()
		Expect(err).NotTo(HaveOccurred())

		// If concurrent control is missing, this is expected to fail.
		// We print a helpful message on failure.
		Expect(val).To(Equal(expectedValue),
			fmt.Sprintf("Expected final value %d (from %d routines * %d incrs), but got %d. This indicates a race condition.",
				expectedValue, numGoroutines, numIncrements, val))
	})

	It("should handle concurrent INCR on multiple keys", func() {
		const numKeys = 10
		const numGoroutines = 20
		const numIncrements = 500
		const expectedValue = int64(numGoroutines * numIncrements)

		var wg sync.WaitGroup
		wg.Add(numKeys * numGoroutines)

		// Initialize keys
		for k := 0; k < numKeys; k++ {
			key := fmt.Sprintf("concurrent_multi_incr_key_%d", k)
			err := client.Set(ctx, key, 0, 0).Err()
			Expect(err).NotTo(HaveOccurred())
		}

		// Start concurrent increments across keys
		for k := 0; k < numKeys; k++ {
			key := fmt.Sprintf("concurrent_multi_incr_key_%d", k)
			for i := 0; i < numGoroutines; i++ {
				go func(targetKey string) {
					defer wg.Done()
					for j := 0; j < numIncrements; j++ {
						err := client.Incr(ctx, targetKey).Err()
						if err != nil {
							fmt.Printf("Error incrementing %s: %v\n", targetKey, err)
						}
					}
				}(key)
			}
		}

		wg.Wait()

		// Verify all keys
		for k := 0; k < numKeys; k++ {
			key := fmt.Sprintf("concurrent_multi_incr_key_%d", k)
			val, err := client.Get(ctx, key).Int64()
			Expect(err).NotTo(HaveOccurred())
			Expect(val).To(Equal(expectedValue), "Key %s mismatch", key)
		}
	})

	It("should handle concurrent LPUSH operations", func() {
		key := "concurrent_list"
		const numGoroutines = 50
		const numPushes = 200
		const totalItems = numGoroutines * numPushes

		// Ensure list is empty
		client.Del(ctx, key)

		var wg sync.WaitGroup
		wg.Add(numGoroutines)

		for i := 0; i < numGoroutines; i++ {
			go func(id int) {
				defer wg.Done()
				for j := 0; j < numPushes; j++ {
					val := fmt.Sprintf("item-%d-%d", id, j)
					err := client.LPush(ctx, key, val).Err()
					if err != nil {
						fmt.Printf("Error pushing to list: %v\n", err)
					}
				}
			}(i)
		}

		wg.Wait()

		lenVal, err := client.LLen(ctx, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(lenVal).To(Equal(int64(totalItems)), "List length mismatch")
	})

	It("should handle concurrent SADD operations", func() {
		key := "concurrent_set"
		const numGoroutines = 50
		const numAdds = 200
		const totalUniqueItems = numGoroutines * numAdds

		client.Del(ctx, key)

		var wg sync.WaitGroup
		wg.Add(numGoroutines)

		for i := 0; i < numGoroutines; i++ {
			go func(id int) {
				defer wg.Done()
				for j := 0; j < numAdds; j++ {
					// Use unique items to verify total count
					val := fmt.Sprintf("member-%d-%d", id, j)
					err := client.SAdd(ctx, key, val).Err()
					if err != nil {
						fmt.Printf("Error adding to set: %v\n", err)
					}
				}
			}(i)
		}

		wg.Wait()

		cardVal, err := client.SCard(ctx, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(cardVal).To(Equal(int64(totalUniqueItems)), "Set cardinality mismatch")
	})
})
