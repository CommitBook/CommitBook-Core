package logger

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"
)

// FileLogger writes log entries to daily log files in .CommitBook/Logs/.
type FileLogger struct {
	logsDir    string
	maxLogDays int
}

// New creates a FileLogger, creating the logs directory if needed.
func New(repoPath string, maxLogDays int) (*FileLogger, error) {
	logsDir := filepath.Join(repoPath, ".CommitBook", "Logs")
	if err := os.MkdirAll(logsDir, 0755); err != nil {
		return nil, fmt.Errorf("creating logs directory: %w", err)
	}
	return &FileLogger{
		logsDir:    logsDir,
		maxLogDays: maxLogDays,
	}, nil
}

// Info logs an informational message.
func (l *FileLogger) Info(msg string) error {
	return l.log("INFO", msg)
}

// Warn logs a warning message.
func (l *FileLogger) Warn(msg string) error {
	return l.log("WARN", msg)
}

// Error logs an error message.
func (l *FileLogger) Error(msg string) error {
	return l.log("ERROR", msg)
}

// Debug logs a debug message.
func (l *FileLogger) Debug(msg string) error {
	return l.log("DEBUG", msg)
}

// Cleanup removes log files older than maxLogDays.
func (l *FileLogger) Cleanup() error {
	if l.maxLogDays <= 0 {
		return nil
	}

	cutoff := time.Now().AddDate(0, 0, -l.maxLogDays)
	entries, err := os.ReadDir(l.logsDir)
	if err != nil {
		return fmt.Errorf("reading logs directory: %w", err)
	}

	for _, entry := range entries {
		if entry.IsDir() || !strings.HasSuffix(entry.Name(), ".log") {
			continue
		}

		// Parse date from filename (YYYY-MM-DD.log)
		name := strings.TrimSuffix(entry.Name(), ".log")
		logDate, err := time.Parse("2006-01-02", name)
		if err != nil {
			continue // skip files that don't match the expected format
		}

		if logDate.Before(cutoff) {
			path := filepath.Join(l.logsDir, entry.Name())
			if err := os.Remove(path); err != nil {
				return fmt.Errorf("removing old log %s: %w", entry.Name(), err)
			}
		}
	}
	return nil
}

func (l *FileLogger) log(level, msg string) error {
	filename := time.Now().Format("2006-01-02") + ".log"
	path := filepath.Join(l.logsDir, filename)

	f, err := os.OpenFile(path, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0644)
	if err != nil {
		return fmt.Errorf("opening log file: %w", err)
	}
	defer f.Close()

	timestamp := time.Now().Format("2006-01-02 15:04:05")
	entry := fmt.Sprintf("[%s] [%s] %s\n", timestamp, level, msg)
	_, err = f.WriteString(entry)
	return err
}
