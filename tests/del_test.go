package tests

import (
	"context"

	"github.com/marsevilspirit/nimbis/tests/util"
	. "github.com/onsi/ginkgo/v2"
	. "github.com/onsi/gomega"
	"github.com/redis/go-redis/v9"
)

var _ = Describe("DEL Commands", func() {
	var rdb *redis.Client
	var ctx context.Context

	BeforeEach(func() {
		rdb = util.NewClient()
		ctx = context.Background()
		Expect(rdb.Ping(ctx).Err()).To(Succeed())

		// Clear test keys before each test
		rdb.Del(ctx, "key1", "key2", "key3", "hash1", "nonexistent")
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

	It("should delete multiple String keys", func() {
		// SET key1, key2, key3
		rdb.Set(ctx, "key1", "value1", 0)
		rdb.Set(ctx, "key2", "value2", 0)
		rdb.Set(ctx, "key3", "value3", 0)

		// DEL key1 key2 key3
		deleted := rdb.Del(ctx, "key1", "key2", "key3").Val()
		Expect(deleted).To(Equal(int64(3)), "Should delete 3 keys")

		// Verify all keys are gone
		val1, _ := rdb.Get(ctx, "key1").Result()
		val2, _ := rdb.Get(ctx, "key2").Result()
		val3, _ := rdb.Get(ctx, "key3").Result()
		Expect(val1).To(BeEmpty())
		Expect(val2).To(BeEmpty())
		Expect(val3).To(BeEmpty())
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

	It("should delete mixed keys (String and Hash)", func() {
		// SET key1
		rdb.Set(ctx, "key1", "value1", 0)

		// HSET hash1 field1 value1
		rdb.HSet(ctx, "hash1", "field1", "value1")

		// DEL key1 hash1 nonexistent
		deleted := rdb.Del(ctx, "key1", "hash1", "nonexistent").Val()
		Expect(deleted).To(Equal(int64(2)), "Should delete 2 keys (key1 and hash1)")

		// Verify both are gone
		val1, err1 := rdb.Get(ctx, "key1").Result()
		Expect(err1).To(Equal(redis.Nil))
		Expect(val1).To(BeEmpty())

		exists := rdb.Exists(ctx, "hash1").Val()
		Expect(exists).To(Equal(int64(0)))
	})

	It("should partially delete keys", func() {
		// SET key1 and key2
		rdb.Set(ctx, "key1", "value1", 0)
		rdb.Set(ctx, "key2", "value2", 0)

		// DEL key1 nonexistent key2
		deleted := rdb.Del(ctx, "key1", "nonexistent", "key2").Val()
		Expect(deleted).To(Equal(int64(2)), "Should delete 2 keys (key1 and key2)")
	})
})
