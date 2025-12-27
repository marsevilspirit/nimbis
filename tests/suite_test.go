package tests

import (
	"fmt"
	"testing"

	"github.com/marsevilspirit/nimbis/tests/util"
	. "github.com/onsi/ginkgo/v2"
	. "github.com/onsi/gomega"
)

func TestNimbis(t *testing.T) {
	RegisterFailHandler(Fail)
	RunSpecs(t, "Nimbis Suite")
}

var _ = BeforeSuite(func() {
	// Start server on port 6379
	err := util.StartServer()
	Expect(err).NotTo(HaveOccurred())
	fmt.Println("Server started on port 6379")
})

var _ = AfterSuite(func() {
	util.StopServer()
	fmt.Println("Server stopped")
})
