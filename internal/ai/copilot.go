package ai

import (
	"bytes"
	"context"
	"fmt"
	"os/exec"
	"strings"
	"time"

	"github.com/ZAAI-com/CommitBook/internal/gitops"
)

// CopilotProvider generates commit messages using GitHub Copilot CLI.
type CopilotProvider struct{}

func (p *CopilotProvider) Name() string {
	return "GitHub Copilot"
}

func (p *CopilotProvider) IsAvailable() bool {
	// Check if gh CLI is installed
	if _, err := exec.LookPath("gh"); err != nil {
		return false
	}
	// Check if copilot extension is installed
	cmd := exec.Command("gh", "copilot", "--version")
	return cmd.Run() == nil
}

func (p *CopilotProvider) Generate(summary gitops.ChangesSummary, repoPath string) (string, error) {
	diffSummary, _ := gitops.DiffSummary(repoPath)

	prompt := fmt.Sprintf(
		"Write a concise one-line git commit message (max 72 chars) for these changes: %s. Diff stats:\n%s",
		summary.String(),
		truncate(diffSummary, 500),
	)

	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()

	cmd := exec.CommandContext(ctx, "gh", "copilot", "suggest", "-t", "git:commit", prompt)
	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr

	if err := cmd.Run(); err != nil {
		return "", fmt.Errorf("gh copilot: %w", err)
	}

	msg := extractMessage(stdout.String())
	if msg == "" {
		return "", fmt.Errorf("empty response from copilot")
	}
	return msg, nil
}

func extractMessage(output string) string {
	// gh copilot suggest output can be multi-line; extract the actual suggestion
	lines := strings.Split(strings.TrimSpace(output), "\n")
	for _, line := range lines {
		line = strings.TrimSpace(line)
		if line == "" || strings.HasPrefix(line, "#") || strings.HasPrefix(line, "?") {
			continue
		}
		// Strip any leading "git commit -m " prefix
		line = strings.TrimPrefix(line, "git commit -m ")
		line = strings.Trim(line, "\"'")
		if line != "" {
			return line
		}
	}
	if len(lines) > 0 {
		return strings.TrimSpace(lines[len(lines)-1])
	}
	return ""
}

func truncate(s string, maxLen int) string {
	if len(s) <= maxLen {
		return s
	}
	return s[:maxLen] + "..."
}
