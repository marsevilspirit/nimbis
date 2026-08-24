package tests

import (
	"context"
	"sort"

	"github.com/marsevilspirit/nimbis/e2e-test/util"
	. "github.com/onsi/ginkgo/v2"
	. "github.com/onsi/gomega"
	"github.com/redis/go-redis/v9"
)

var _ = Describe("Set Commands", func() {
	var rdb *redis.Client
	var ctx context.Context

	BeforeEach(func() {
		rdb = util.NewClient()
		ctx = context.Background()
		Expect(rdb.Ping(ctx).Err()).To(Succeed())
		util.Del(ctx, rdb, util.SetType, "myset")
	})

	AfterEach(func() {
		util.Del(ctx, rdb, util.SetType, "myset")
		Expect(rdb.Close()).To(Succeed())
	})

	It("should support SADD, SMEMBERS, SCARD", func() {
		key := "myset"

		// SADD
		n, err := rdb.SAdd(ctx, key, "m1", "m2", "m3").Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(n).To(Equal(int64(3)))

		// Duplicate SADD
		n, err = rdb.SAdd(ctx, key, "m2", "m4").Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(n).To(Equal(int64(1))) // Only m4 is new

		// SCARD
		card, err := rdb.SCard(ctx, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(card).To(Equal(int64(4)))

		// SMEMBERS
		members, err := rdb.SMembers(ctx, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(members).To(HaveLen(4))
		sort.Strings(members)
		Expect(members).To(Equal([]string{"m1", "m2", "m3", "m4"}))
	})

	It("should support SISMEMBER", func() {
		key := "myset"
		rdb.SAdd(ctx, key, "m1")

		isMember, err := rdb.SIsMember(ctx, key, "m1").Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(isMember).To(BeTrue())

		isMember, err = rdb.SIsMember(ctx, key, "m2").Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(isMember).To(BeFalse())
	})

	It("should deduplicate members during initial meta_missing SADD", func() {
		key := "myset_dedup"
		util.Del(ctx, rdb, util.SetType, key)

		// Cold insert with duplicate members in the SAME command
		n, err := rdb.SAdd(ctx, key, "a", "a", "b", "c", "b").Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(n).To(Equal(int64(3))) // Should only add 'a', 'b', 'c' once

		card, err := rdb.SCard(ctx, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(card).To(Equal(int64(3))) // Should not be inflated to 5

		util.Del(ctx, rdb, util.SetType, key)
	})

	It("should support SREM", func() {
		key := "myset"
		rdb.SAdd(ctx, key, "m1", "m2", "m3")

		n, err := rdb.SRem(ctx, key, "m1", "m3", "m4").Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(n).To(Equal(int64(2))) // m1, m3 removed. m4 not found.

		members, err := rdb.SMembers(ctx, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(members).To(Equal([]string{"m2"}))

		// SCARD should update
		card, err := rdb.SCard(ctx, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(card).To(Equal(int64(1)))
	})

	It("should isolate a Set from a same-name String", func() {
		key := "myset_same_name"
		Expect(rdb.Set(ctx, key, "value", 0).Err()).NotTo(HaveOccurred())
		Expect(rdb.SAdd(ctx, key, "m1").Err()).NotTo(HaveOccurred())

		value, err := rdb.Get(ctx, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(value).To(Equal("value"))
		Expect(rdb.SIsMember(ctx, key, "m1").Val()).To(BeTrue())
	})
})
