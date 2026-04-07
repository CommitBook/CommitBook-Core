package config

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"time"
)

const (
	GlobalConfigDir  = ".commitbook"
	GlobalConfigFile = "config.json"
	RepoConfigDir    = ".CommitBook"
	RepoConfigFile   = "config.json"
	RepoLogsDir      = "Logs"
	LockFile         = ".lock"
)

// GlobalConfig is stored at ~/.commitbook/config.json
type GlobalConfig struct {
	Version string               `json:"version"`
	Repos   map[string]RepoEntry `json:"repos"`
	AI      AIConfig             `json:"ai"`
}

type RepoEntry struct {
	Enabled  bool   `json:"enabled"`
	Schedule string `json:"schedule"`
}

type AIConfig struct {
	Providers []string `json:"providers"`
}

// RepoConfig is stored at <repo>/.CommitBook/config.json
type RepoConfig struct {
	Enabled     bool            `json:"enabled"`
	Schedule    string          `json:"schedule"`
	LastCommit  string          `json:"last_commit,omitempty"`
	CreatedAt   string          `json:"created_at"`
	Git         GitSettings     `json:"git"`
	Logging     LoggingSettings `json:"logging"`
	SchedulerID string          `json:"scheduler_id,omitempty"`
}

type GitSettings struct {
	AutoPush bool   `json:"auto_push"`
	Branch   string `json:"branch"`
}

type LoggingSettings struct {
	Level      string `json:"level"`
	MaxLogDays int    `json:"max_log_days"`
}

// DefaultGlobalConfig returns a new GlobalConfig with sensible defaults.
func DefaultGlobalConfig() GlobalConfig {
	return GlobalConfig{
		Version: "1.0.0",
		Repos:   make(map[string]RepoEntry),
		AI: AIConfig{
			Providers: []string{"gh-copilot", "claude-cli", "codex-cli", "fallback"},
		},
	}
}

// DefaultRepoConfig returns a new RepoConfig with sensible defaults.
func DefaultRepoConfig(branch string) RepoConfig {
	return RepoConfig{
		Enabled:  true,
		Schedule: "0 * * * *",
		CreatedAt: time.Now().UTC().Format(time.RFC3339),
		Git: GitSettings{
			AutoPush: true,
			Branch:   branch,
		},
		Logging: LoggingSettings{
			Level:      "info",
			MaxLogDays: 30,
		},
	}
}

// GlobalConfigPath returns the full path to the global config file.
func GlobalConfigPath() (string, error) {
	home, err := os.UserHomeDir()
	if err != nil {
		return "", fmt.Errorf("cannot determine home directory: %w", err)
	}
	return filepath.Join(home, GlobalConfigDir, GlobalConfigFile), nil
}

// RepoConfigPath returns the full path to the repo config file.
func RepoConfigPath(repoPath string) string {
	return filepath.Join(repoPath, RepoConfigDir, RepoConfigFile)
}

// RepoLogsPath returns the full path to the repo logs directory.
func RepoLogsPath(repoPath string) string {
	return filepath.Join(repoPath, RepoConfigDir, RepoLogsDir)
}

// RepoLockPath returns the full path to the repo lock file.
func RepoLockPath(repoPath string) string {
	return filepath.Join(repoPath, RepoConfigDir, LockFile)
}

// RepoCommitBookDir returns the full path to the .CommitBook directory.
func RepoCommitBookDir(repoPath string) string {
	return filepath.Join(repoPath, RepoConfigDir)
}

// LoadGlobalConfig loads the global config from disk. If the file does not exist,
// it returns a default config without error.
func LoadGlobalConfig() (GlobalConfig, error) {
	path, err := GlobalConfigPath()
	if err != nil {
		return GlobalConfig{}, err
	}

	data, err := os.ReadFile(path)
	if err != nil {
		if os.IsNotExist(err) {
			return DefaultGlobalConfig(), nil
		}
		return GlobalConfig{}, fmt.Errorf("reading global config: %w", err)
	}

	var cfg GlobalConfig
	if err := json.Unmarshal(data, &cfg); err != nil {
		return GlobalConfig{}, fmt.Errorf("parsing global config: %w", err)
	}
	if cfg.Repos == nil {
		cfg.Repos = make(map[string]RepoEntry)
	}
	return cfg, nil
}

// SaveGlobalConfig writes the global config to disk, creating directories as needed.
func SaveGlobalConfig(cfg GlobalConfig) error {
	path, err := GlobalConfigPath()
	if err != nil {
		return err
	}

	if err := os.MkdirAll(filepath.Dir(path), 0755); err != nil {
		return fmt.Errorf("creating global config directory: %w", err)
	}

	data, err := json.MarshalIndent(cfg, "", "  ")
	if err != nil {
		return fmt.Errorf("marshaling global config: %w", err)
	}
	data = append(data, '\n')

	return os.WriteFile(path, data, 0644)
}

// LoadRepoConfig loads the repo config from disk.
func LoadRepoConfig(repoPath string) (RepoConfig, error) {
	path := RepoConfigPath(repoPath)

	data, err := os.ReadFile(path)
	if err != nil {
		if os.IsNotExist(err) {
			return RepoConfig{}, fmt.Errorf("repo not set up: %s does not exist (run 'commitbook setup')", path)
		}
		return RepoConfig{}, fmt.Errorf("reading repo config: %w", err)
	}

	var cfg RepoConfig
	if err := json.Unmarshal(data, &cfg); err != nil {
		return RepoConfig{}, fmt.Errorf("parsing repo config: %w", err)
	}
	return cfg, nil
}

// SaveRepoConfig writes the repo config to disk.
func SaveRepoConfig(repoPath string, cfg RepoConfig) error {
	path := RepoConfigPath(repoPath)

	if err := os.MkdirAll(filepath.Dir(path), 0755); err != nil {
		return fmt.Errorf("creating repo config directory: %w", err)
	}

	data, err := json.MarshalIndent(cfg, "", "  ")
	if err != nil {
		return fmt.Errorf("marshaling repo config: %w", err)
	}
	data = append(data, '\n')

	return os.WriteFile(path, data, 0644)
}

// IsRepoSetUp returns true if the repo has a .CommitBook/config.json file.
func IsRepoSetUp(repoPath string) bool {
	_, err := os.Stat(RepoConfigPath(repoPath))
	return err == nil
}

// ResolveRepoPath resolves the repo path from the flag or uses the current directory.
func ResolveRepoPath(flagValue string) (string, error) {
	if flagValue != "" {
		return filepath.Abs(flagValue)
	}
	return os.Getwd()
}
