package tests

import (
	"context"
	"fmt"
	"sort"
	"sync"

	"github.com/marsevilspirit/nimbis/e2e-test/util"
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
				defer GinkgoRecover()

				// Use a new client per goroutine to simulate distinct clients better,
				// though sharing one is also fine for Go-Redis which is thread-safe.
				// However, creating new clients ensures we are hitting the server concurrently on different cnx if pooled.
				// Note: util.NewClient() creates a new client each time.
				// But to avoid too many connections opening/closing rapidly, using the shared client
				// derived from the pool is standard. Go-Redis client is thread-safe.
				// For stricter "distinct client" simulation let's use the shared client which manages a pool.

				for j := 0; j < numIncrements; j++ {
					err := client.Incr(ctx, key).Err()
					Expect(err).NotTo(HaveOccurred())
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
					defer GinkgoRecover()
					for j := 0; j < numIncrements; j++ {
						err := client.Incr(ctx, targetKey).Err()
						Expect(err).NotTo(HaveOccurred())
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
			Expect(val).To(Equal(expectedValue), fmt.Sprintf("Key %s mismatch", key))
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
				defer GinkgoRecover()
				for j := 0; j < numPushes; j++ {
					val := fmt.Sprintf("item-%d-%d", id, j)
					err := client.LPush(ctx, key, val).Err()
					Expect(err).NotTo(HaveOccurred())
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
				defer GinkgoRecover()
				for j := 0; j < numAdds; j++ {
					// Use unique items to verify total count
					val := fmt.Sprintf("member-%d-%d", id, j)
					err := client.SAdd(ctx, key, val).Err()
					Expect(err).NotTo(HaveOccurred())
				}
			}(i)
		}

		wg.Wait()

		cardVal, err := client.SCard(ctx, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(cardVal).To(Equal(int64(totalUniqueItems)), "Set cardinality mismatch")
	})

	It("should keep multi-key DEL/EXISTS consistent under concurrent mixed commands", func() {
		key1, key2 := findCrossShardKeys(2)
		Expect(client.Set(ctx, key1, "seed1", 0).Err()).NotTo(HaveOccurred())
		Expect(client.Set(ctx, key2, "seed2", 0).Err()).NotTo(HaveOccurred())

		const numGoroutines = 40
		const numIterations = 200

		var wg sync.WaitGroup
		wg.Add(numGoroutines)

		for i := 0; i < numGoroutines; i++ {
			go func(id int) {
				defer wg.Done()
				defer GinkgoRecover()

				for j := 0; j < numIterations; j++ {
					existsCount, err := client.Exists(ctx, key1, key2).Result()
					Expect(err).NotTo(HaveOccurred())
					Expect(existsCount).To(BeNumerically(">=", 0))
					Expect(existsCount).To(BeNumerically("<=", 2))

					deletedCount, err := client.Del(ctx, key1, key2).Result()
					Expect(err).NotTo(HaveOccurred())
					Expect(deletedCount).To(BeNumerically(">=", 0))
					Expect(deletedCount).To(BeNumerically("<=", 2))

					// Recreate keys to interleave write/read/delete across shards.
					if (id+j)%2 == 0 {
						Expect(client.Set(ctx, key1, fmt.Sprintf("v1-%d-%d", id, j), 0).Err()).NotTo(HaveOccurred())
					}
					if (id+j)%3 == 0 {
						Expect(client.Set(ctx, key2, fmt.Sprintf("v2-%d-%d", id, j), 0).Err()).NotTo(HaveOccurred())
					}
				}
			}(i)
		}

		wg.Wait()

		// Deterministic final check after concurrent phase.
		Expect(client.Set(ctx, key1, "final1", 0).Err()).NotTo(HaveOccurred())
		Expect(client.Set(ctx, key2, "final2", 0).Err()).NotTo(HaveOccurred())

		existsCount, err := client.Exists(ctx, key1, key2).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(existsCount).To(Equal(int64(2)))

		deletedCount, err := client.Del(ctx, key1, key2).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(deletedCount).To(Equal(int64(2)))

		existsCount, err = client.Exists(ctx, key1, key2).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(existsCount).To(Equal(int64(0)))
	})

	It("should preserve strict multi-key invariants under high-concurrency mixed commands", func() {
		const numGoroutines = 32
		const numIterations = 60
		const msetnxGroups = 8

		var wg sync.WaitGroup
		winners := make([]int, msetnxGroups)
		var winnersMu sync.Mutex
		wg.Add(numGoroutines)

		for i := 0; i < numGoroutines; i++ {
			go func(id int) {
				defer wg.Done()
				defer GinkgoRecover()

				localClient := util.NewClient()
				defer localClient.Close()

				for j := 0; j < numIterations; j++ {
					prefix := fmt.Sprintf("mixed:%d:%d", id, j)
					key1 := prefix + ":k1"
					key2 := prefix + ":k2"
					missing := prefix + ":missing"
					value1 := fmt.Sprintf("v1-%d-%d", id, j)
					value2 := fmt.Sprintf("v2-%d-%d", id, j)

					Expect(localClient.MSet(ctx, key1, value1, key2, value2).Err()).NotTo(HaveOccurred())

					values, err := localClient.MGet(ctx, key1, key2, missing).Result()
					Expect(err).NotTo(HaveOccurred())
					Expect(values).To(Equal([]interface{}{value1, value2, nil}))

					existsCount, err := localClient.Exists(ctx, key1, key2, missing).Result()
					Expect(err).NotTo(HaveOccurred())
					Expect(existsCount).To(Equal(int64(2)))

					deletedCount, err := localClient.Del(ctx, key1, key2, missing).Result()
					Expect(err).NotTo(HaveOccurred())
					Expect(deletedCount).To(Equal(int64(2)))

					existsCount, err = localClient.Exists(ctx, key1, key2, missing).Result()
					Expect(err).NotTo(HaveOccurred())
					Expect(existsCount).To(Equal(int64(0)))

					set1 := prefix + ":set1"
					set2 := prefix + ":set2"
					Expect(localClient.SAdd(ctx, set1, "a", "b", "c").Err()).NotTo(HaveOccurred())
					Expect(localClient.SAdd(ctx, set2, "b", "c", "d").Err()).NotTo(HaveOccurred())

					union, err := localClient.SUnion(ctx, set1, set2).Result()
					Expect(err).NotTo(HaveOccurred())
					sort.Strings(union)
					Expect(union).To(Equal([]string{"a", "b", "c", "d"}))

					inter, err := localClient.SInter(ctx, set1, set2).Result()
					Expect(err).NotTo(HaveOccurred())
					sort.Strings(inter)
					Expect(inter).To(Equal([]string{"b", "c"}))

					diff, err := localClient.SDiff(ctx, set1, set2).Result()
					Expect(err).NotTo(HaveOccurred())
					sort.Strings(diff)
					Expect(diff).To(Equal([]string{"a"}))

					group := (id + j) % msetnxGroups
					lockKey1 := fmt.Sprintf("mixed:msetnx:%d:k1", group)
					lockKey2 := fmt.Sprintf("mixed:msetnx:%d:k2", group)
					written, err := localClient.MSetNX(
						ctx,
						lockKey1, fmt.Sprintf("winner-%d-%d-a", id, j),
						lockKey2, fmt.Sprintf("winner-%d-%d-b", id, j),
					).Result()
					Expect(err).NotTo(HaveOccurred())

					if written {
						winnersMu.Lock()
						winners[group]++
						winnersMu.Unlock()
					}
				}
			}(i)
		}

		wg.Wait()

		for group, winnerCount := range winners {
			Expect(winnerCount).To(Equal(1), fmt.Sprintf("MSETNX group %d should have exactly one winner", group))
			lockKey1 := fmt.Sprintf("mixed:msetnx:%d:k1", group)
			lockKey2 := fmt.Sprintf("mixed:msetnx:%d:k2", group)
			values, err := client.MGet(ctx, lockKey1, lockKey2).Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(values[0]).NotTo(BeNil())
			Expect(values[1]).NotTo(BeNil())
		}
	})

	It("should allow only one concurrent MSETNX winner for the same keys", func() {
		key1, key2 := findCrossShardKeys(2)
		Expect(client.Del(ctx, key1, key2).Err()).NotTo(HaveOccurred())

		const numGoroutines = 40
		var wg sync.WaitGroup
		results := make(chan bool, numGoroutines)
		wg.Add(numGoroutines)

		for i := 0; i < numGoroutines; i++ {
			go func(id int) {
				defer wg.Done()
				defer GinkgoRecover()

				written, err := client.MSetNX(
					ctx,
					key1, fmt.Sprintf("v1-%d", id),
					key2, fmt.Sprintf("v2-%d", id),
				).Result()
				Expect(err).NotTo(HaveOccurred())
				results <- written
			}(i)
		}

		wg.Wait()
		close(results)

		winners := 0
		for written := range results {
			if written {
				winners++
			}
		}
		Expect(winners).To(Equal(1))

		values, err := client.MGet(ctx, key1, key2).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(values[0]).NotTo(BeNil())
		Expect(values[1]).NotTo(BeNil())
	})
})
