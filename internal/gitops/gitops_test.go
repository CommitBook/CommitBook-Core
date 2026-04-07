package gitops

import (
	"os"
	"os/exec"
	"path/filepath"
	"testing"
)

func TestParsePorcelain(t *testing.T) {
	tests := []struct {
		name     string
		input    string
		wantNew  int
		wantMod  int
		wantDel  int
		wantDesc string
	}{
		{
			name:     "empty",
			input:    "",
			wantNew:  0,
			wantMod:  0,
			wantDel:  0,
			wantDesc: "no changes",
		},
		{
			name:     "new files",
			input:    "?? file1.md\n?? file2.md\n",
			wantNew:  2,
			wantMod:  0,
			wantDel:  0,
			wantDesc: "2 new",
		},
		{
			name:     "modified files",
			input:    " M notes.md\n M todo.md\n",
			wantNew:  0,
			wantMod:  2,
			wantDel:  0,
			wantDesc: "2 modified",
		},
		{
			name:     "deleted files",
			input:    " D old.md\n",
			wantNew:  0,
			wantMod:  0,
			wantDel:  1,
			wantDesc: "1 deleted",
		},
		{
			name:     "mixed changes",
			input:    "?? new.md\n M edited.md\n D removed.md\n",
			wantNew:  1,
			wantMod:  1,
			wantDel:  1,
			wantDesc: "1 new, 1 modified, 1 deleted",
		},
		{
			name:     "added file (staged)",
			input:    "A  staged.md\n",
			wantNew:  1,
			wantMod:  0,
			wantDel:  0,
			wantDesc: "1 new",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			summary := parsePorcelain(tt.input)
			if len(summary.New) != tt.wantNew {
				t.Errorf("new: got %d, want %d", len(summary.New), tt.wantNew)
			}
			if len(summary.Modified) != tt.wantMod {
				t.Errorf("modified: got %d, want %d", len(summary.Modified), tt.wantMod)
			}
			if len(summary.Deleted) != tt.wantDel {
				t.Errorf("deleted: got %d, want %d", len(summary.Deleted), tt.wantDel)
			}
			if summary.String() != tt.wantDesc {
				t.Errorf("description: got %q, want %q", summary.String(), tt.wantDesc)
			}
		})
	}
}

func TestChangesSummary(t *testing.T) {
	empty := ChangesSummary{}
	if !empty.IsClean() {
		t.Error("expected empty summary to be clean")
	}
	if empty.Total() != 0 {
		t.Error("expected total 0")
	}

	changes := ChangesSummary{
		New:      []string{"a.md"},
		Modified: []string{"b.md", "c.md"},
	}
	if changes.IsClean() {
		t.Error("expected non-empty summary to not be clean")
	}
	if changes.Total() != 3 {
		t.Errorf("expected total 3, got %d", changes.Total())
	}
}

func TestIsRepo(t *testing.T) {
	// Create a temp git repo
	repo := createTempRepo(t)
	if !IsRepo(repo) {
		t.Error("expected temp repo to be a git repo")
	}

	// Non-repo directory
	tmpDir := t.TempDir()
	if IsRepo(tmpDir) {
		t.Error("expected non-repo dir to return false")
	}
}

func TestHasRemote(t *testing.T) {
	repo := createTempRepo(t)
	if HasRemote(repo) {
		t.Error("expected fresh repo to have no remotes")
	}
}

func TestCurrentBranch(t *testing.T) {
	repo := createTempRepo(t)
	branch, err := CurrentBranch(repo)
	if err != nil {
		t.Fatalf("CurrentBranch: %v", err)
	}
	// Git default branch is usually main or master
	if branch == "" {
		t.Error("expected non-empty branch name")
	}
}

func TestStatusAndStageAll(t *testing.T) {
	repo := createTempRepo(t)

	// Add a file
	if err := os.WriteFile(filepath.Join(repo, "test.md"), []byte("hello"), 0644); err != nil {
		t.Fatal(err)
	}

	summary, err := Status(repo)
	if err != nil {
		t.Fatalf("Status: %v", err)
	}
	if summary.IsClean() {
		t.Error("expected changes after adding file")
	}
	if len(summary.New) != 1 {
		t.Errorf("expected 1 new file, got %d", len(summary.New))
	}

	// Stage all
	if err := StageAll(repo); err != nil {
		t.Fatalf("StageAll: %v", err)
	}
}

func TestCommit(t *testing.T) {
	repo := createTempRepo(t)

	// Add and commit a file
	if err := os.WriteFile(filepath.Join(repo, "test.md"), []byte("hello"), 0644); err != nil {
		t.Fatal(err)
	}
	if err := StageAll(repo); err != nil {
		t.Fatal(err)
	}
	hash, err := Commit(repo, "test commit")
	if err != nil {
		t.Fatalf("Commit: %v", err)
	}
	if hash == "" {
		t.Error("expected non-empty hash")
	}
}

func createTempRepo(t *testing.T) string {
	t.Helper()
	dir := t.TempDir()

	cmds := [][]string{
		{"git", "init", dir},
		{"git", "-C", dir, "config", "user.email", "test@test.com"},
		{"git", "-C", dir, "config", "user.name", "Test"},
		{"git", "-C", dir, "config", "commit.gpgsign", "false"},
	}

	// Create initial commit so HEAD exists
	initialFile := filepath.Join(dir, ".gitkeep")
	if err := os.WriteFile(initialFile, []byte(""), 0644); err != nil {
		t.Fatal(err)
	}
	cmds = append(cmds,
		[]string{"git", "-C", dir, "add", "."},
		[]string{"git", "-C", dir, "commit", "-m", "initial"},
	)

	for _, args := range cmds {
		cmd := exec.Command(args[0], args[1:]...)
		cmd.Stdout = nil
		cmd.Stderr = nil
		if err := cmd.Run(); err != nil {
			t.Fatalf("setup cmd %v: %v", args, err)
		}
	}
	return dir
}
