package logger

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestNewCreatesDirectory(t *testing.T) {
	tmpDir := t.TempDir()
	_, err := New(tmpDir, 30)
	if err != nil {
		t.Fatalf("New: %v", err)
	}

	logsDir := filepath.Join(tmpDir, ".CommitBook", "Logs")
	info, err := os.Stat(logsDir)
	if err != nil {
		t.Fatalf("logs directory not created: %v", err)
	}
	if !info.IsDir() {
		t.Error("expected logs path to be a directory")
	}
}

func TestLogWritesToFile(t *testing.T) {
	tmpDir := t.TempDir()
	l, err := New(tmpDir, 30)
	if err != nil {
		t.Fatal(err)
	}

	if err := l.Info("test message"); err != nil {
		t.Fatalf("Info: %v", err)
	}

	// Check today's log file exists
	filename := time.Now().Format("2006-01-02") + ".log"
	path := filepath.Join(l.logsDir, filename)
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("reading log file: %v", err)
	}

	content := string(data)
	if !strings.Contains(content, "[INFO]") {
		t.Error("expected [INFO] in log entry")
	}
	if !strings.Contains(content, "test message") {
		t.Error("expected message in log entry")
	}
}

func TestLogLevels(t *testing.T) {
	tmpDir := t.TempDir()
	l, err := New(tmpDir, 30)
	if err != nil {
		t.Fatal(err)
	}

	l.Info("info msg")
	l.Warn("warn msg")
	l.Error("error msg")
	l.Debug("debug msg")

	filename := time.Now().Format("2006-01-02") + ".log"
	path := filepath.Join(l.logsDir, filename)
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}

	content := string(data)
	for _, level := range []string{"INFO", "WARN", "ERROR", "DEBUG"} {
		if !strings.Contains(content, "["+level+"]") {
			t.Errorf("expected [%s] in log output", level)
		}
	}
}

func TestCleanupRemovesOldLogs(t *testing.T) {
	tmpDir := t.TempDir()
	l, err := New(tmpDir, 7)
	if err != nil {
		t.Fatal(err)
	}

	// Create an old log file (10 days ago)
	oldDate := time.Now().AddDate(0, 0, -10).Format("2006-01-02")
	oldPath := filepath.Join(l.logsDir, oldDate+".log")
	os.WriteFile(oldPath, []byte("old log"), 0644)

	// Create a recent log file (2 days ago)
	recentDate := time.Now().AddDate(0, 0, -2).Format("2006-01-02")
	recentPath := filepath.Join(l.logsDir, recentDate+".log")
	os.WriteFile(recentPath, []byte("recent log"), 0644)

	if err := l.Cleanup(); err != nil {
		t.Fatalf("Cleanup: %v", err)
	}

	// Old log should be removed
	if _, err := os.Stat(oldPath); !os.IsNotExist(err) {
		t.Error("expected old log to be removed")
	}

	// Recent log should remain
	if _, err := os.Stat(recentPath); err != nil {
		t.Error("expected recent log to remain")
	}
}

func TestCleanupIgnoresNonLogFiles(t *testing.T) {
	tmpDir := t.TempDir()
	l, err := New(tmpDir, 1)
	if err != nil {
		t.Fatal(err)
	}

	// Create a non-log file
	otherPath := filepath.Join(l.logsDir, "notes.txt")
	os.WriteFile(otherPath, []byte("keep me"), 0644)

	if err := l.Cleanup(); err != nil {
		t.Fatal(err)
	}

	if _, err := os.Stat(otherPath); err != nil {
		t.Error("expected non-log file to remain")
	}
}
