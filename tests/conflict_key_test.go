package tests

import (
	"context"

	"github.com/marsevilspirit/nimbis/tests/util"
	. "github.com/onsi/ginkgo/v2"
	. "github.com/onsi/gomega"
	"github.com/redis/go-redis/v9"
)

var _ = Describe("Type Conflict & Persistence", func() {
	var rdb *redis.Client
	var ctx context.Context

	BeforeEach(func() {
		rdb = util.NewClient()
		ctx = context.Background()
		Expect(rdb.Ping(ctx).Err()).To(Succeed())

		// Ensure clean state for keys used in tests
		rdb.Del(ctx, "s_key", "h_key", "conflict_key", "conflict_key_2", "del_key", "complex_key", "edge_key", "key:🔑:special")
	})

	AfterEach(func() {
		Expect(rdb.Close()).To(Succeed())
	})

	Context("String to Hash Conflicts", func() {
		It("should return WRONGTYPE when performing Hash operations on a String key", func() {
			key := "s_key"
			// 1. Setup String
			err := rdb.Set(ctx, key, "value", 0).Err()
			Expect(err).NotTo(HaveOccurred())

			// 2. Hash operations should fail
			// HSET
			err = rdb.HSet(ctx, key, "field", "value").Err()
			Expect(err).To(HaveOccurred())
			Expect(err.Error()).To(ContainSubstring("WRONGTYPE"))

			// HGET
			err = rdb.HGet(ctx, key, "field").Err()
			Expect(err).To(HaveOccurred())
			Expect(err.Error()).To(ContainSubstring("WRONGTYPE"))

			// HMGET
			_, err = rdb.HMGet(ctx, key, "field").Result()
			Expect(err).To(HaveOccurred())
			Expect(err.Error()).To(ContainSubstring("WRONGTYPE"))

			// HGETALL
			_, err = rdb.HGetAll(ctx, key).Result()
			Expect(err).To(HaveOccurred())
			Expect(err.Error()).To(ContainSubstring("WRONGTYPE"))

			// HLEN
			_, err = rdb.HLen(ctx, key).Result()
			Expect(err).To(HaveOccurred())
			Expect(err.Error()).To(ContainSubstring("WRONGTYPE"))

			// 3. String operations should still success
			val, err := rdb.Get(ctx, key).Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(val).To(Equal("value"))
		})
	})

	Context("Hash to String Conflicts", func() {
		It("should return WRONGTYPE when performing String operations on a Hash key", func() {
			key := "h_key"
			// 1. Setup Hash
			err := rdb.HSet(ctx, key, "f1", "v1").Err()
			Expect(err).NotTo(HaveOccurred())

			// 2. String GET should fail
			// Note: SET overwrites (valid), but GET checks type
			err = rdb.Get(ctx, key).Err()
			Expect(err).To(HaveOccurred())
			Expect(err.Error()).To(ContainSubstring("WRONGTYPE"))

			// 3. Hash operations should still success
			val, err := rdb.HGet(ctx, key, "f1").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(val).To(Equal("v1"))

			all, err := rdb.HGetAll(ctx, key).Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(all).To(HaveLen(1))
			Expect(all["f1"]).To(Equal("v1"))
		})
	})

	Context("Type Overwrite with Cleanup", func() {
		It("should properly cleanup Hash fields when overwritten by String", func() {
			key := "conflict_key"

			// 1. Setup Hash with multiple fields
			err := rdb.HSet(ctx, key, "f1", "v1", "f2", "v2").Err()
			Expect(err).NotTo(HaveOccurred())
			Expect(rdb.HLen(ctx, key).Val()).To(Equal(int64(2)))

			// 2. Overwrite with SET (Valid operation in Redis)
			err = rdb.Set(ctx, key, "new_string_val", 0).Err()
			Expect(err).NotTo(HaveOccurred())

			// 3. Verify it is now a String
			val, err := rdb.Get(ctx, key).Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(val).To(Equal("new_string_val"))

			// 4. Verify Hash operation returns WRONGTYPE
			// Note: If the cleanup wasn't done properly, or if only meta was updated,
			// we rely on the implementation. If the implementation checks meta first, it sees String.
			// But we also want to ensure the old data is conceptually 'gone'.
			// In a black-box test, we verify the interface behavior.
			err = rdb.HGet(ctx, key, "f1").Err()
			Expect(err).To(HaveOccurred())
			Expect(err.Error()).To(ContainSubstring("WRONGTYPE"))
		})

		It("should NOT overwrite String with Hash when using HSET", func() {
			// clarification: Redis HSET on existing String key is WRONGTYPE.
			// It does NOT overwrite. Only SET overwrites any type.
			key := "conflict_key_2"

			// 1. Setup String
			rdb.Set(ctx, key, "str_val", 0)

			// 2. Try HSET -> WRONGTYPE
			err := rdb.HSet(ctx, key, "f1", "v1").Err()
			Expect(err).To(HaveOccurred())
			Expect(err.Error()).To(ContainSubstring("WRONGTYPE"))

			// 3. Value remains String
			val, _ := rdb.Get(ctx, key).Result()
			Expect(val).To(Equal("str_val"))
		})
	})

	Context("DEL and Type Switching", func() {
		It("should allow creating different type after DEL", func() {
			key := "del_key"

			// 1. String -> DEL -> Hash
			rdb.Set(ctx, key, "s_val", 0)
			rdb.Del(ctx, key)

			err := rdb.HSet(ctx, key, "f1", "v1").Err()
			Expect(err).NotTo(HaveOccurred())

			val, err := rdb.HGet(ctx, key, "f1").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(val).To(Equal("v1"))

			// 2. Hash -> DEL -> String
			rdb.Del(ctx, key)

			err = rdb.Set(ctx, key, "new_s_val", 0).Err()
			Expect(err).NotTo(HaveOccurred())

			sVal, err := rdb.Get(ctx, key).Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(sVal).To(Equal("new_s_val"))
		})

		It("should handle DEL on non-existent key silently", func() {
			n, err := rdb.Del(ctx, "nonexistent_key").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(n).To(Equal(int64(0)))
		})
	})

	Context("Complex Alternating Operations", func() {
		It("should handle multiple type transitions correctly", func() {
			key := "complex_key"

			// String
			rdb.Set(ctx, key, "1", 0)
			Expect(rdb.Get(ctx, key).Val()).To(Equal("1"))

			// Fail HSET
			Expect(rdb.HSet(ctx, key, "f", "v").Err()).To(HaveOccurred())

			// Overwrite with String again
			rdb.Set(ctx, key, "2", 0)
			Expect(rdb.Get(ctx, key).Val()).To(Equal("2"))

			// DEL
			rdb.Del(ctx, key)

			// Hash
			rdb.HSet(ctx, key, "f", "1")
			Expect(rdb.HGet(ctx, key, "f").Val()).To(Equal("1"))

			// Fail GET
			Expect(rdb.Get(ctx, key).Err()).To(HaveOccurred())

			// Overwrite with SET (Force type change)
			rdb.Set(ctx, key, "3", 0)
			Expect(rdb.Get(ctx, key).Val()).To(Equal("3"))

			// Verify Hash operation fails now
			Expect(rdb.HGet(ctx, key, "f").Err()).To(HaveOccurred())
		})
	})

	Context("List Conflicts", func() {
		It("should return WRONGTYPE when performing List operations on a String key", func() {
			key := "s_list_key"
			// 1. Setup String
			rdb.Set(ctx, key, "value", 0)

			// 2. List operations should fail
			Expect(rdb.LPush(ctx, key, "v").Err()).To(HaveOccurred())
			Expect(rdb.LPush(ctx, key, "v").Err().Error()).To(ContainSubstring("WRONGTYPE"))

			Expect(rdb.RPush(ctx, key, "v").Err()).To(HaveOccurred())
			Expect(rdb.LPop(ctx, key).Err()).To(HaveOccurred())
			Expect(rdb.RPop(ctx, key).Err()).To(HaveOccurred())
			Expect(rdb.LLen(ctx, key).Err()).To(HaveOccurred())
			Expect(rdb.LRange(ctx, key, 0, -1).Err()).To(HaveOccurred())
		})

		It("should return WRONGTYPE when performing String/Hash operations on a List key", func() {
			key := "l_other_key"
			// 1. Setup List
			rdb.LPush(ctx, key, "v1")

			// 2. String operations should fail
			Expect(rdb.Get(ctx, key).Err()).To(HaveOccurred())
			Expect(rdb.Get(ctx, key).Err().Error()).To(ContainSubstring("WRONGTYPE"))

			// 3. Hash operations should fail
			Expect(rdb.HSet(ctx, key, "f", "v").Err()).To(HaveOccurred())
			Expect(rdb.HGet(ctx, key, "f").Err()).To(HaveOccurred())
		})

		It("should overwrite List with SET", func() {
			key := "l_overwrite_key"
			rdb.LPush(ctx, key, "v1")
			Expect(rdb.LLen(ctx, key).Val()).To(Equal(int64(1)))

			// Overwrite
			rdb.Set(ctx, key, "new_val", 0)
			expectVal, _ := rdb.Get(ctx, key).Result()
			Expect(expectVal).To(Equal("new_val"))

			// Old list gone
			Expect(rdb.LLen(ctx, key).Err()).To(HaveOccurred())
		})
	})

	Context("Edge Cases", func() {
		It("should handle empty values", func() {
			key := "edge_key"

			// Empty String
			rdb.Set(ctx, key, "", 0)
			val, err := rdb.Get(ctx, key).Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(val).To(Equal(""))

			// Try HSET -> WRONGTYPE
			Expect(rdb.HSet(ctx, key, "f", "v").Err()).To(HaveOccurred())

			rdb.Del(ctx, key)

			// Empty Hash Field Value
			rdb.HSet(ctx, key, "empty_field", "")
			hVal, err := rdb.HGet(ctx, key, "empty_field").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(hVal).To(Equal(""))

			// Try GET -> WRONGTYPE
			Expect(rdb.Get(ctx, key).Err()).To(HaveOccurred())
		})

		It("should handle special character keys", func() {
			key := "key:🔑:special"

			rdb.HSet(ctx, key, "field.🚀", "val.✨")
			val, err := rdb.HGet(ctx, key, "field.🚀").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(val).To(Equal("val.✨"))

			// Conflict check
			Expect(rdb.Get(ctx, key).Err()).To(HaveOccurred())
		})
	})
})
