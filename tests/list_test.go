package tests

import (
	"context"

	"github.com/marsevilspirit/nimbis/tests/util"
	. "github.com/onsi/ginkgo/v2"
	. "github.com/onsi/gomega"
	"github.com/redis/go-redis/v9"
)

var _ = Describe("List Commands", func() {
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

	It("should LPUSH and LPOP correctly", func() {
		key := "mylist_l"

		// LPUSH
		res, err := rdb.LPush(ctx, key, "v1", "v2").Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(res).To(Equal(int64(2)))

		// LPOP
		val, err := rdb.LPop(ctx, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(val).To(Equal("v2")) // v2 is pushed last on left, so it's first

		val, err = rdb.LPop(ctx, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(val).To(Equal("v1"))

		// Empty
		_, err = rdb.LPop(ctx, key).Result()
		Expect(err).To(Equal(redis.Nil))
	})

	It("should RPUSH and RPOP correctly", func() {
		key := "mylist_r"

		// RPUSH
		res, err := rdb.RPush(ctx, key, "v1", "v2").Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(res).To(Equal(int64(2)))

		// RPOP
		val, err := rdb.RPop(ctx, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(val).To(Equal("v2")) // v2 is pushed last on right, so it's last

		val, err = rdb.RPop(ctx, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(val).To(Equal("v1"))
	})

	It("should LLEN correctly", func() {
		key := "mylist_len"
		Expect(rdb.LLen(ctx, key).Val()).To(Equal(int64(0)))

		rdb.LPush(ctx, key, "v1")
		Expect(rdb.LLen(ctx, key).Val()).To(Equal(int64(1)))

		rdb.LPush(ctx, key, "v2")
		Expect(rdb.LLen(ctx, key).Val()).To(Equal(int64(2)))

		// Cleanup with RPOP
		rdb.RPop(ctx, key)
		Expect(rdb.LLen(ctx, key).Val()).To(Equal(int64(1)))
	})

	It("should LRANGE correctly", func() {
		key := "mylist_range"
		// [1, 2, 3] logic
		// RPUSH 1, 2, 3 -> [1, 2, 3]
		rdb.RPush(ctx, key, "1", "2", "3")

		// LRANGE 0 -1 -> [1, 2, 3]
		res, err := rdb.LRange(ctx, key, 0, -1).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(res).To(HaveLen(3))
		Expect(res[0]).To(Equal("1"))
		Expect(res[1]).To(Equal("2"))
		Expect(res[2]).To(Equal("3"))

		// LRANGE 0 1 -> [1, 2]
		res, err = rdb.LRange(ctx, key, 0, 1).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(res).To(HaveLen(2))
		Expect(res[0]).To(Equal("1"))
		Expect(res[1]).To(Equal("2"))
	})
})
