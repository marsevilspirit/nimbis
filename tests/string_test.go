package tests

import (
	"context"

	"github.com/marsevilspirit/nimbis/tests/util"
	. "github.com/onsi/ginkgo/v2"
	. "github.com/onsi/gomega"
	"github.com/redis/go-redis/v9"
)

var _ = Describe("Get/Set Commands", func() {
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

	It("should SET and GET a value", func() {
		key := "ginkgo_key"
		val := "ginkgo_value"

		err := rdb.Set(ctx, key, val, 0).Err()
		Expect(err).NotTo(HaveOccurred())

		result, err := rdb.Get(ctx, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(result).To(Equal(val))
	})

	It("should return nil for non-existent key", func() {
		key := "non_existent_key"
		err := rdb.Get(ctx, key).Err()
		Expect(err).To(Equal(redis.Nil))
	})

	It("should INCR and DECR a value", func() {
		key := "counter_key"

		// INCR from non-existent key
		val, err := rdb.Incr(ctx, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(val).To(Equal(int64(1)))

		// INCR again
		val, err = rdb.Incr(ctx, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(val).To(Equal(int64(2)))

		// DECR
		val, err = rdb.Decr(ctx, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(val).To(Equal(int64(1)))

		// DECR again
		val, err = rdb.Decr(ctx, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(val).To(Equal(int64(0)))

		// DECR below zero
		val, err = rdb.Decr(ctx, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(val).To(Equal(int64(-1)))
	})

	It("should return error for INCR/DECR on non-integer value", func() {
		key := "string_key"
		err := rdb.Set(ctx, key, "not_an_integer", 0).Err()
		Expect(err).NotTo(HaveOccurred())

		err = rdb.Incr(ctx, key).Err()
		Expect(err).To(HaveOccurred())
		Expect(err.Error()).To(ContainSubstring("ERR value is not an integer or out of range"))

		err = rdb.Decr(ctx, key).Err()
		Expect(err).To(HaveOccurred())
		Expect(err.Error()).To(ContainSubstring("ERR value is not an integer or out of range"))
	})

	It("should APPEND to a value", func() {
		key := "append_key"

		// APPEND to non-existing key
		len, err := rdb.Append(ctx, key, "Hello").Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(len).To(Equal(int64(5)))

		// APPEND to existing key
		len, err = rdb.Append(ctx, key, " World").Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(len).To(Equal(int64(11)))

		// Verify the final string
		val, err := rdb.Get(ctx, key).Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(val).To(Equal("Hello World"))
	})
})
