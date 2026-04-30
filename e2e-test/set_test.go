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
		rdb.Del(ctx, "myset")
	})

	AfterEach(func() {
		rdb.Del(ctx, "myset")
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
		rdb.Del(ctx, key)

		// Cold insert with duplicate members in the SAME command
		n, err := rdb.SAdd(ctx, key, "a", "a", "b", "c", "b").Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(n).To(Equal(int64(3))) // Should only add 'a', 'b', 'c' once

		card, err := rdb.SCard(ctx, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(card).To(Equal(int64(3))) // Should not be inflated to 5

		rdb.Del(ctx, key)
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

	It("should support SUNION, SINTER, and SDIFF across workers", func() {
		key1, key2 := findCrossShardKeys(2)
		Expect(rdb.Del(ctx, key1, key2).Err()).To(Succeed())

		_, err := rdb.SAdd(ctx, key1, "a", "b", "c").Result()
		Expect(err).NotTo(HaveOccurred())
		_, err = rdb.SAdd(ctx, key2, "b", "c", "d").Result()
		Expect(err).NotTo(HaveOccurred())

		union, err := rdb.SUnion(ctx, key1, key2).Result()
		Expect(err).NotTo(HaveOccurred())
		sort.Strings(union)
		Expect(union).To(Equal([]string{"a", "b", "c", "d"}))

		inter, err := rdb.SInter(ctx, key1, key2).Result()
		Expect(err).NotTo(HaveOccurred())
		sort.Strings(inter)
		Expect(inter).To(Equal([]string{"b", "c"}))

		diff, err := rdb.SDiff(ctx, key1, key2).Result()
		Expect(err).NotTo(HaveOccurred())
		sort.Strings(diff)
		Expect(diff).To(Equal([]string{"a"}))

		inter, err = rdb.SInter(ctx, key1, "missing_set_key").Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(inter).To(BeEmpty())
	})

	It("should handle WRONGTYPE", func() {
		key := "myset_wrongtype"
		rdb.Set(ctx, key, "value", 0)

		err := rdb.SAdd(ctx, key, "m1").Err()
		Expect(err).To(HaveOccurred())
		Expect(err.Error()).To(ContainSubstring("WRONGTYPE"))
	})
})
