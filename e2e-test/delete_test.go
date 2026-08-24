package tests

import (
	"context"

	"github.com/marsevilspirit/nimbis/e2e-test/util"
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
		util.Del(ctx, rdb, util.StringType, "key1", "key2")
		util.Del(ctx, rdb, util.HashType, "hash1")
	})

	AfterEach(func() {
		Expect(rdb.Close()).To(Succeed())
	})

	It("should delete a single String key", func() {
		// SET key1
		err := rdb.Set(ctx, "key1", "value1", 0).Err()
		Expect(err).NotTo(HaveOccurred())

		// DEL key1
		deleted := util.Del(ctx, rdb, util.StringType, "key1").Val()
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
		deleted := util.Del(ctx, rdb, util.HashType, "hash1").Val()
		Expect(deleted).To(Equal(int64(1)), "Should delete 1 hash")

		// Verify hash is gone
		exists := util.Exists(ctx, rdb, util.HashType, "hash1").Val()
		Expect(exists).To(Equal(int64(0)))

		// Verify HGET returns nil
		val, err := rdb.HGet(ctx, "hash1", "field1").Result()
		Expect(err).To(Equal(redis.Nil))
		Expect(val).To(BeEmpty())
	})

	It("should delete non-existent key", func() {
		// DEL nonexistent
		deleted := util.Del(ctx, rdb, util.StringType, "nonexistent").Val()
		Expect(deleted).To(Equal(int64(0)), "Should delete 0 keys")
	})

	It("should delete multiple keys and count only existing keys", func() {
		Expect(rdb.Set(ctx, "key1", "value1", 0).Err()).NotTo(HaveOccurred())
		Expect(rdb.Set(ctx, "key2", "value2", 0).Err()).NotTo(HaveOccurred())

		deleted, err := util.Del(ctx, rdb, util.StringType, "key1", "key2", "missing").Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(deleted).To(Equal(int64(2)))

		exists, err := util.Exists(ctx, rdb, util.StringType, "key1", "key2").Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(exists).To(Equal(int64(0)))
	})
})
