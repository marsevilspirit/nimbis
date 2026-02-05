package tests

import (
	"context"
	"runtime"
	"strconv"

	"github.com/marsevilspirit/nimbis/tests/util"
	. "github.com/onsi/ginkgo/v2"
	. "github.com/onsi/gomega"
	"github.com/redis/go-redis/v9"
)

var _ = Describe("CONFIG Commands", func() {
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

	Describe("CONFIG GET", func() {
		It("should get the server address", func() {
			result, err := rdb.ConfigGet(ctx, "addr").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(result).To(HaveLen(1))
			Expect(result).To(HaveKeyWithValue("addr", "127.0.0.1:6379"))
		})

		It("should get the data path", func() {
			result, err := rdb.ConfigGet(ctx, "data_path").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(result).To(HaveLen(1))
			Expect(result).To(HaveKeyWithValue("data_path", "./nimbis_data"))
		})

		It("should return error for non-existent field", func() {
			_, err := rdb.ConfigGet(ctx, "non_existent_field").Result()
			Expect(err).To(HaveOccurred())
			Expect(err.Error()).To(ContainSubstring("Field 'non_existent_field' not found"))
		})

		It("should get all fields with * wildcard", func() {
			result, err := rdb.ConfigGet(ctx, "*").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(result).To(HaveLen(6)) // addr, data_path, save, appendonly, log_level, worker_threads
			Expect(result).To(HaveKeyWithValue("addr", "127.0.0.1:6379"))
			Expect(result).To(HaveKeyWithValue("data_path", "./nimbis_data"))
			Expect(result).To(HaveKeyWithValue("save", ""))
			Expect(result).To(HaveKeyWithValue("appendonly", "no"))
			Expect(result).To(HaveKeyWithValue("log_level", "info"))
			Expect(result).To(HaveKeyWithValue("worker_threads", strconv.Itoa(runtime.NumCPU())))
		})

		It("should match fields with prefix wildcard", func() {
			result, err := rdb.ConfigGet(ctx, "addr*").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(result).To(HaveLen(1))
			Expect(result).To(HaveKeyWithValue("addr", "127.0.0.1:6379"))
		})

		It("should get the save config", func() {
			result, err := rdb.ConfigGet(ctx, "save").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(result).To(HaveLen(1))
			Expect(result).To(HaveKeyWithValue("save", ""))
		})

		It("should get the appendonly config", func() {
			result, err := rdb.ConfigGet(ctx, "appendonly").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(result).To(HaveLen(1))
			Expect(result).To(HaveKeyWithValue("appendonly", "no"))
		})

		It("should match fields with suffix wildcard", func() {
			result, err := rdb.ConfigGet(ctx, "*path").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(result).To(HaveLen(1))
			Expect(result).To(HaveKeyWithValue("data_path", "./nimbis_data"))
		})

		It("should return empty array for non-matching wildcard", func() {
			result, err := rdb.ConfigGet(ctx, "nonexistent*").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(result).To(BeEmpty())
		})
	})

	Describe("CONFIG SET", func() {
		It("should fail to set immutable field 'addr'", func() {
			err := rdb.ConfigSet(ctx, "addr", "localhost:6380").Err()
			Expect(err).To(HaveOccurred())
			Expect(err.Error()).To(ContainSubstring("Field 'addr' is immutable"))

			// Verify the value hasn't changed
			result, err := rdb.ConfigGet(ctx, "addr").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(result["addr"]).To(Equal("127.0.0.1:6379"))
		})

		It("should fail to set immutable field 'data_path'", func() {
			err := rdb.ConfigSet(ctx, "data_path", "/tmp/nimbis").Err()
			Expect(err).To(HaveOccurred())
			Expect(err.Error()).To(ContainSubstring("Field 'data_path' is immutable"))

			// Verify the value hasn't changed
			result, err := rdb.ConfigGet(ctx, "data_path").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(result["data_path"]).To(Equal("./nimbis_data"))
		})

		It("should fail to set non-existent field", func() {
			err := rdb.ConfigSet(ctx, "unknown_field", "value").Err()
			Expect(err).To(HaveOccurred())
			Expect(err.Error()).To(ContainSubstring("Field 'unknown_field' not found"))
		})
	})
})
