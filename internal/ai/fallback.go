package ai

import (
	"fmt"
	"time"

	"github.com/ZAAI-com/CommitBook/internal/gitops"
)

// FallbackProvider generates a simple timestamp-based commit message.
type FallbackProvider struct{}

func (p *FallbackProvider) Name() string {
	return "Fallback"
}

func (p *FallbackProvider) IsAvailable() bool {
	return true
}

func (p *FallbackProvider) Generate(summary gitops.ChangesSummary, repoPath string) (string, error) {
	timestamp := time.Now().Format("2006-01-02 15:04:05")
	msg := fmt.Sprintf("Writing %s", timestamp)
	if !summary.IsClean() {
		msg = fmt.Sprintf("%s (%s)", msg, summary.String())
	}
	return msg, nil
}
