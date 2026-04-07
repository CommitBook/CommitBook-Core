package ai

import (
	"strings"
	"testing"

	"github.com/ZAAI-com/CommitBook/internal/gitops"
)

func TestFallbackProvider(t *testing.T) {
	p := &FallbackProvider{}

	if !p.IsAvailable() {
		t.Error("fallback should always be available")
	}

	if p.Name() != "Fallback" {
		t.Errorf("expected name 'Fallback', got %q", p.Name())
	}

	summary := gitops.ChangesSummary{
		New:      []string{"notes.md"},
		Modified: []string{"todo.md"},
	}

	msg, err := p.Generate(summary, "/tmp")
	if err != nil {
		t.Fatalf("Generate: %v", err)
	}

	if !strings.HasPrefix(msg, "Writing ") {
		t.Errorf("expected message to start with 'Writing ', got %q", msg)
	}
	if !strings.Contains(msg, "1 new") {
		t.Errorf("expected message to contain '1 new', got %q", msg)
	}
	if !strings.Contains(msg, "1 modified") {
		t.Errorf("expected message to contain '1 modified', got %q", msg)
	}
}

func TestFallbackProviderClean(t *testing.T) {
	p := &FallbackProvider{}
	summary := gitops.ChangesSummary{}

	msg, err := p.Generate(summary, "/tmp")
	if err != nil {
		t.Fatal(err)
	}

	if strings.Contains(msg, "(") {
		t.Errorf("clean summary should not have parenthetical, got %q", msg)
	}
}

func TestGenerateCommitMessageFallback(t *testing.T) {
	summary := gitops.ChangesSummary{
		Modified: []string{"test.md"},
	}

	// With only "fallback" in providers, should use fallback
	msg, provider := GenerateCommitMessage(summary, []string{"fallback"}, "/tmp")
	if provider != "Fallback" {
		t.Errorf("expected Fallback provider, got %q", provider)
	}
	if !strings.HasPrefix(msg, "Writing ") {
		t.Errorf("expected fallback message, got %q", msg)
	}
}

func TestGenerateCommitMessageUnknownProvider(t *testing.T) {
	summary := gitops.ChangesSummary{
		Modified: []string{"test.md"},
	}

	// Unknown providers should be skipped, falling through to ultimate fallback
	msg, provider := GenerateCommitMessage(summary, []string{"nonexistent"}, "/tmp")
	if provider != "Fallback" {
		t.Errorf("expected Fallback provider, got %q", provider)
	}
	if msg == "" {
		t.Error("expected non-empty message")
	}
}

func TestExtractMessage(t *testing.T) {
	tests := []struct {
		name  string
		input string
		want  string
	}{
		{"simple", "Update notes for today", "Update notes for today"},
		{"with quotes", `"Update notes for today"`, "Update notes for today"},
		{"with git prefix", `git commit -m "Update notes"`, "Update notes"},
		{"multiline", "# Suggestion\nUpdate notes for today\n", "Update notes for today"},
		{"empty", "", ""},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := extractMessage(tt.input)
			if got != tt.want {
				t.Errorf("extractMessage(%q) = %q, want %q", tt.input, got, tt.want)
			}
		})
	}
}

func TestCheckAvailability(t *testing.T) {
	result := CheckAvailability([]string{"fallback", "nonexistent"})
	if !result["fallback"] {
		t.Error("fallback should be available")
	}
	if result["nonexistent"] {
		t.Error("nonexistent should not be available")
	}
}
