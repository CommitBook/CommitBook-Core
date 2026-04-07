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

// CodexProvider generates commit messages using OpenAI Codex CLI.
type CodexProvider struct{}

func (p *CodexProvider) Name() string {
	return "Codex"
}

func (p *CodexProvider) IsAvailable() bool {
	_, err := exec.LookPath("codex")
	return err == nil
}

func (p *CodexProvider) Generate(summary gitops.ChangesSummary, repoPath string) (string, error) {
	diffSummary, _ := gitops.DiffSummary(repoPath)

	prompt := fmt.Sprintf(
		"Write a concise one-line git commit message (max 72 chars, no quotes) for these changes: %s. Diff stats:\n%s",
		summary.String(),
		truncate(diffSummary, 500),
	)

	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	cmd := exec.CommandContext(ctx, "codex", "--quiet", prompt)
	cmd.Dir = repoPath
	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr

	if err := cmd.Run(); err != nil {
		return "", fmt.Errorf("codex: %w", err)
	}

	msg := strings.TrimSpace(stdout.String())
	msg = strings.Trim(msg, "\"'`")

	if msg == "" {
		return "", fmt.Errorf("empty response from codex")
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
