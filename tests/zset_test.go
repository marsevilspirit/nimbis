package tests

import (
	"context"

	"github.com/marsevilspirit/nimbis/tests/util"
	. "github.com/onsi/ginkgo/v2"
	. "github.com/onsi/gomega"
	"github.com/redis/go-redis/v9"
)

var _ = Describe("ZSet Commands", func() {
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

	It("should ZADD and ZRANGE", func() {
		key := "zset_test_key"
		rdb.Del(ctx, key)

		count, err := rdb.ZAdd(ctx, key, redis.Z{Score: 1.0, Member: "one"}).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(count).To(Equal(int64(1)))

		// Duplicate member update score
		count, err = rdb.ZAdd(ctx, key, redis.Z{Score: 2.0, Member: "one"}).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(count).To(Equal(int64(0)))

		// Add multiple
		count, err = rdb.ZAdd(ctx, key, redis.Z{Score: 2.0, Member: "two"}, redis.Z{Score: 3.0, Member: "three"}).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(count).To(Equal(int64(2)))

		// ZRANGE 0 -1
		vals, err := rdb.ZRange(ctx, key, 0, -1).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(vals).To(Equal([]string{"one", "two", "three"})) // Ordered by score

		// ZRANGE 0 -1 WITHSCORES
		valsWithScores, err := rdb.ZRangeWithScores(ctx, key, 0, -1).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(valsWithScores).To(Equal([]redis.Z{
			{Score: 2.0, Member: "one"},
			{Score: 2.0, Member: "two"},
			{Score: 3.0, Member: "three"},
		}))
	})

	It("should ZSCORE", func() {
		key := "zset_score_key"
		rdb.Del(ctx, key)
		rdb.ZAdd(ctx, key, redis.Z{Score: 1.5, Member: "one"})

		score, err := rdb.ZScore(ctx, key, "one").Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(score).To(Equal(1.5))

		err = rdb.ZScore(ctx, key, "missing").Err()
		Expect(err).To(Equal(redis.Nil))
	})

	It("should ZREM and ZCARD", func() {
		key := "zset_rem_key"
		rdb.Del(ctx, key)
		rdb.ZAdd(ctx, key, redis.Z{Score: 1.0, Member: "one"}, redis.Z{Score: 2.0, Member: "two"})

		card, err := rdb.ZCard(ctx, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(card).To(Equal(int64(2)))

		count, err := rdb.ZRem(ctx, key, "one").Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(count).To(Equal(int64(1)))

		count, err = rdb.ZRem(ctx, key, "one").Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(count).To(Equal(int64(0)))

		card, err = rdb.ZCard(ctx, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(card).To(Equal(int64(1)))
	})
})
