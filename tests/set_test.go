package tests

import (
	"context"
	"fmt"
	"sort"
	"time"

	"github.com/marsevilspirit/nimbis/tests/util"
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

	It("should handle WRONGTYPE", func() {
		key := "myset_wrongtype"
		rdb.Set(ctx, key, "value", 0)

		err := rdb.SAdd(ctx, key, "m1").Err()
		Expect(err).To(HaveOccurred())
		Expect(err.Error()).To(ContainSubstring("WRONGTYPE"))
	})

	It("should efficiently SMEMBERS without scanning unrelated keys (Performance Test)", func() {
		keyA := "set_A_perf"
		keyZ := "set_Z_perf"

		rdb.Del(ctx, keyA, keyZ)

		// 1. Create setA and immediately delete it
		rdb.SAdd(ctx, keyA, "init")
		rdb.Del(ctx, keyA)

		// 2. Populate setZ (lexicographically after setA) with many elements
		var members []interface{}
		for i := 0; i < 100000; i++ {
			members = append(members, fmt.Sprintf("m%d", i))
		}
		// Add in chunks
		chunkSize := 5000
		for i := 0; i < len(members); i += chunkSize {
			end := i + chunkSize
			if end > len(members) {
				end = len(members)
			}
			rdb.SAdd(ctx, keyZ, members[i:end]...)
		}

		// 3. Re-create setA. The new meta.version will be > all sequences in setZ.
		rdb.SAdd(ctx, keyA, "single_member")

		// 4. Measure SMEMBERS setA
		start := time.Now()
		for i := 0; i < 20; i++ {
			res, err := rdb.SMembers(ctx, keyA).Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(len(res)).To(Equal(1))
		}
		duration := time.Since(start)
		fmt.Printf("\n[DEBUG-TEST] SMEMBERS Performance duration: %v\n", duration)

		rdb.Del(ctx, keyA, keyZ)
	})
})
