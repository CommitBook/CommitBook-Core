package scheduler

import (
	"strings"
	"testing"
)

func TestPlistLabel(t *testing.T) {
	label := PlistLabel("/Users/test/notes")
	if !strings.HasPrefix(label, "com.commitbook.") {
		t.Errorf("expected label to start with com.commitbook., got %s", label)
	}
	// Should be deterministic
	label2 := PlistLabel("/Users/test/notes")
	if label != label2 {
		t.Error("expected same label for same path")
	}
	// Different paths should produce different labels
	label3 := PlistLabel("/Users/test/other")
	if label == label3 {
		t.Error("expected different labels for different paths")
	}
}

func TestPlistPath(t *testing.T) {
	path, err := PlistPath("/Users/test/notes")
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(path, "LaunchAgents") {
		t.Errorf("expected path to contain LaunchAgents, got %s", path)
	}
	if !strings.HasSuffix(path, ".plist") {
		t.Errorf("expected path to end with .plist, got %s", path)
	}
}
