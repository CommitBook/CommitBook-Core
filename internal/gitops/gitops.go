package gitops

import (
	"bytes"
	"context"
	"fmt"
	"os/exec"
	"strings"
	"time"
)

// ChangesSummary describes what changed in the working tree.
type ChangesSummary struct {
	New      []string
	Modified []string
	Deleted  []string
}

// Total returns the total number of changed files.
func (c ChangesSummary) Total() int {
	return len(c.New) + len(c.Modified) + len(c.Deleted)
}

// IsClean returns true if there are no changes.
func (c ChangesSummary) IsClean() bool {
	return c.Total() == 0
}

// String returns a human-readable summary.
func (c ChangesSummary) String() string {
	parts := []string{}
	if len(c.New) > 0 {
		parts = append(parts, fmt.Sprintf("%d new", len(c.New)))
	}
	if len(c.Modified) > 0 {
		parts = append(parts, fmt.Sprintf("%d modified", len(c.Modified)))
	}
	if len(c.Deleted) > 0 {
		parts = append(parts, fmt.Sprintf("%d deleted", len(c.Deleted)))
	}
	if len(parts) == 0 {
		return "no changes"
	}
	return strings.Join(parts, ", ")
}

// IsRepo checks if the given path is inside a git repository.
func IsRepo(path string) bool {
	cmd := exec.Command("git", "-C", path, "rev-parse", "--git-dir")
	return cmd.Run() == nil
}

// HasRemote checks if the repository has any remotes configured.
func HasRemote(path string) bool {
	out, err := gitOutput(path, "remote")
	if err != nil {
		return false
	}
	return strings.TrimSpace(out) != ""
}

// RemoteURL returns the URL of the origin remote.
func RemoteURL(path string) (string, error) {
	out, err := gitOutput(path, "remote", "get-url", "origin")
	if err != nil {
		return "", fmt.Errorf("no origin remote: %w", err)
	}
	return strings.TrimSpace(out), nil
}

// CheckRemote verifies that the origin remote is accessible.
func CheckRemote(path string) error {
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	cmd := exec.CommandContext(ctx, "git", "-C", path, "ls-remote", "--exit-code", "origin")
	var stderr bytes.Buffer
	cmd.Stderr = &stderr

	if err := cmd.Run(); err != nil {
		return fmt.Errorf("remote not accessible: %s", strings.TrimSpace(stderr.String()))
	}
	return nil
}

// Status returns a summary of changes in the working tree.
func Status(path string) (ChangesSummary, error) {
	out, err := gitOutput(path, "status", "--porcelain")
	if err != nil {
		return ChangesSummary{}, fmt.Errorf("git status: %w", err)
	}
	return parsePorcelain(out), nil
}

// StageAll stages all changes in the repository.
func StageAll(path string) error {
	_, err := gitOutput(path, "add", "-A")
	if err != nil {
		return fmt.Errorf("git add: %w", err)
	}
	return nil
}

// Commit creates a commit with the given message. Returns the short hash.
func Commit(path string, message string) (string, error) {
	_, err := gitOutput(path, "commit", "-m", message)
	if err != nil {
		return "", fmt.Errorf("git commit: %w", err)
	}

	hash, err := gitOutput(path, "rev-parse", "--short", "HEAD")
	if err != nil {
		return "", fmt.Errorf("git rev-parse: %w", err)
	}
	return strings.TrimSpace(hash), nil
}

// Push pushes to the given remote and branch.
func Push(path, remote, branch string) error {
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	cmd := exec.CommandContext(ctx, "git", "-C", path, "push", remote, branch)
	var stderr bytes.Buffer
	cmd.Stderr = &stderr

	if err := cmd.Run(); err != nil {
		return fmt.Errorf("git push: %s", strings.TrimSpace(stderr.String()))
	}
	return nil
}

// CurrentBranch returns the current branch name.
func CurrentBranch(path string) (string, error) {
	out, err := gitOutput(path, "rev-parse", "--abbrev-ref", "HEAD")
	if err != nil {
		return "", fmt.Errorf("git branch: %w", err)
	}
	return strings.TrimSpace(out), nil
}

// DiffStat returns the diff --stat output for staged and unstaged changes.
func DiffStat(path string) (string, error) {
	out, err := gitOutput(path, "diff", "--stat")
	if err != nil {
		return "", err
	}
	staged, err := gitOutput(path, "diff", "--staged", "--stat")
	if err != nil {
		return out, nil
	}
	if staged != "" {
		out = out + staged
	}
	return strings.TrimSpace(out), nil
}

// DiffSummary returns a short diff summary for AI context.
func DiffSummary(path string) (string, error) {
	out, err := gitOutput(path, "diff", "--stat", "--no-color")
	if err != nil {
		return "", err
	}
	staged, err := gitOutput(path, "diff", "--staged", "--stat", "--no-color")
	if err == nil && staged != "" {
		out = staged + out
	}
	// Truncate to avoid overwhelming AI prompts
	lines := strings.Split(out, "\n")
	if len(lines) > 20 {
		lines = append(lines[:20], fmt.Sprintf("... and %d more files", len(lines)-20))
	}
	return strings.Join(lines, "\n"), nil
}

func gitOutput(path string, args ...string) (string, error) {
	fullArgs := append([]string{"-C", path}, args...)
	cmd := exec.Command("git", fullArgs...)
	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr
	if err := cmd.Run(); err != nil {
		errMsg := strings.TrimSpace(stderr.String())
		if errMsg == "" {
			errMsg = err.Error()
		}
		return "", fmt.Errorf("%s", errMsg)
	}
	return stdout.String(), nil
}

func parsePorcelain(output string) ChangesSummary {
	var summary ChangesSummary
	for _, line := range strings.Split(output, "\n") {
		if len(line) < 4 {
			continue
		}
		status := line[:2]
		file := strings.TrimSpace(line[3:])

		switch {
		case strings.HasPrefix(status, "??"):
			summary.New = append(summary.New, file)
		case strings.Contains(status, "D"):
			summary.Deleted = append(summary.Deleted, file)
		case strings.Contains(status, "A"):
			summary.New = append(summary.New, file)
		default:
			summary.Modified = append(summary.Modified, file)
		}
	}
	return summary
}
