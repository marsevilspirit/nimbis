package tests

import (
	"context"

	"github.com/marsevilspirit/nimbis/tests/util"
	. "github.com/onsi/ginkgo/v2"
	. "github.com/onsi/gomega"
	"github.com/redis/go-redis/v9"
)

var _ = Describe("Hash Commands", func() {
	var rdb *redis.Client
	var ctx context.Context

	BeforeEach(func() {
		rdb = util.NewClient()
		ctx = context.Background()
		Expect(rdb.Ping(ctx).Err()).To(Succeed())
	})

	AfterEach(func() {
		Expect(rdb.Close()).To(Succeed())
	})

	It("should HSET and HGET users profile", func() {
		key := "user:100"
		field := "username"
		val := "alice"

		// Set single field
		res, err := rdb.HSet(ctx, key, field, val).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(res).To(Equal(int64(1)))

		// Get field
		result, err := rdb.HGet(ctx, key, field).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(result).To(Equal(val))
	})

	It("should HSET multiple fields for a post and HMGET them", func() {
		key := "post:123"

		// Set multiple fields
		res, err := rdb.HSet(ctx, key, "title", "Hello World", "author", "alice", "likes", "42").Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(res).To(Equal(int64(3)))

		// HMGET
		results, err := rdb.HMGet(ctx, key, "title", "author", "likes", "comments").Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(results).To(HaveLen(4))
		Expect(results[0]).To(Equal("Hello World"))
		Expect(results[1]).To(Equal("alice"))
		Expect(results[2]).To(Equal("42"))
		Expect(results[3]).To(BeNil())
	})

	It("should return correct HLEN for a shopping cart", func() {
		key := "cart:user:456"

		Expect(rdb.HLen(ctx, key).Val()).To(Equal(int64(0)))

		rdb.HSet(ctx, key, "item:apple", "2")
		Expect(rdb.HLen(ctx, key).Val()).To(Equal(int64(1)))

		rdb.HSet(ctx, key, "item:banana", "5")
		Expect(rdb.HLen(ctx, key).Val()).To(Equal(int64(2)))

		// Overwrite quantity
		rdb.HSet(ctx, key, "item:apple", "3")
		Expect(rdb.HLen(ctx, key).Val()).To(Equal(int64(2)))
	})

	It("should HGETALL correctly for a server config", func() {
		key := "config:server:1"
		rdb.HSet(ctx, key, "port", "6379", "timeout", "300", "max_clients", "10000")

		result, err := rdb.HGetAll(ctx, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(result).To(HaveLen(3))
		Expect(result["port"]).To(Equal("6379"))
		Expect(result["timeout"]).To(Equal("300"))
		Expect(result["max_clients"]).To(Equal("10000"))
	})

	It("should return nil/empty for non-existent session", func() {
		key := "session:invalid"

		Expect(rdb.HGet(ctx, key, "token").Err()).To(Equal(redis.Nil))
		Expect(rdb.HLen(ctx, key).Val()).To(Equal(int64(0)))

		all, err := rdb.HGetAll(ctx, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(all).To(BeEmpty())
	})
})
