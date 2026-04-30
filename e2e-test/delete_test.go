package tests

import (
	"context"
	"fmt"

	"github.com/marsevilspirit/nimbis/e2e-test/util"
	. "github.com/onsi/ginkgo/v2"
	. "github.com/onsi/gomega"
	"github.com/redis/go-redis/v9"
)

func hashKey(key string) uint64 {
	var hasher uint64 = 0xcbf29ce484222325
	for i := 0; i < len(key); i++ {
		hasher ^= uint64(key[i])
		hasher *= 0x100000001b3
	}
	return hasher
}

func findCrossShardKeys(workerCount int) (string, string) {
	seen := map[int]string{}
	for i := 0; i < 2000; i++ {
		key := fmt.Sprintf("e2e:route:key:%d", i)
		worker := int(hashKey(key) % uint64(workerCount))
		if existing, ok := seen[worker]; ok && existing != key {
			for otherWorker, otherKey := range seen {
				if otherWorker != worker {
					return otherKey, key
				}
			}
		}
		if _, ok := seen[worker]; !ok {
			seen[worker] = key
		}
	}
	panic("failed to find cross-shard keys")
}

var _ = Describe("DEL Commands", func() {
	var rdb *redis.Client
	var ctx context.Context

	BeforeEach(func() {
		rdb = util.NewClient()
		ctx = context.Background()
		Expect(rdb.Ping(ctx).Err()).To(Succeed())

		// Clear test keys before each test
		rdb.Del(ctx, "key1")
		rdb.Del(ctx, "hash1")
	})

	AfterEach(func() {
		Expect(rdb.Close()).To(Succeed())
	})

	It("should delete a single String key", func() {
		// SET key1
		err := rdb.Set(ctx, "key1", "value1", 0).Err()
		Expect(err).NotTo(HaveOccurred())

		// DEL key1
		deleted := rdb.Del(ctx, "key1").Val()
		Expect(deleted).To(Equal(int64(1)), "Should delete 1 key")

		// Verify key is gone
		val, err := rdb.Get(ctx, "key1").Result()
		Expect(err).To(Equal(redis.Nil))
		Expect(val).To(BeEmpty())
	})

	It("should delete a Hash key", func() {
		// HSET hash1 field1 value1
		err := rdb.HSet(ctx, "hash1", "field1", "value1").Err()
		Expect(err).NotTo(HaveOccurred())

		// DEL hash1
		deleted := rdb.Del(ctx, "hash1").Val()
		Expect(deleted).To(Equal(int64(1)), "Should delete 1 hash")

		// Verify hash is gone
		exists := rdb.Exists(ctx, "hash1").Val()
		Expect(exists).To(Equal(int64(0)))

		// Verify HGET returns nil
		val, err := rdb.HGet(ctx, "hash1", "field1").Result()
		Expect(err).To(Equal(redis.Nil))
		Expect(val).To(BeEmpty())
	})

	It("should delete non-existent key", func() {
		// DEL nonexistent
		deleted := rdb.Del(ctx, "nonexistent").Val()
		Expect(deleted).To(Equal(int64(0)), "Should delete 0 keys")
	})

	It("should support multi-key DEL and EXISTS across shards", func() {
		key1, key2 := findCrossShardKeys(2)
		Expect(rdb.Set(ctx, key1, "v1", 0).Err()).To(Succeed())
		Expect(rdb.Set(ctx, key2, "v2", 0).Err()).To(Succeed())

		exists := rdb.Exists(ctx, key1, key2, "missing").Val()
		Expect(exists).To(Equal(int64(2)))

		deleted := rdb.Del(ctx, key1, key2).Val()
		Expect(deleted).To(Equal(int64(2)))

		deleted = rdb.Del(ctx, "missing1", "missing2").Val()
		Expect(deleted).To(Equal(int64(0)))

		Expect(rdb.Set(ctx, key1, "v1", 0).Err()).To(Succeed())
		deleted = rdb.Del(ctx, key1, "missing3").Val()
		Expect(deleted).To(Equal(int64(1)))
	})
})
