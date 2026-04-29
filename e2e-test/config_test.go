package tests

import (
	"context"
	"strconv"

	"github.com/marsevilspirit/nimbis/e2e-test/util"
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
		It("should get the host and port", func() {
			result, err := rdb.ConfigGet(ctx, "host").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(result).To(HaveLen(1))
			Expect(result).To(HaveKeyWithValue("host", "127.0.0.1"))

			result, err = rdb.ConfigGet(ctx, "port").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(result).To(HaveLen(1))
			Expect(result).To(HaveKeyWithValue("port", "6379"))
		})

		It("should get the object store URL", func() {
			result, err := rdb.ConfigGet(ctx, "object_store_url").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(result).To(HaveLen(1))
			Expect(result).To(HaveKeyWithValue("object_store_url", "file:nimbis_store"))
		})

		It("should get the log output", func() {
			result, err := rdb.ConfigGet(ctx, "log_output").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(result).To(HaveLen(1))
			Expect(result).To(HaveKeyWithValue("log_output", "terminal"))
		})

		It("should get the log rotation", func() {
			result, err := rdb.ConfigGet(ctx, "log_rotation").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(result).To(HaveLen(1))
			Expect(result).To(HaveKeyWithValue("log_rotation", "daily"))
		})

		It("should get the trace enabled flag", func() {
			result, err := rdb.ConfigGet(ctx, "trace_enabled").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(result).To(HaveLen(1))
			Expect(result).To(HaveKeyWithValue("trace_enabled", "false"))
		})

		It("should return error for non-existent field", func() {
			_, err := rdb.ConfigGet(ctx, "non_existent_field").Result()
			Expect(err).To(HaveOccurred())
			Expect(err.Error()).To(ContainSubstring("Field 'non_existent_field' not found"))
		})

		It("should get all fields with * wildcard", func() {
			result, err := rdb.ConfigGet(ctx, "*").Result()
			Expect(err).NotTo(HaveOccurred())
			// host, port, object_store_url, object_store_options, save, appendonly,
			// log_level, log_output, log_rotation, trace_enabled, trace_endpoint,
			// trace_sampling_ratio, trace_protocol, trace_export_timeout_seconds,
			// trace_report_interval_ms, worker_threads
			Expect(result).To(HaveLen(16))
			Expect(result).To(HaveKeyWithValue("host", "127.0.0.1"))
			Expect(result).To(HaveKeyWithValue("port", "6379"))
			Expect(result).To(HaveKeyWithValue("object_store_url", "file:nimbis_store"))
			Expect(result).To(HaveKeyWithValue("object_store_options", "{}"))
			Expect(result).To(HaveKeyWithValue("save", ""))
			Expect(result).To(HaveKeyWithValue("appendonly", "no"))
			Expect(result).To(HaveKeyWithValue("log_level", "info"))
			Expect(result).To(HaveKeyWithValue("log_output", "terminal"))
			Expect(result).To(HaveKeyWithValue("log_rotation", "daily"))
			Expect(result).To(HaveKeyWithValue("trace_enabled", "false"))
			Expect(result).To(HaveKeyWithValue("trace_endpoint", ""))
			Expect(result).To(HaveKeyWithValue("trace_sampling_ratio", "0.0001"))
			Expect(result).To(HaveKeyWithValue("trace_protocol", "grpc"))
			Expect(result).To(HaveKeyWithValue("trace_export_timeout_seconds", "10"))
			Expect(result).To(HaveKeyWithValue("trace_report_interval_ms", "1000"))
			workerThreads, ok := result["worker_threads"]
			Expect(ok).To(BeTrue())
			workerThreadsInt, convErr := strconv.Atoi(workerThreads)
			Expect(convErr).NotTo(HaveOccurred())
			Expect(workerThreadsInt).To(BeNumerically(">", 0))
		})

		It("should match fields with prefix wildcard", func() {
			result, err := rdb.ConfigGet(ctx, "ho*").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(result).To(HaveLen(1))
			Expect(result).To(HaveKeyWithValue("host", "127.0.0.1"))
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
			result, err := rdb.ConfigGet(ctx, "*url").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(result).To(HaveLen(1))
			Expect(result).To(HaveKeyWithValue("object_store_url", "file:nimbis_store"))
		})

		It("should return empty array for non-matching wildcard", func() {
			result, err := rdb.ConfigGet(ctx, "nonexistent*").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(result).To(BeEmpty())
		})
	})

	Describe("CONFIG SET", func() {
		It("should fail to set immutable field 'host'", func() {
			err := rdb.ConfigSet(ctx, "host", "localhost").Err()
			Expect(err).To(HaveOccurred())
			Expect(err.Error()).To(ContainSubstring("Field 'host' is immutable"))

			// Verify the value hasn't changed
			result, err := rdb.ConfigGet(ctx, "host").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(result["host"]).To(Equal("127.0.0.1"))
		})

		It("should fail to set immutable field 'object_store_url'", func() {
			err := rdb.ConfigSet(ctx, "object_store_url", "file:/tmp/nimbis").Err()
			Expect(err).To(HaveOccurred())
			Expect(err.Error()).To(ContainSubstring("Field 'object_store_url' is immutable"))

			// Verify the value hasn't changed
			result, err := rdb.ConfigGet(ctx, "object_store_url").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(result["object_store_url"]).To(Equal("file:nimbis_store"))
		})

		It("should fail to set immutable field 'object_store_options'", func() {
			err := rdb.ConfigSet(ctx, "object_store_options", "{}").Err()
			Expect(err).To(HaveOccurred())
			Expect(err.Error()).To(ContainSubstring("Field 'object_store_options' is immutable"))

			result, err := rdb.ConfigGet(ctx, "object_store_options").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(result["object_store_options"]).To(Equal("{}"))
		})

		It("should fail to set immutable field 'log_output'", func() {
			err := rdb.ConfigSet(ctx, "log_output", "file").Err()
			Expect(err).To(HaveOccurred())
			Expect(err.Error()).To(ContainSubstring("Field 'log_output' is immutable"))

			result, err := rdb.ConfigGet(ctx, "log_output").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(result["log_output"]).To(Equal("terminal"))
		})

		It("should fail to set immutable field 'log_rotation'", func() {
			err := rdb.ConfigSet(ctx, "log_rotation", "hourly").Err()
			Expect(err).To(HaveOccurred())
			Expect(err.Error()).To(ContainSubstring("Field 'log_rotation' is immutable"))

			result, err := rdb.ConfigGet(ctx, "log_rotation").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(result["log_rotation"]).To(Equal("daily"))
		})

		It("should fail to set immutable field 'trace_enabled'", func() {
			err := rdb.ConfigSet(ctx, "trace_enabled", "true").Err()
			Expect(err).To(HaveOccurred())
			Expect(err.Error()).To(ContainSubstring("Field 'trace_enabled' is immutable"))

			result, err := rdb.ConfigGet(ctx, "trace_enabled").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(result["trace_enabled"]).To(Equal("false"))
		})

		It("should fail to set non-existent field", func() {
			err := rdb.ConfigSet(ctx, "unknown_field", "value").Err()
			Expect(err).To(HaveOccurred())
			Expect(err.Error()).To(ContainSubstring("Field 'unknown_field' not found"))
		})

		It("should set log_level with a valid EnvFilter expression", func() {
			filter := "nimbis=debug,storage=debug,resp=info,slatedb=warn,tokio=warn,info"

			err := rdb.ConfigSet(ctx, "log_level", filter).Err()
			Expect(err).NotTo(HaveOccurred())

			result, err := rdb.ConfigGet(ctx, "log_level").Result()
			Expect(err).NotTo(HaveOccurred())
			Expect(result).To(HaveKeyWithValue("log_level", filter))

			// Restore default so this test does not affect others.
			Expect(rdb.ConfigSet(ctx, "log_level", "info").Err()).To(Succeed())
		})

		It("should reject an invalid EnvFilter expression for log_level", func() {
			err := rdb.ConfigSet(ctx, "log_level", "nimbis=verbose").Err()
			Expect(err).To(HaveOccurred())
			Expect(err.Error()).To(ContainSubstring("Invalid log level"))
		})
	})
})
