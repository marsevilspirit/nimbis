package tests

import (
	"bufio"
	"net"
	"strings"
	"time"

	. "github.com/onsi/ginkgo/v2"
	. "github.com/onsi/gomega"
)

func mustReadLine(reader *bufio.Reader) string {
	line, err := reader.ReadString('\n')
	Expect(err).NotTo(HaveOccurred())
	return line
}

func assertHelloResp2(reader *bufio.Reader) {
	Expect(mustReadLine(reader)).To(Equal("*14\r\n"))
	Expect(mustReadLine(reader)).To(Equal("$6\r\n"))
	Expect(mustReadLine(reader)).To(Equal("server\r\n"))
	Expect(mustReadLine(reader)).To(Equal("$6\r\n"))
	Expect(mustReadLine(reader)).To(Equal("nimbis\r\n"))
	Expect(mustReadLine(reader)).To(Equal("$7\r\n"))
	Expect(mustReadLine(reader)).To(Equal("version\r\n"))
	Expect(strings.HasPrefix(mustReadLine(reader), "$")).To(BeTrue())
	Expect(strings.TrimSpace(mustReadLine(reader))).NotTo(BeEmpty())
	Expect(mustReadLine(reader)).To(Equal("$5\r\n"))
	Expect(mustReadLine(reader)).To(Equal("proto\r\n"))
	Expect(mustReadLine(reader)).To(Equal(":2\r\n"))
	Expect(mustReadLine(reader)).To(Equal("$2\r\n"))
	Expect(mustReadLine(reader)).To(Equal("id\r\n"))
	Expect(strings.HasPrefix(mustReadLine(reader), ":")).To(BeTrue())
	Expect(mustReadLine(reader)).To(Equal("$4\r\n"))
	Expect(mustReadLine(reader)).To(Equal("mode\r\n"))
	Expect(mustReadLine(reader)).To(Equal("$10\r\n"))
	Expect(mustReadLine(reader)).To(Equal("standalone\r\n"))
	Expect(mustReadLine(reader)).To(Equal("$4\r\n"))
	Expect(mustReadLine(reader)).To(Equal("role\r\n"))
	Expect(mustReadLine(reader)).To(Equal("$6\r\n"))
	Expect(mustReadLine(reader)).To(Equal("master\r\n"))
	Expect(mustReadLine(reader)).To(Equal("$7\r\n"))
	Expect(mustReadLine(reader)).To(Equal("modules\r\n"))
	Expect(mustReadLine(reader)).To(Equal("*0\r\n"))
}

var _ = Describe("HELLO Command", func() {
	var conn net.Conn
	var reader *bufio.Reader

	BeforeEach(func() {
		var err error
		conn, err = net.Dial("tcp", "localhost:6379")
		Expect(err).NotTo(HaveOccurred())
		Expect(conn.SetDeadline(time.Now().Add(5 * time.Second))).To(Succeed())
		reader = bufio.NewReader(conn)
	})

	AfterEach(func() {
		if conn != nil {
			_ = conn.Close()
		}
	})

	It("should support HELLO default as RESP2 handshake", func() {
		_, err := conn.Write([]byte("HELLO\r\n"))
		Expect(err).NotTo(HaveOccurred())
		assertHelloResp2(reader)
	})

	It("should support HELLO 2", func() {
		_, err := conn.Write([]byte("HELLO 2\r\n"))
		Expect(err).NotTo(HaveOccurred())
		assertHelloResp2(reader)
	})

	It("should support HELLO 3 with RESP3 map response", func() {
		_, err := conn.Write([]byte("HELLO 3\r\n"))
		Expect(err).NotTo(HaveOccurred())
		Expect(mustReadLine(reader)).To(Equal("%7\r\n"))
	})

	It("should reject unsupported HELLO protocol version", func() {
		_, err := conn.Write([]byte("HELLO 4\r\n"))
		Expect(err).NotTo(HaveOccurred())
		line := mustReadLine(reader)
		Expect(line).To(HavePrefix("-NOPROTO"))
		Expect(line).To(ContainSubstring("Use 2 or 3"))
	})
})
