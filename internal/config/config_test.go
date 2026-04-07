package config

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
)

func TestDefaultGlobalConfig(t *testing.T) {
	cfg := DefaultGlobalConfig()
	if cfg.Version != "1.0.0" {
		t.Errorf("expected version 1.0.0, got %s", cfg.Version)
	}
	if len(cfg.Repos) != 0 {
		t.Errorf("expected empty repos map, got %d entries", len(cfg.Repos))
	}
	if len(cfg.AI.Providers) != 4 {
		t.Errorf("expected 4 AI providers, got %d", len(cfg.AI.Providers))
	}
}

func TestDefaultRepoConfig(t *testing.T) {
	cfg := DefaultRepoConfig("main")
	if !cfg.Enabled {
		t.Error("expected enabled to be true")
	}
	if cfg.Schedule != "0 * * * *" {
		t.Errorf("expected hourly schedule, got %s", cfg.Schedule)
	}
	if !cfg.Git.AutoPush {
		t.Error("expected auto_push to be true")
	}
	if cfg.Git.Branch != "main" {
		t.Errorf("expected branch main, got %s", cfg.Git.Branch)
	}
	if cfg.Logging.MaxLogDays != 30 {
		t.Errorf("expected max_log_days 30, got %d", cfg.Logging.MaxLogDays)
	}
}

func TestSaveAndLoadRepoConfig(t *testing.T) {
	tmpDir := t.TempDir()
	cfg := DefaultRepoConfig("main")

	if err := SaveRepoConfig(tmpDir, cfg); err != nil {
		t.Fatalf("SaveRepoConfig: %v", err)
	}

	loaded, err := LoadRepoConfig(tmpDir)
	if err != nil {
		t.Fatalf("LoadRepoConfig: %v", err)
	}

	if loaded.Schedule != cfg.Schedule {
		t.Errorf("schedule mismatch: got %s, want %s", loaded.Schedule, cfg.Schedule)
	}
	if loaded.Git.Branch != cfg.Git.Branch {
		t.Errorf("branch mismatch: got %s, want %s", loaded.Git.Branch, cfg.Git.Branch)
	}
}

func TestLoadRepoConfigNotSetUp(t *testing.T) {
	tmpDir := t.TempDir()
	_, err := LoadRepoConfig(tmpDir)
	if err == nil {
		t.Error("expected error for non-existent config")
	}
}

func TestIsRepoSetUp(t *testing.T) {
	tmpDir := t.TempDir()
	if IsRepoSetUp(tmpDir) {
		t.Error("expected false for empty directory")
	}

	cfg := DefaultRepoConfig("main")
	if err := SaveRepoConfig(tmpDir, cfg); err != nil {
		t.Fatalf("SaveRepoConfig: %v", err)
	}

	if !IsRepoSetUp(tmpDir) {
		t.Error("expected true after saving config")
	}
}

func TestRepoConfigJSON(t *testing.T) {
	cfg := DefaultRepoConfig("develop")
	data, err := json.MarshalIndent(cfg, "", "  ")
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}

	var parsed RepoConfig
	if err := json.Unmarshal(data, &parsed); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}

	if parsed.Git.Branch != "develop" {
		t.Errorf("expected branch develop, got %s", parsed.Git.Branch)
	}
}

func TestRepoConfigPath(t *testing.T) {
	path := RepoConfigPath("/tmp/myrepo")
	expected := filepath.Join("/tmp/myrepo", ".CommitBook", "config.json")
	if path != expected {
		t.Errorf("expected %s, got %s", expected, path)
	}
}

func TestResolveRepoPath(t *testing.T) {
	// With explicit flag
	path, err := ResolveRepoPath("/tmp/myrepo")
	if err != nil {
		t.Fatalf("ResolveRepoPath: %v", err)
	}
	if path != "/tmp/myrepo" {
		t.Errorf("expected /tmp/myrepo, got %s", path)
	}

	// Without flag (uses cwd)
	cwd, _ := os.Getwd()
	path, err = ResolveRepoPath("")
	if err != nil {
		t.Fatalf("ResolveRepoPath: %v", err)
	}
	if path != cwd {
		t.Errorf("expected %s, got %s", cwd, path)
	}
}
