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
})
