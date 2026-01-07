package tests

import (
	"context"
	"time"

	"github.com/marsevilspirit/nimbis/tests/util"
	. "github.com/onsi/ginkgo/v2"
	. "github.com/onsi/gomega"
	"github.com/redis/go-redis/v9"
)

var _ = Describe("Prefix Collision Tests", func() {
	var rdb *redis.Client
	var ctx context.Context

	BeforeEach(func() {
		rdb = util.NewClient()
		ctx = context.Background()
		Expect(rdb.Ping(ctx).Err()).To(Succeed())
		Expect(rdb.FlushDB(ctx).Err()).To(Succeed())
	})

	AfterEach(func() {
		Expect(rdb.Close()).To(Succeed())
	})

	Context("String", func() {
		It("should handle keys where one is prefix of another", func() {
			err := rdb.Set(ctx, "user1", "value1", 0).Err()
			Expect(err).NotTo(HaveOccurred())

			err = rdb.Set(ctx, "user1S", "value1S", 0).Err()
			Expect(err).NotTo(HaveOccurred())

			val, err := rdb.Get(ctx, "user1").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(val).To(Equal("value1"))

			val, err = rdb.Get(ctx, "user1S").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(val).To(Equal("value1S"))
		})
	})

	Context("Hash", func() {
		It("should verify independent keys 'user1' and 'user12'", func() {
			k1 := "user1"
			k2 := "user12"

			Expect(rdb.HSet(ctx, k1, "f1", "v1").Err()).To(Succeed())
			Expect(rdb.HSet(ctx, k2, "f2", "v2").Err()).To(Succeed())

			// Scan HGETALL for user1
			res1, err := rdb.HGetAll(ctx, k1).Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(res1).To(HaveKey("f1"))
			Expect(res1).NotTo(HaveKey("f2"))
			Expect(res1).To(HaveLen(1))

			// Scan HGETALL for user12
			res2, err := rdb.HGetAll(ctx, k2).Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(res2).To(HaveKey("f2"))
			Expect(res2).NotTo(HaveKey("f1"))
			Expect(res2).To(HaveLen(1))
		})
	})

	Context("Set", func() {
		It("should verify independent keys 'user1' and 'user12'", func() {
			k1 := "user1"
			k2 := "user12"

			Expect(rdb.SAdd(ctx, k1, "m1").Err()).To(Succeed())
			Expect(rdb.SAdd(ctx, k2, "m2").Err()).To(Succeed())

			// SMEMBERS user1
			res1, err := rdb.SMembers(ctx, k1).Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(res1).To(ContainElement("m1"))
			Expect(res1).NotTo(ContainElement("m2"))
			Expect(res1).To(HaveLen(1))
		})
	})

	Context("List", func() {
		It("should verify independent keys 'user1' and 'user12'", func() {
			k1 := "user1"
			k2 := "user12"

			Expect(rdb.RPush(ctx, k1, "e1").Err()).To(Succeed())
			Expect(rdb.RPush(ctx, k2, "e2").Err()).To(Succeed())

			// LRANGE user1
			res1, err := rdb.LRange(ctx, k1, 0, -1).Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(res1).To(ContainElement("e1"))
			Expect(res1).NotTo(ContainElement("e2"))
			Expect(res1).To(HaveLen(1))
		})
	})

	Context("ZSet", func() {
		It("should NOT delete 'user12' data when 'user1' expires or is deleted", func() {
			key1 := "user1"
			key2 := "user12"

			err := rdb.ZAdd(ctx, key1, redis.Z{Score: 1, Member: "m1"}).Err()
			Expect(err).NotTo(HaveOccurred())

			err = rdb.ZAdd(ctx, key2, redis.Z{Score: 2, Member: "m2"}).Err()
			Expect(err).NotTo(HaveOccurred())

			// Expire 'user1'
			rdb.Expire(ctx, key1, 1*time.Second)
			time.Sleep(1500 * time.Millisecond)

			// Trigger lazy expiration
			n, err := rdb.Exists(ctx, key1).Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(n).To(Equal(int64(0)))

			// Verify 'user12' still exists and has data
			card, err := rdb.ZCard(ctx, key2).Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(card).To(Equal(int64(1)))

			val, err := rdb.ZScore(ctx, key2, "m2").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(val).To(Equal(2.0))
		})

		It("should NOT list 'user1S' or 'user12' items in ZRANGE 'user1'", func() {
			key1 := "user1"
			key2 := "user12"
			key3 := "user1S"

			rdb.ZAdd(ctx, key1, redis.Z{Score: 1, Member: "m1"})
			rdb.ZAdd(ctx, key2, redis.Z{Score: 2, Member: "m2"})
			rdb.ZAdd(ctx, key3, redis.Z{Score: 3, Member: "m3"})

			// ZRANGE user1
			res, err := rdb.ZRange(ctx, key1, 0, -1).Result()
			Expect(err).NotTo(HaveOccurred())

			Expect(res).To(ContainElement("m1"))
			Expect(res).NotTo(ContainElement("m2"))
			Expect(res).NotTo(ContainElement("m3"))
			Expect(res).To(HaveLen(1))
		})
	})
})
