package tests

import (
	"context"
	"time"

	"github.com/marsevilspirit/nimbis/e2e-test/util"
	. "github.com/onsi/ginkgo/v2"
	. "github.com/onsi/gomega"
	"github.com/redis/go-redis/v9"
)

var _ = Describe("Same-Name Typed Keyspaces", func() {
	var rdb *redis.Client
	var ctx context.Context

	testKeys := []string{
		"same_name_all_types",
		"same_name_mutations",
		"same_name_del",
		"same_name_del_a",
		"same_name_del_b",
		"same_name_expire",
		"key:shared:🔑",
	}

	seedAllTypes := func(key string) {
		Expect(rdb.Set(ctx, key, "string-v1", 0).Err()).To(Succeed())
		Expect(rdb.HSet(ctx, key, "field-1", "hash-v1").Err()).To(Succeed())
		Expect(rdb.RPush(ctx, key, "list-1", "list-2").Err()).To(Succeed())
		Expect(rdb.SAdd(ctx, key, "set-1", "set-2").Err()).To(Succeed())
		Expect(rdb.ZAdd(ctx, key,
			redis.Z{Score: 1, Member: "zset-1"},
			redis.Z{Score: 2, Member: "zset-2"},
		).Err()).To(Succeed())
	}

	expectSeededTypes := func(key string) {
		value, err := rdb.Get(ctx, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(value).To(Equal("string-v1"))

		hashValue, err := rdb.HGet(ctx, key, "field-1").Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(hashValue).To(Equal("hash-v1"))

		listValues, err := rdb.LRange(ctx, key, 0, -1).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(listValues).To(Equal([]string{"list-1", "list-2"}))

		setValues, err := rdb.SMembers(ctx, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(setValues).To(ConsistOf("set-1", "set-2"))

		zsetValues, err := rdb.ZRange(ctx, key, 0, -1).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(zsetValues).To(Equal([]string{"zset-1", "zset-2"}))
	}

	expectTypedExists := func(keyType util.KeyType, key string, expected int64) {
		exists, err := util.Exists(ctx, rdb, keyType, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(exists).To(Equal(expected))
	}

	BeforeEach(func() {
		rdb = util.NewClient()
		ctx = context.Background()
		Expect(rdb.Ping(ctx).Err()).To(Succeed())
		for _, keyType := range []util.KeyType{
			util.StringType,
			util.HashType,
			util.ListType,
			util.SetType,
			util.ZSetType,
		} {
			Expect(util.Del(ctx, rdb, keyType, testKeys...).Err()).To(Succeed())
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
			Expect(util.Del(ctx, rdb, keyType, testKeys...).Err()).To(Succeed())
		}
		Expect(rdb.Close()).To(Succeed())
	})

	It("allows String, Hash, List, Set, and ZSet values to share one name", func() {
		key := "same_name_all_types"
		seedAllTypes(key)

		expectSeededTypes(key)

		expectTypedExists(util.StringType, key, 1)
		expectTypedExists(util.HashType, key, 1)
		expectTypedExists(util.ListType, key, 1)
		expectTypedExists(util.SetType, key, 1)
		expectTypedExists(util.ZSetType, key, 1)
	})

	It("isolates every type-specific mutation to its own namespace", func() {
		key := "same_name_mutations"
		seedAllTypes(key)

		Expect(rdb.Set(ctx, key, "string-v2", 0).Err()).To(Succeed())
		Expect(rdb.HSet(ctx, key, "field-1", "hash-v2", "field-2", "hash-v3").Err()).To(Succeed())
		Expect(rdb.LPush(ctx, key, "list-0").Err()).To(Succeed())
		Expect(rdb.SRem(ctx, key, "set-1").Err()).To(Succeed())
		Expect(rdb.SAdd(ctx, key, "set-3").Err()).To(Succeed())
		Expect(rdb.ZRem(ctx, key, "zset-1").Err()).To(Succeed())
		Expect(rdb.ZAdd(ctx, key, redis.Z{Score: 3, Member: "zset-3"}).Err()).To(Succeed())

		value, err := rdb.Get(ctx, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(value).To(Equal("string-v2"))

		hashValues, err := rdb.HGetAll(ctx, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(hashValues).To(Equal(map[string]string{
			"field-1": "hash-v2",
			"field-2": "hash-v3",
		}))

		listValues, err := rdb.LRange(ctx, key, 0, -1).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(listValues).To(Equal([]string{"list-0", "list-1", "list-2"}))

		setValues, err := rdb.SMembers(ctx, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(setValues).To(ConsistOf("set-2", "set-3"))

		zsetValues, err := rdb.ZRange(ctx, key, 0, -1).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(zsetValues).To(Equal([]string{"zset-2", "zset-3"}))
	})

	It("routes DEL to exactly one same-name namespace", func() {
		key := "same_name_del"
		seedAllTypes(key)

		deleted, err := util.Del(ctx, rdb, util.HashType, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(deleted).To(Equal(int64(1)))

		_, err = rdb.HGet(ctx, key, "field-1").Result()
		Expect(err).To(Equal(redis.Nil))
		expectTypedExists(util.HashType, key, 0)
		expectTypedExists(util.StringType, key, 1)
		expectTypedExists(util.ListType, key, 1)
		expectTypedExists(util.SetType, key, 1)
		expectTypedExists(util.ZSetType, key, 1)
		Expect(rdb.Get(ctx, key).Val()).To(Equal("string-v1"))
		Expect(rdb.LRange(ctx, key, 0, -1).Val()).To(Equal([]string{"list-1", "list-2"}))
		Expect(rdb.SMembers(ctx, key).Val()).To(ConsistOf("set-1", "set-2"))
		Expect(rdb.ZRange(ctx, key, 0, -1).Val()).To(Equal([]string{"zset-1", "zset-2"}))

		deleted, err = util.Del(ctx, rdb, util.HashType, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(deleted).To(Equal(int64(0)))
	})

	It("allows multi-key DEL only within its selected type", func() {
		firstKey := "same_name_del_a"
		secondKey := "same_name_del_b"
		seedAllTypes(firstKey)
		seedAllTypes(secondKey)

		deleted, err := util.Del(
			ctx,
			rdb,
			util.SetType,
			firstKey,
			secondKey,
			"same_name_missing",
		).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(deleted).To(Equal(int64(2)))
		expectTypedExists(util.SetType, firstKey, 0)
		expectTypedExists(util.SetType, secondKey, 0)
		expectTypedExists(util.StringType, firstKey, 1)
		expectTypedExists(util.HashType, secondKey, 1)
		expectTypedExists(util.ListType, firstKey, 1)
		expectTypedExists(util.ZSetType, secondKey, 1)
	})

	It("routes EXPIRE and EXISTS to exactly one same-name namespace", func() {
		key := "same_name_expire"
		seedAllTypes(key)

		expired, err := util.Expire(ctx, rdb, util.HashType, key, 2*time.Second).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(expired).To(BeTrue())
		expectSeededTypes(key)

		Eventually(func(g Gomega) {
			exists, err := util.Exists(ctx, rdb, util.HashType, key).Result()
			g.Expect(err).NotTo(HaveOccurred())
			g.Expect(exists).To(Equal(int64(0)))
		}, 5*time.Second, 50*time.Millisecond).Should(Succeed())

		_, err = rdb.HGet(ctx, key, "field-1").Result()
		Expect(err).To(Equal(redis.Nil))
		expectTypedExists(util.StringType, key, 1)
		expectTypedExists(util.ListType, key, 1)
		expectTypedExists(util.SetType, key, 1)
		expectTypedExists(util.ZSetType, key, 1)
	})

	It("supports same-name typed values with Unicode keys and payloads", func() {
		key := "key:shared:🔑"
		Expect(rdb.Set(ctx, key, "string:✨", 0).Err()).To(Succeed())
		Expect(rdb.HSet(ctx, key, "field:🚀", "hash:🌙").Err()).To(Succeed())
		Expect(rdb.RPush(ctx, key, "list:星").Err()).To(Succeed())
		Expect(rdb.SAdd(ctx, key, "set:云").Err()).To(Succeed())
		Expect(rdb.ZAdd(ctx, key, redis.Z{Score: 1, Member: "zset:雨"}).Err()).To(Succeed())

		Expect(rdb.Get(ctx, key).Val()).To(Equal("string:✨"))
		Expect(rdb.HGet(ctx, key, "field:🚀").Val()).To(Equal("hash:🌙"))
		Expect(rdb.LRange(ctx, key, 0, -1).Val()).To(Equal([]string{"list:星"}))
		Expect(rdb.SMembers(ctx, key).Val()).To(ConsistOf("set:云"))
		Expect(rdb.ZRange(ctx, key, 0, -1).Val()).To(Equal([]string{"zset:雨"}))
	})
})
