package cmd

import (
	"fmt"
	"os"
	"os/exec"

	"github.com/ZAAI-com/CommitBook/internal/config"
	"github.com/ZAAI-com/CommitBook/internal/gitops"
	"github.com/fatih/color"
	"github.com/spf13/cobra"
)

var doctorCmd = &cobra.Command{
	Use:   "doctor",
	Short: "Check system requirements and configuration health",
	Long:  `Verifies that git, AI tools, and CommitBook configuration are properly set up.`,
	RunE:  runDoctor,
}

func init() {
	rootCmd.AddCommand(doctorCmd)
}

func runDoctor(cmd *cobra.Command, args []string) error {
	repoDir, err := config.ResolveRepoPath(repoPath)
	if err != nil {
		return fmt.Errorf("resolving repo path: %w", err)
	}

	green := color.New(color.FgGreen)
	red := color.New(color.FgRed)
	yellow := color.New(color.FgYellow)

	pass := 0
	fail := 0
	warn := 0

	check := func(name string, fn func() error) {
		if err := fn(); err != nil {
			red.Printf("  ✗ %s: %s\n", name, err)
			fail++
		} else {
			green.Printf("  ✓ %s\n", name)
			pass++
		}
	}

	optional := func(name string, fn func() error) {
		if err := fn(); err != nil {
			yellow.Printf("  ○ %s: %s\n", name, err)
			warn++
		} else {
			green.Printf("  ✓ %s\n", name)
			pass++
		}
	}

	fmt.Println("CommitBook Doctor")
	fmt.Println()

	// Core requirements
	fmt.Println("Core:")
	check("Git installed", func() error {
		return checkCommand("git", "--version")
	})
	check("Git repository", func() error {
		if !gitops.IsRepo(repoDir) {
			return fmt.Errorf("%s is not a git repository", repoDir)
		}
		return nil
	})
	check("Git remote accessible", func() error {
		if !gitops.HasRemote(repoDir) {
			return fmt.Errorf("no remote configured")
		}
		return gitops.CheckRemote(repoDir)
	})
	fmt.Println()

	// AI tools
	fmt.Println("AI commit message tools:")
	optional("GitHub Copilot CLI", func() error {
		if err := checkCommand("gh", "copilot", "--version"); err != nil {
			return fmt.Errorf("not available (install: gh extension install github/gh-copilot)")
		}
		return nil
	})
	optional("GitHub CLI authenticated", func() error {
		return checkCommand("gh", "auth", "status")
	})
	optional("Claude Code CLI", func() error {
		if _, err := exec.LookPath("claude"); err != nil {
			return fmt.Errorf("not found in PATH")
		}
		return nil
	})
	optional("Codex CLI", func() error {
		if _, err := exec.LookPath("codex"); err != nil {
			return fmt.Errorf("not found in PATH")
		}
		return nil
	})
	fmt.Println()

	// CommitBook configuration
	fmt.Println("CommitBook:")
	check("Configuration", func() error {
		if !config.IsRepoSetUp(repoDir) {
			return fmt.Errorf("not set up (run 'commitbook setup')")
		}
		_, err := config.LoadRepoConfig(repoDir)
		return err
	})
	check("Logs directory writable", func() error {
		logsDir := config.RepoLogsPath(repoDir)
		testFile := logsDir + "/.doctor-test"
		if err := os.MkdirAll(logsDir, 0755); err != nil {
			return fmt.Errorf("cannot create logs directory: %w", err)
		}
		if err := os.WriteFile(testFile, []byte("test"), 0644); err != nil {
			return fmt.Errorf("cannot write to logs directory: %w", err)
		}
		os.Remove(testFile)
		return nil
	})
	check("launchctl accessible", func() error {
		return checkCommand("launchctl", "print", fmt.Sprintf("gui/%d", os.Getuid()))
	})

	// Summary
	fmt.Println()
	fmt.Printf("Results: ")
	green.Printf("%d passed", pass)
	if fail > 0 {
		fmt.Print(", ")
		red.Printf("%d failed", fail)
	}
	if warn > 0 {
		fmt.Print(", ")
		yellow.Printf("%d optional", warn)
	}
	fmt.Println()

	if fail > 0 {
		return fmt.Errorf("%d check(s) failed", fail)
	}
	return nil
}

func checkCommand(name string, args ...string) error {
	cmd := exec.Command(name, args...)
	cmd.Stdout = nil
	cmd.Stderr = nil
	return cmd.Run()
}
