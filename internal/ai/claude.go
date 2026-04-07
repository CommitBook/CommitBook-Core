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

// ClaudeProvider generates commit messages using Claude Code CLI.
type ClaudeProvider struct{}

func (p *ClaudeProvider) Name() string {
	return "Claude Code"
}

func (p *ClaudeProvider) IsAvailable() bool {
	_, err := exec.LookPath("claude")
	return err == nil
}

func (p *ClaudeProvider) Generate(summary gitops.ChangesSummary, repoPath string) (string, error) {
	diffSummary, _ := gitops.DiffSummary(repoPath)

	prompt := fmt.Sprintf(
		"Write a concise one-line git commit message (max 72 chars, no quotes) for these changes: %s. Diff stats:\n%s",
		summary.String(),
		truncate(diffSummary, 500),
	)

	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	cmd := exec.CommandContext(ctx, "claude", "-p", prompt)
	cmd.Dir = repoPath
	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr

	if err := cmd.Run(); err != nil {
		return "", fmt.Errorf("claude: %w", err)
	}

	msg := strings.TrimSpace(stdout.String())
	// Clean up potential markdown formatting or quotes
	msg = strings.Trim(msg, "\"'`")
	msg = strings.TrimPrefix(msg, "```")
	msg = strings.TrimSuffix(msg, "```")
	msg = strings.TrimSpace(msg)

	if msg == "" {
		return "", fmt.Errorf("empty response from claude")
	}

	// Take only the first line
	if idx := strings.IndexByte(msg, '\n'); idx != -1 {
		msg = msg[:idx]
	}

	// Truncate to 72 chars
	if len(msg) > 72 {
		msg = msg[:72]
	}

	return msg, nil
}
