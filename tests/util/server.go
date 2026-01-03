package util

import (
	"context"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"time"

	"github.com/redis/go-redis/v9"
)

var serverCmd *exec.Cmd

// findProjectRoot searches upward from the current directory
// to find the project root (identified by Cargo.toml)
func findProjectRoot() (string, error) {
	dir, err := os.Getwd()
	if err != nil {
		return "", err
	}

	for {
		// Check if Cargo.toml exists (Rust project marker)
		cargoToml := filepath.Join(dir, "Cargo.toml")
		if _, err := os.Stat(cargoToml); err == nil {
			return dir, nil
		}

		// Check if we've reached the filesystem root
		parent := filepath.Dir(dir)
		if parent == dir {
			return "", fmt.Errorf("project root not found (no Cargo.toml in parent directories)")
		}
		dir = parent
	}
}

// findBinary locates the nimbis binary, first checking the NIMBIS_BIN
// environment variable, then falling back to target/debug/nimbis
func findBinary() (string, error) {
	// 1. Check environment variable first
	if binPath := os.Getenv("NIMBIS_BIN"); binPath != "" {
		if _, err := os.Stat(binPath); err == nil {
			return binPath, nil
		}
		return "", fmt.Errorf("NIMBIS_BIN is set to %s but file not found", binPath)
	}

	// 2. Find project root and construct binary path
	projectRoot, err := findProjectRoot()
	if err != nil {
		return "", fmt.Errorf("failed to find project root: %w", err)
	}

	binName := "nimbis"
	if runtime.GOOS == "windows" {
		binName = "nimbis.exe"
	}

	binPath := filepath.Join(projectRoot, "target", "debug", binName)
	if _, err := os.Stat(binPath); os.IsNotExist(err) {
		return "", fmt.Errorf("binary not found at %s (hint: run 'just build' or 'cargo build')", binPath)
	}

	return binPath, nil
}

// StartServer starts the nimbis server on the specified port.
// It assumes the binary is located at ../../target/debug/nimbis
func StartServer() error {
	// Find the binary using environment variable or project root detection
	binPath, err := findBinary()
	if err != nil {
		return err
	}

	// Get project root for setting working directory
	projectRoot, err := findProjectRoot()
	if err != nil {
		return fmt.Errorf("failed to find project root: %w", err)
	}

	// Clean up nimbis_data
	dataPath := filepath.Join(projectRoot, "nimbis_data")
	_ = os.RemoveAll(dataPath)

	serverCmd = exec.Command(binPath)
	serverCmd.Dir = projectRoot // Run from project root to find nimbis_data
	// Redirect stdout/stderr for debugging
	serverCmd.Stdout = os.Stdout
	serverCmd.Stderr = os.Stderr

	if err := serverCmd.Start(); err != nil {
		return fmt.Errorf("failed to start server: %w", err)
	}

	// Wait for server to be ready
	addr := "localhost:6379"
	client := redis.NewClient(&redis.Options{
		Addr: addr,
	})
	defer client.Close()

	ctx := context.Background()
	for i := 0; i < 20; i++ {
		err := client.Ping(ctx).Err()
		if err == nil {
			return nil // Server is ready
		}
		fmt.Printf("Tick %d: Ping failed: %v\n", i, err)
		time.Sleep(100 * time.Millisecond)
	}

	_ = serverCmd.Process.Kill()
	serverCmd = nil
	return fmt.Errorf("server failed to start on %s", addr)
}

// StopServer kills the server process.
func StopServer() {
	if serverCmd != nil && serverCmd.Process != nil {
		_ = serverCmd.Process.Kill()
		_ = serverCmd.Wait()
		serverCmd = nil
	}
}

// NewClient creates a new Redis client connected to the local server.
func NewClient() *redis.Client {
	return redis.NewClient(&redis.Options{
		Addr: "localhost:6379",
	})
}
