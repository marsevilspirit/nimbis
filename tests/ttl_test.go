package tests

import (
	"context"
	"time"

	"github.com/marsevilspirit/nimbis/tests/util"
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
	}

	BeforeEach(func() {
		rdb = util.NewClient()
		ctx = context.Background()
		Expect(rdb.Ping(ctx).Err()).To(Succeed())
		// Clean up potentially conflicting keys
		rdb.Del(ctx, ttlTestKeys...)
	})

	AfterEach(func() {
		rdb.Del(ctx, ttlTestKeys...)
		Expect(rdb.Close()).To(Succeed())
	})

	It("should handle basic EXPIRE and TTL for String", func() {
		key := "expire_key"
		val := "value"

		// 1. Set key
		err := rdb.Set(ctx, key, val, 0).Err()
		Expect(err).NotTo(HaveOccurred())

		// 2. Check TTL (no expiry) -> -1
		ttl, err := rdb.TTL(ctx, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(ttl).To(Equal(time.Duration(-1)))

		// 3. Set Expiry (2 seconds) using EXPIRE cmd
		res, err := rdb.Expire(ctx, key, 2*time.Second).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(res).To(BeTrue())

		// 4. Check TTL -> should be between 0 and 2s
		ttl, err = rdb.TTL(ctx, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(ttl).To(BeNumerically(">", 0))
		Expect(ttl).To(BeNumerically("<=", 2*time.Second))

		// 5. Wait for expiration
		time.Sleep(2500 * time.Millisecond)

		// 6. Check if key is gone
		exists, err := rdb.Exists(ctx, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(exists).To(Equal(int64(0)))

		// 7. Check TTL on missing key -> -2
		ttl, err = rdb.TTL(ctx, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(ttl).To(Equal(time.Duration(-2)))
	})

	It("should handle EXPIRE on non-existent key", func() {
		key := "non_existent_key_expire"
		res, err := rdb.Expire(ctx, key, 10*time.Second).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(res).To(BeFalse())
	})

	It("should handle EXPIRE update", func() {
		key := "expire_update_key"
		rdb.Set(ctx, key, "val", 0)

		// Set 10s
		rdb.Expire(ctx, key, 10*time.Second)
		ttl, _ := rdb.TTL(ctx, key).Result()
		Expect(ttl).To(BeNumerically(">", 8*time.Second))

		// Update to 1s
		res, err := rdb.Expire(ctx, key, 1*time.Second).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(res).To(BeTrue())

		// Check updated TTL
		ttl, _ = rdb.TTL(ctx, key).Result()
		Expect(ttl).To(BeNumerically("<=", 1*time.Second))
	})

	It("should handle basic EXPIRE and TTL for Hash", func() {
		key := "hash_expire_key"

		// 1. HSet
		err := rdb.HSet(ctx, key, "f1", "v1").Err()
		Expect(err).NotTo(HaveOccurred())

		// 2. EXPIRE
		res, err := rdb.Expire(ctx, key, 2*time.Second).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(res).To(BeTrue())

		// 3. TTL check
		ttl, _ := rdb.TTL(ctx, key).Result()
		Expect(ttl).To(BeNumerically(">", 0))

		// 4. Wait
		time.Sleep(2500 * time.Millisecond)

		// 5. HGet -> should be missing
		_, err = rdb.HGet(ctx, key, "f1").Result()
		Expect(err).To(Equal(redis.Nil))

		// 6. Exists -> 0
		exists, _ := rdb.Exists(ctx, key).Result()
		Expect(exists).To(Equal(int64(0)))
	})

	It("should retain TTL after SREM", func() {
		key := "set_ttl_srem_key"
		rdb.SAdd(ctx, key, "m1", "m2")

		rdb.Expire(ctx, key, 10*time.Second)

		ttlBefore, _ := rdb.TTL(ctx, key).Result()
		Expect(ttlBefore).To(BeNumerically(">", 0))

		rdb.SRem(ctx, key, "m1")

		ttlAfter, _ := rdb.TTL(ctx, key).Result()
		Expect(ttlAfter).To(BeNumerically(">", 0))
		Expect(ttlAfter).To(BeNumerically("<=", ttlBefore))
	})

	It("should retain TTL after HSET", func() {
		key := "hash_ttl_hset_key"
		rdb.HSet(ctx, key, "f1", "v1")
		rdb.Expire(ctx, key, 10*time.Second)

		ttlBefore, _ := rdb.TTL(ctx, key).Result()
		Expect(ttlBefore).To(BeNumerically(">", 0))

		rdb.HSet(ctx, key, "f2", "v2")

		ttlAfter, _ := rdb.TTL(ctx, key).Result()
		Expect(ttlAfter).To(BeNumerically(">", 0))
		Expect(ttlAfter).To(BeNumerically("<=", ttlBefore))
	})

	It("should retain TTL after LPUSH", func() {
		key := "list_ttl_lpush_key"
		rdb.LPush(ctx, key, "m1")
		rdb.Expire(ctx, key, 10*time.Second)

		ttlBefore, _ := rdb.TTL(ctx, key).Result()
		Expect(ttlBefore).To(BeNumerically(">", 0))

		rdb.LPush(ctx, key, "m2")

		ttlAfter, _ := rdb.TTL(ctx, key).Result()
		Expect(ttlAfter).To(BeNumerically(">", 0))
		Expect(ttlAfter).To(BeNumerically("<=", ttlBefore))
	})

	It("should retain TTL after ZADD", func() {
		key := "zset_ttl_zadd_key"
		rdb.ZAdd(ctx, key, redis.Z{Score: 1.0, Member: "m1"})
		rdb.Expire(ctx, key, 10*time.Second)

		ttlBefore, _ := rdb.TTL(ctx, key).Result()
		Expect(ttlBefore).To(BeNumerically(">", 0))

		rdb.ZAdd(ctx, key, redis.Z{Score: 2.0, Member: "m2"})

		ttlAfter, _ := rdb.TTL(ctx, key).Result()
		Expect(ttlAfter).To(BeNumerically(">", 0))
		Expect(ttlAfter).To(BeNumerically("<=", ttlBefore))
	})
})
