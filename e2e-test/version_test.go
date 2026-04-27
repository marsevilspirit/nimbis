package tests

import (
	"context"
	"fmt"
	"sort"

	"github.com/marsevilspirit/nimbis/e2e-test/util"
	. "github.com/onsi/ginkgo/v2"
	. "github.com/onsi/gomega"
	"github.com/redis/go-redis/v9"
)

var _ = Describe("Version Isolation", func() {
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

	// Test that after DEL + re-create, old data is invisible (version isolation)
	Describe("Set version isolation", func() {
		It("should not leak old members after DEL and re-create", func() {
			key := "version_set_test"
			rdb.Del(ctx, key)

			// 1. Create a set with members
			n, err := rdb.SAdd(ctx, key, "old_m1", "old_m2", "old_m3").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(n).To(Equal(int64(3)))

			// 2. DEL the set (logical delete, O(1))
			deleted, err := rdb.Del(ctx, key).Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(deleted).To(Equal(int64(1)))

			// 3. Verify set is gone
			exists, err := rdb.Exists(ctx, key).Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(exists).To(Equal(int64(0)))

			// 4. Re-create set with new members
			n, err = rdb.SAdd(ctx, key, "new_m1", "new_m2").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(n).To(Equal(int64(2)))

			// 5. Verify ONLY new members are visible (no old data leaking)
			members, err := rdb.SMembers(ctx, key).Result()
			Expect(err).NotTo(HaveOccurred())
			sort.Strings(members)
			Expect(members).To(Equal([]string{"new_m1", "new_m2"}))

			// 6. SCARD should be 2, not 5
			card, err := rdb.SCard(ctx, key).Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(card).To(Equal(int64(2)))

			// Cleanup
			rdb.Del(ctx, key)
		})
	})

	Describe("Hash version isolation", func() {
		It("should not leak old fields after DEL and re-create", func() {
			key := "version_hash_test"
			rdb.Del(ctx, key)

			// 1. Create a hash with fields
			err := rdb.HSet(ctx, key, "old_f1", "v1", "old_f2", "v2").Err()
			Expect(err).NotTo(HaveOccurred())

			// 2. DEL the hash
			deleted, err := rdb.Del(ctx, key).Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(deleted).To(Equal(int64(1)))

			// 3. Re-create hash
			err = rdb.HSet(ctx, key, "new_f1", "v3").Err()
			Expect(err).NotTo(HaveOccurred())

			// 4. Verify ONLY new fields are visible
			fields, err := rdb.HGetAll(ctx, key).Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(fields).To(Equal(map[string]string{"new_f1": "v3"}))

			// 5. HLEN should be 1, not 3
			hlen, err := rdb.HLen(ctx, key).Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(hlen).To(Equal(int64(1)))

			// 6. Old fields should return nil
			val, err := rdb.HGet(ctx, key, "old_f1").Result()
			Expect(err).To(Equal(redis.Nil))
			Expect(val).To(BeEmpty())

			// Cleanup
			rdb.Del(ctx, key)
		})
	})

	Describe("ZSet version isolation", func() {
		It("should not leak old members after DEL and re-create", func() {
			key := "version_zset_test"
			rdb.Del(ctx, key)

			// 1. Create a sorted set
			n, err := rdb.ZAdd(ctx, key,
				redis.Z{Score: 1.0, Member: "old_z1"},
				redis.Z{Score: 2.0, Member: "old_z2"},
			).Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(n).To(Equal(int64(2)))

			// 2. DEL the zset
			deleted, err := rdb.Del(ctx, key).Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(deleted).To(Equal(int64(1)))

			// 3. Re-create zset
			n, err = rdb.ZAdd(ctx, key,
				redis.Z{Score: 10.0, Member: "new_z1"},
			).Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(n).To(Equal(int64(1)))

			// 4. Verify ONLY new members
			members, err := rdb.ZRangeWithScores(ctx, key, 0, -1).Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(members).To(HaveLen(1))
			Expect(members[0].Member).To(Equal("new_z1"))
			Expect(members[0].Score).To(Equal(10.0))

			// 5. ZCARD should be 1, not 3
			card, err := rdb.ZCard(ctx, key).Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(card).To(Equal(int64(1)))

			// Cleanup
			rdb.Del(ctx, key)
		})
	})

	Describe("List version isolation", func() {
		It("should not leak old elements after DEL and re-create", func() {
			key := "version_list_test"
			rdb.Del(ctx, key)

			// 1. Create a list
			n, err := rdb.RPush(ctx, key, "old_e1", "old_e2", "old_e3").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(n).To(Equal(int64(3)))

			// 2. DEL the list
			deleted, err := rdb.Del(ctx, key).Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(deleted).To(Equal(int64(1)))

			// 3. Re-create list
			n, err = rdb.RPush(ctx, key, "new_e1").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(n).To(Equal(int64(1)))

			// 4. Verify ONLY new elements
			elems, err := rdb.LRange(ctx, key, 0, -1).Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(elems).To(Equal([]string{"new_e1"}))

			// 5. LLEN should be 1, not 4
			llen, err := rdb.LLen(ctx, key).Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(llen).To(Equal(int64(1)))

			// Cleanup
			rdb.Del(ctx, key)
		})
	})

	Describe("Non-recreate updates", func() {
		It("should keep existing set members visible across normal updates", func() {
			key := "version_set_non_recreate_test"
			rdb.Del(ctx, key)

			// Initial generation
			n, err := rdb.SAdd(ctx, key, "m1", "m2").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(n).To(Equal(int64(2)))

			// Non-recreate updates: duplicate add + new member
			n, err = rdb.SAdd(ctx, key, "m2", "m3").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(n).To(Equal(int64(1)))

			members, err := rdb.SMembers(ctx, key).Result()
			Expect(err).NotTo(HaveOccurred())
			sort.Strings(members)
			Expect(members).To(Equal([]string{"m1", "m2", "m3"}))

			// Remove one and add one more in the same generation
			removed, err := rdb.SRem(ctx, key, "m2").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(removed).To(Equal(int64(1)))

			n, err = rdb.SAdd(ctx, key, "m4").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(n).To(Equal(int64(1)))

			members, err = rdb.SMembers(ctx, key).Result()
			Expect(err).NotTo(HaveOccurred())
			sort.Strings(members)
			Expect(members).To(Equal([]string{"m1", "m3", "m4"}))

			card, err := rdb.SCard(ctx, key).Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(card).To(Equal(int64(3)))

			rdb.Del(ctx, key)
		})

		It("should keep existing zset members visible across score updates", func() {
			key := "version_zset_non_recreate_test"
			rdb.Del(ctx, key)

			n, err := rdb.ZAdd(ctx, key,
				redis.Z{Score: 1.0, Member: "m1"},
				redis.Z{Score: 2.0, Member: "m2"},
			).Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(n).To(Equal(int64(2)))

			// Non-recreate update: update score of existing member
			n, err = rdb.ZAdd(ctx, key,
				redis.Z{Score: 5.0, Member: "m1"},
			).Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(n).To(Equal(int64(0)))

			// Add a new member in same generation
			n, err = rdb.ZAdd(ctx, key,
				redis.Z{Score: 3.0, Member: "m3"},
			).Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(n).To(Equal(int64(1)))

			score1, err := rdb.ZScore(ctx, key, "m1").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(score1).To(Equal(5.0))

			score2, err := rdb.ZScore(ctx, key, "m2").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(score2).To(Equal(2.0))

			score3, err := rdb.ZScore(ctx, key, "m3").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(score3).To(Equal(3.0))

			card, err := rdb.ZCard(ctx, key).Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(card).To(Equal(int64(3)))

			rdb.Del(ctx, key)
		})

		It("should keep existing list elements visible across push and pop", func() {
			key := "version_list_non_recreate_test"
			rdb.Del(ctx, key)

			n, err := rdb.RPush(ctx, key, "a", "b").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(n).To(Equal(int64(2)))

			// Non-recreate update: push new element
			n, err = rdb.RPush(ctx, key, "c").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(n).To(Equal(int64(3)))

			// Non-recreate update: pop one element
			popped, err := rdb.LPop(ctx, key).Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(popped).To(Equal("a"))

			// Non-recreate update: append again
			n, err = rdb.RPush(ctx, key, "d").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(n).To(Equal(int64(3)))

			elems, err := rdb.LRange(ctx, key, 0, -1).Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(elems).To(Equal([]string{"b", "c", "d"}))

			llen, err := rdb.LLen(ctx, key).Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(llen).To(Equal(int64(3)))

			rdb.Del(ctx, key)
		})
	})

	// Stress test: rapid create-delete cycles should not accumulate visible data
	Describe("Rapid create-delete cycles", func() {
		It("should not accumulate stale data across many cycles", func() {
			key := "version_stress_test"
			rdb.Del(ctx, key)

			// Perform 20 create-delete cycles
			for i := 0; i < 20; i++ {
				// SADD
				members := []interface{}{
					fmt.Sprintf("m_%d_a", i),
					fmt.Sprintf("m_%d_b", i),
					fmt.Sprintf("m_%d_c", i),
				}
				n, err := rdb.SAdd(ctx, key, members...).Result()
				Expect(err).NotTo(HaveOccurred())
				Expect(n).To(Equal(int64(3)))

				// DEL
				deleted, err := rdb.Del(ctx, key).Result()
				Expect(err).NotTo(HaveOccurred())
				Expect(deleted).To(Equal(int64(1)))
			}

			// After all cycles, key should not exist
			exists, err := rdb.Exists(ctx, key).Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(exists).To(Equal(int64(0)))

			// Final create with known data
			n, err := rdb.SAdd(ctx, key, "final_member").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(n).To(Equal(int64(1)))

			// Verify ONLY the final member is visible
			members, err := rdb.SMembers(ctx, key).Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(members).To(Equal([]string{"final_member"}))

			card, err := rdb.SCard(ctx, key).Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(card).To(Equal(int64(1)))

			// Cleanup
			rdb.Del(ctx, key)
		})
	})
})
