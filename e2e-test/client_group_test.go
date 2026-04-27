package tests

import (
	"context"
	"strconv"
	"strings"

	"github.com/marsevilspirit/nimbis/e2e-test/util"
	. "github.com/onsi/ginkgo/v2"
	. "github.com/onsi/gomega"
	"github.com/redis/go-redis/v9"
)

type clientListEntry struct {
	id   int64
	name string
}

func parseClientList(result interface{}) []clientListEntry {
	raw, ok := result.(string)
	Expect(ok).To(BeTrue(), "CLIENT LIST should return bulk string")

	raw = strings.TrimSpace(raw)
	if raw == "" {
		return nil
	}

	lines := strings.Split(raw, "\n")
	entries := make([]clientListEntry, 0, len(lines))
	for _, line := range lines {
		line = strings.TrimSpace(line)
		if line == "" {
			continue
		}

		parts := strings.SplitN(line, " ", 2)
		Expect(parts).To(HaveLen(2), "unexpected CLIENT LIST line format: %s", line)
		Expect(strings.HasPrefix(parts[0], "id=")).To(BeTrue(), "unexpected CLIENT LIST id part: %s", line)
		Expect(strings.HasPrefix(parts[1], "name=")).To(BeTrue(), "unexpected CLIENT LIST name part: %s", line)

		idStr := strings.TrimPrefix(parts[0], "id=")
		id, err := strconv.ParseInt(idStr, 10, 64)
		Expect(err).NotTo(HaveOccurred(), "invalid client id in line: %s", line)

		entries = append(entries, clientListEntry{
			id:   id,
			name: strings.TrimPrefix(parts[1], "name="),
		})
	}

	return entries
}

func mustClientID(ctx context.Context, rdb *redis.Client) int64 {
	result, err := rdb.Do(ctx, "CLIENT", "ID").Result()
	Expect(err).NotTo(HaveOccurred())
	id, ok := result.(int64)
	Expect(ok).To(BeTrue(), "CLIENT ID should return int64")
	Expect(id).To(BeNumerically(">", 0))
	return id
}

func findClient(entries []clientListEntry, id int64) (clientListEntry, bool) {
	for _, entry := range entries {
		if entry.id == id {
			return entry, true
		}
	}
	return clientListEntry{}, false
}

var _ = Describe("CLIENT Group Commands", func() {
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

	It("should return stable positive client id for the same connection", func() {
		id1 := mustClientID(ctx, rdb)
		id2 := mustClientID(ctx, rdb)
		Expect(id1).To(Equal(id2))
	})

	It("should keep names isolated per client", func() {
		other := util.NewClient()
		defer func() { Expect(other.Close()).To(Succeed()) }()
		Expect(other.Ping(ctx).Err()).To(Succeed())

		_, err := rdb.Do(ctx, "CLIENT", "SETNAME", "alpha").Result()
		Expect(err).NotTo(HaveOccurred())
		_, err = other.Do(ctx, "CLIENT", "SETNAME", "beta").Result()
		Expect(err).NotTo(HaveOccurred())

		name1, err := rdb.Do(ctx, "CLIENT", "GETNAME").Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(name1).To(Equal("alpha"))

		name2, err := other.Do(ctx, "CLIENT", "GETNAME").Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(name2).To(Equal("beta"))
	})

	It("should support case-insensitive subcommands", func() {
		_, err := rdb.Do(ctx, "CLIENT", "setname", "lower").Result()
		Expect(err).NotTo(HaveOccurred())

		name, err := rdb.Do(ctx, "CLIENT", "getname").Result()
		Expect(err).NotTo(HaveOccurred())
		Expect(name).To(Equal("lower"))
	})

	It("should return nil for GETNAME before setting a name", func() {
		result, err := rdb.Do(ctx, "CLIENT", "GETNAME").Result()
		Expect(err).To(Equal(redis.Nil))
		Expect(result).To(BeNil())
	})

	It("should list clients with ids and names", func() {
		other := util.NewClient()
		defer func() { Expect(other.Close()).To(Succeed()) }()
		Expect(other.Ping(ctx).Err()).To(Succeed())

		id1 := mustClientID(ctx, rdb)
		id2 := mustClientID(ctx, other)

		_, err := rdb.Do(ctx, "CLIENT", "SETNAME", "alice").Result()
		Expect(err).NotTo(HaveOccurred())
		_, err = other.Do(ctx, "CLIENT", "SETNAME", "bob").Result()
		Expect(err).NotTo(HaveOccurred())

		result, err := rdb.Do(ctx, "CLIENT", "LIST").Result()
		Expect(err).NotTo(HaveOccurred())

		entries := parseClientList(result)
		Expect(entries).NotTo(BeEmpty())

		entry1, ok := findClient(entries, id1)
		Expect(ok).To(BeTrue())
		Expect(entry1.name).To(Equal("alice"))

		entry2, ok := findClient(entries, id2)
		Expect(ok).To(BeTrue())
		Expect(entry2.name).To(Equal("bob"))

		for i := 1; i < len(entries); i++ {
			Expect(entries[i-1].id).To(BeNumerically("<=", entries[i].id))
		}
	})

	It("should reject unknown subcommand", func() {
		_, err := rdb.Do(ctx, "CLIENT", "BOGUS").Result()
		Expect(err).To(HaveOccurred())
		Expect(err.Error()).To(ContainSubstring("ERR unknown CLIENT subcommand 'BOGUS'"))
	})

	It("should reject wrong number of arguments", func() {
		_, err := rdb.Do(ctx, "CLIENT").Result()
		Expect(err).To(HaveOccurred())
		Expect(err.Error()).To(ContainSubstring("ERR wrong number of arguments for 'client' command"))

		_, err = rdb.Do(ctx, "CLIENT", "SETNAME").Result()
		Expect(err).To(HaveOccurred())
		Expect(err.Error()).To(ContainSubstring("ERR wrong number of arguments for 'setname' command"))

		_, err = rdb.Do(ctx, "CLIENT", "GETNAME", "extra").Result()
		Expect(err).To(HaveOccurred())
		Expect(err.Error()).To(ContainSubstring("ERR wrong number of arguments for 'getname' command"))

		_, err = rdb.Do(ctx, "CLIENT", "ID", "extra").Result()
		Expect(err).To(HaveOccurred())
		Expect(err.Error()).To(ContainSubstring("ERR wrong number of arguments for 'id' command"))

		_, err = rdb.Do(ctx, "CLIENT", "LIST", "extra").Result()
		Expect(err).To(HaveOccurred())
		Expect(err.Error()).To(ContainSubstring("ERR wrong number of arguments for 'list' command"))
	})
})
