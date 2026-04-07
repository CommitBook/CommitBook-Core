package ai

import (
	"fmt"

	"github.com/ZAAI-com/CommitBook/internal/gitops"
)

// Provider generates commit messages.
type Provider interface {
	Name() string
	IsAvailable() bool
	Generate(summary gitops.ChangesSummary, repoPath string) (string, error)
}

// registry of all providers by key
var registry = map[string]Provider{}

func init() {
	providers := []Provider{
		&CopilotProvider{},
		&ClaudeProvider{},
		&CodexProvider{},
		&FallbackProvider{},
	}
	for _, p := range providers {
		registry[providerKey(p)] = p
	}
}

// GenerateCommitMessage tries each provider in order and returns the first successful result.
func GenerateCommitMessage(summary gitops.ChangesSummary, providerNames []string, repoPath string) (string, string) {
	for _, name := range providerNames {
		p, ok := registry[name]
		if !ok {
			continue
		}
		if !p.IsAvailable() {
			continue
		}
		msg, err := p.Generate(summary, repoPath)
		if err != nil {
			continue
		}
		if msg != "" {
			return msg, p.Name()
		}
	}

	// Ultimate fallback
	fb := &FallbackProvider{}
	msg, _ := fb.Generate(summary, repoPath)
	return msg, fb.Name()
}

// CheckAvailability returns a map of provider name -> available status.
func CheckAvailability(providerNames []string) map[string]bool {
	result := make(map[string]bool)
	for _, name := range providerNames {
		p, ok := registry[name]
		if !ok {
			result[name] = false
			continue
		}
		result[name] = p.IsAvailable()
	}
	return result
}

func providerKey(p Provider) string {
	switch p.(type) {
	case *CopilotProvider:
		return "gh-copilot"
	case *ClaudeProvider:
		return "claude-cli"
	case *CodexProvider:
		return "codex-cli"
	case *FallbackProvider:
		return "fallback"
	default:
		return fmt.Sprintf("unknown-%s", p.Name())
	}
}
