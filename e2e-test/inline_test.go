package tests

import (
	"bufio"
	"net"
	"time"

	. "github.com/onsi/ginkgo/v2"
	. "github.com/onsi/gomega"
)

var _ = Describe("Inline Command Parsing", func() {
	var conn net.Conn
	var reader *bufio.Reader

	BeforeEach(func() {
		// Ensure server is running (suite_test.go usually handles this, but we need raw connection)
		// We assume util.StartServer() is called in Suite setup.

		var err error
		conn, err = net.Dial("tcp", "localhost:6379")
		Expect(err).NotTo(HaveOccurred())
		conn.SetDeadline(time.Now().Add(5 * time.Second))
		reader = bufio.NewReader(conn)
	})

	AfterEach(func() {
		if conn != nil {
			conn.Close()
		}
	})

	It("should handle valid inline PING", func() {
		_, err := conn.Write([]byte("PING\r\n"))
		Expect(err).NotTo(HaveOccurred())

		line, err := reader.ReadString('\n')
		Expect(err).NotTo(HaveOccurred())
		// PING returns simple string PONG: "+PONG\r\n"
		Expect(line).To(Equal("+PONG\r\n"))
	})

	It("should handle valid inline SET and GET", func() {
		_, err := conn.Write([]byte("SET inline_key inline_val\r\n"))
		Expect(err).NotTo(HaveOccurred())

		line, err := reader.ReadString('\n')
		Expect(err).NotTo(HaveOccurred())
		Expect(line).To(Equal("+OK\r\n"))

		_, err = conn.Write([]byte("GET inline_key\r\n"))
		Expect(err).NotTo(HaveOccurred())

		// GET returns bulk string: "$10\r\ninline_val\r\n"
		line, err = reader.ReadString('\n')
		Expect(err).NotTo(HaveOccurred())
		Expect(line).To(Equal("$10\r\n"))

		line, err = reader.ReadString('\n')
		Expect(err).NotTo(HaveOccurred())
		Expect(line).To(Equal("inline_val\r\n"))
	})

	It("should skip empty lines", func() {
		// Send empty lines then PING
		_, err := conn.Write([]byte("\r\n\r\n \r\nPING\r\n"))
		Expect(err).NotTo(HaveOccurred())

		line, err := reader.ReadString('\n')
		Expect(err).NotTo(HaveOccurred())
		Expect(line).To(Equal("+PONG\r\n"))
	})

	It("should return error for invalid start character", func() {
		// Send control character start
		_, err := conn.Write([]byte("\x01PING\r\n"))
		Expect(err).NotTo(HaveOccurred())

		line, err := reader.ReadString('\n')
		Expect(err).NotTo(HaveOccurred())
		// Check that it's an error response
		Expect(line).To(HavePrefix("-ERR"))
		Expect(line).To(ContainSubstring("Invalid type marker"))
	})

	It("should handle leading whitespace", func() {
		_, err := conn.Write([]byte("   PING\r\n"))
		Expect(err).NotTo(HaveOccurred())

		line, err := reader.ReadString('\n')
		Expect(err).NotTo(HaveOccurred())
		Expect(line).To(Equal("+PONG\r\n"))
	})
})
