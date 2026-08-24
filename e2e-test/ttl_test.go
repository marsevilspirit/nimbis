package tests

import (
	"context"
	"time"

	"github.com/marsevilspirit/nimbis/e2e-test/util"
	. "github.com/onsi/ginkgo/v2"
	. "github.com/onsi/gomega"
	"github.com/redis/go-redis/v9"
)

var _ = Describe("Expire/TTL Commands", func() {
	var rdb *redis.Client
	var ctx context.Context

	ttlTestKeys := []string{
		"expire_key",
		"no_expire_key",
		"hash_expire_key",
		"set_ttl_srem_key",
		"hash_ttl_hset_key",
		"list_ttl_lpush_key",
		"zset_ttl_zadd_key",
		"expire_update_key",
		"non_existent_key_expire",
	}

	BeforeEach(func() {
		rdb = util.NewClient()
		ctx = context.Background()
		Expect(rdb.Ping(ctx).Err()).To(Succeed())
		// Clean up potentially conflicting keys
		for _, keyType := range []util.KeyType{
			util.StringType,
			util.HashType,
			util.ListType,
			util.SetType,
			util.ZSetType,
		} {
			util.Del(ctx, rdb, keyType, ttlTestKeys...)
		}
	})

	AfterEach(func() {
		for _, keyType := range []util.KeyType{
			util.StringType,
			util.HashType,
			util.ListType,
			util.SetType,
			util.ZSetType,
		} {
			util.Del(ctx, rdb, keyType, ttlTestKeys...)
		}
		Expect(rdb.Close()).To(Succeed())
	})

	It("should handle basic EXPIRE and TTL for String", func() {
		key := "expire_key"
		val := "value"

		// 1. Set key
		err := rdb.Set(ctx, key, val, 0).Err()
		Expect(err).NotTo(HaveOccurred())

		// 2. Check TTL (no expiry) -> -1
		ttl, err := util.TTL(ctx, rdb, util.StringType, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(ttl).To(Equal(time.Duration(-1)))

		// 3. Set Expiry (2 seconds) using EXPIRE cmd
		res, err := util.Expire(ctx, rdb, util.StringType, key, 2*time.Second).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(res).To(BeTrue())

		// 4. Check TTL -> should be between 0 and 2s
		ttl, err = util.TTL(ctx, rdb, util.StringType, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(ttl).To(BeNumerically(">", 0))
		Expect(ttl).To(BeNumerically("<=", 2*time.Second))

		// 5. Wait for expiration
		time.Sleep(2500 * time.Millisecond)

		// 6. Check if key is gone
		exists, err := util.Exists(ctx, rdb, util.StringType, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(exists).To(Equal(int64(0)))

		// 7. Check TTL on missing key -> -2
		ttl, err = util.TTL(ctx, rdb, util.StringType, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(ttl).To(Equal(time.Duration(-2)))
	})

	It("should count multiple keys in EXISTS", func() {
		Expect(rdb.Set(ctx, "expire_key", "value", 0).Err()).NotTo(HaveOccurred())
		Expect(rdb.Set(ctx, "no_expire_key", "value", 0).Err()).NotTo(HaveOccurred())

		exists, err := util.Exists(
			ctx,
			rdb,
			util.StringType,
			"expire_key",
			"no_expire_key",
			"missing",
		).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(exists).To(Equal(int64(2)))
	})

	It("should handle EXPIRE on non-existent key", func() {
		key := "non_existent_key_expire"
		res, err := util.Expire(ctx, rdb, util.StringType, key, 10*time.Second).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(res).To(BeFalse())
	})

	It("should handle EXPIRE update", func() {
		key := "expire_update_key"
		rdb.Set(ctx, key, "val", 0)

		// Set 10s
		util.Expire(ctx, rdb, util.StringType, key, 10*time.Second)
		ttl, _ := util.TTL(ctx, rdb, util.StringType, key).Result()
		Expect(ttl).To(BeNumerically(">", 8*time.Second))

		// Update to 1s
		res, err := util.Expire(ctx, rdb, util.StringType, key, time.Second).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(res).To(BeTrue())

		// Check updated TTL
		ttl, _ = util.TTL(ctx, rdb, util.StringType, key).Result()
		Expect(ttl).To(BeNumerically("<=", 1*time.Second))
	})

	It("should handle basic EXPIRE and TTL for Hash", func() {
		key := "hash_expire_key"

		// 1. HSet
		err := rdb.HSet(ctx, key, "f1", "v1").Err()
		Expect(err).NotTo(HaveOccurred())

		// 2. EXPIRE
		res, err := util.Expire(ctx, rdb, util.HashType, key, 2*time.Second).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(res).To(BeTrue())

		// 3. TTL check
		ttl, _ := util.TTL(ctx, rdb, util.HashType, key).Result()
		Expect(ttl).To(BeNumerically(">", 0))

		// 4. Wait
		time.Sleep(2500 * time.Millisecond)

		// 5. HGet -> should be missing
		_, err = rdb.HGet(ctx, key, "f1").Result()
		Expect(err).To(Equal(redis.Nil))

		// 6. Exists -> 0
		exists, _ := util.Exists(ctx, rdb, util.HashType, key).Result()
		Expect(exists).To(Equal(int64(0)))
	})

	It("should retain TTL after SREM", func() {
		key := "set_ttl_srem_key"
		_, err := rdb.SAdd(ctx, key, "m1", "m2").Result()
		Expect(err).NotTo(HaveOccurred())

		res, err := util.Expire(ctx, rdb, util.SetType, key, 10*time.Second).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(res).To(BeTrue())

		ttlBefore, err := util.TTL(ctx, rdb, util.SetType, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(ttlBefore).To(BeNumerically(">", 0))

		_, err = rdb.SRem(ctx, key, "m1").Result()
		Expect(err).NotTo(HaveOccurred())

		ttlAfter, err := util.TTL(ctx, rdb, util.SetType, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(ttlAfter).To(BeNumerically(">", 0))
		Expect(ttlAfter).To(BeNumerically("<=", ttlBefore))
	})

	It("should retain TTL after HSET", func() {
		key := "hash_ttl_hset_key"
		err := rdb.HSet(ctx, key, "f1", "v1").Err()
		Expect(err).NotTo(HaveOccurred())

		res, err := util.Expire(ctx, rdb, util.HashType, key, 10*time.Second).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(res).To(BeTrue())

		ttlBefore, err := util.TTL(ctx, rdb, util.HashType, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(ttlBefore).To(BeNumerically(">", 0))

		err = rdb.HSet(ctx, key, "f2", "v2").Err()
		Expect(err).NotTo(HaveOccurred())

		ttlAfter, err := util.TTL(ctx, rdb, util.HashType, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(ttlAfter).To(BeNumerically(">", 0))
		Expect(ttlAfter).To(BeNumerically("<=", ttlBefore))
	})

	It("should retain TTL after LPUSH", func() {
		key := "list_ttl_lpush_key"
		_, err := rdb.LPush(ctx, key, "m1").Result()
		Expect(err).NotTo(HaveOccurred())

		res, err := util.Expire(ctx, rdb, util.ListType, key, 10*time.Second).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(res).To(BeTrue())

		ttlBefore, err := util.TTL(ctx, rdb, util.ListType, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(ttlBefore).To(BeNumerically(">", 0))

		_, err = rdb.LPush(ctx, key, "m2").Result()
		Expect(err).NotTo(HaveOccurred())

		ttlAfter, err := util.TTL(ctx, rdb, util.ListType, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(ttlAfter).To(BeNumerically(">", 0))
		Expect(ttlAfter).To(BeNumerically("<=", ttlBefore))
	})

	It("should retain TTL after ZADD", func() {
		key := "zset_ttl_zadd_key"
		_, err := rdb.ZAdd(ctx, key, redis.Z{Score: 1.0, Member: "m1"}).Result()
		Expect(err).NotTo(HaveOccurred())

		res, err := util.Expire(ctx, rdb, util.ZSetType, key, 10*time.Second).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(res).To(BeTrue())

		ttlBefore, err := util.TTL(ctx, rdb, util.ZSetType, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(ttlBefore).To(BeNumerically(">", 0))

		_, err = rdb.ZAdd(ctx, key, redis.Z{Score: 2.0, Member: "m2"}).Result()
		Expect(err).NotTo(HaveOccurred())

		ttlAfter, err := util.TTL(ctx, rdb, util.ZSetType, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(ttlAfter).To(BeNumerically(">", 0))
		Expect(ttlAfter).To(BeNumerically("<=", ttlBefore))
	})
})
