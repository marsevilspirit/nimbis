package tests

import (
	"context"
	"fmt"
	"strconv"

	"github.com/marsevilspirit/nimbis/tests/util"
	. "github.com/onsi/ginkgo/v2"
	. "github.com/onsi/gomega"
	"github.com/redis/go-redis/v9"
)

const expectedClientID int64 = 1

func normalizeHelloMap(result interface{}) map[string]interface{} {
	switch v := result.(type) {
	case map[string]interface{}:
		return v
	case map[interface{}]interface{}:
		out := make(map[string]interface{}, len(v))
		for key, val := range v {
			out[fmt.Sprint(key)] = val
		}
		return out
	case []interface{}:
		Expect(len(v) % 2).To(Equal(0))
		out := make(map[string]interface{}, len(v)/2)
		for i := 0; i < len(v); i += 2 {
			out[fmt.Sprint(v[i])] = v[i+1]
		}
		return out
	default:
		Fail(fmt.Sprintf("unexpected HELLO result type: %T", result))
		return nil
	}
}

func expectHelloFieldString(m map[string]interface{}, key string, expected string) {
	val, ok := m[key]
	Expect(ok).To(BeTrue())
	Expect(fmt.Sprint(val)).To(Equal(expected))
}

func expectHelloFieldInt(m map[string]interface{}, key string, expected int64) {
	val, ok := m[key]
	Expect(ok).To(BeTrue())
	switch num := val.(type) {
	case int64:
		Expect(num).To(Equal(expected))
	case int:
		Expect(int64(num)).To(Equal(expected))
	default:
		parsed, err := strconv.ParseInt(fmt.Sprint(val), 10, 64)
		Expect(err).NotTo(HaveOccurred())
		Expect(parsed).To(Equal(expected))
	}
}

var _ = Describe("HELLO Command", func() {
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

	It("should support HELLO default", func() {
		result, err := rdb.Do(ctx, "HELLO").Result()
		Expect(err).NotTo(HaveOccurred())

		hello := normalizeHelloMap(result)
		expectHelloFieldString(hello, "server", "nimbis")
		expectHelloFieldInt(hello, "proto", 2)
		expectHelloFieldInt(hello, "id", expectedClientID)
	})

	It("should support HELLO 2", func() {
		result, err := rdb.Do(ctx, "HELLO", "2").Result()
		Expect(err).NotTo(HaveOccurred())

		hello := normalizeHelloMap(result)
		expectHelloFieldString(hello, "server", "nimbis")
		expectHelloFieldInt(hello, "proto", 2)
		expectHelloFieldInt(hello, "id", expectedClientID)
	})

	It("should support HELLO 3", func() {
		result, err := rdb.Do(ctx, "HELLO", "3").Result()
		Expect(err).NotTo(HaveOccurred())

		hello := normalizeHelloMap(result)
		expectHelloFieldString(hello, "server", "nimbis")
		expectHelloFieldInt(hello, "proto", 3)
		expectHelloFieldInt(hello, "id", expectedClientID)
	})

	It("should reject unsupported HELLO protocol version", func() {
		_, err := rdb.Do(ctx, "HELLO", "4").Result()
		Expect(err).To(HaveOccurred())
		Expect(err.Error()).To(ContainSubstring("NOPROTO unsupported protocol version"))
		Expect(err.Error()).To(ContainSubstring("Use 2 or 3"))
	})
})
