package cmd

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/ZAAI-com/CommitBook/internal/config"
	"github.com/ZAAI-com/CommitBook/internal/gitops"
	"github.com/fatih/color"
	"github.com/spf13/cobra"
)

var setupCmd = &cobra.Command{
	Use:   "setup",
	Short: "Initialize a repository for automatic commits",
	Long:  `Sets up a git repository for CommitBook by creating the .CommitBook directory, configuration, and updating .gitignore.`,
	RunE:  runSetup,
}

func init() {
	rootCmd.AddCommand(setupCmd)
}

func runSetup(cmd *cobra.Command, args []string) error {
	repoDir, err := config.ResolveRepoPath(repoPath)
	if err != nil {
		return fmt.Errorf("resolving repo path: %w", err)
	}

	// Verify it's a git repo
	if !gitops.IsRepo(repoDir) {
		return fmt.Errorf("%s is not a git repository", repoDir)
	}

	// Check if already set up
	if config.IsRepoSetUp(repoDir) {
		color.Yellow("CommitBook is already set up in %s", repoDir)
		color.Yellow("Run 'commitbook doctor' to check configuration health.")
		return nil
	}

	// Get current branch
	branch, err := gitops.CurrentBranch(repoDir)
	if err != nil {
		branch = "main"
	}

	// Check for remote
	if !gitops.HasRemote(repoDir) {
		color.Yellow("Warning: No git remote configured. Auto-push will fail until a remote is added.")
	}

	// Create .CommitBook directory
	commitBookDir := config.RepoCommitBookDir(repoDir)
	if err := os.MkdirAll(commitBookDir, 0755); err != nil {
		return fmt.Errorf("creating .CommitBook directory: %w", err)
	}

	// Create Logs directory
	logsDir := config.RepoLogsPath(repoDir)
	if err := os.MkdirAll(logsDir, 0755); err != nil {
		return fmt.Errorf("creating Logs directory: %w", err)
	}

	// Write default repo config
	repoCfg := config.DefaultRepoConfig(branch)
	if err := config.SaveRepoConfig(repoDir, repoCfg); err != nil {
		return fmt.Errorf("writing repo config: %w", err)
	}

	// Update .gitignore
	if err := updateGitignore(repoDir); err != nil {
		return fmt.Errorf("updating .gitignore: %w", err)
	}

	// Register in global config
	globalCfg, err := config.LoadGlobalConfig()
	if err != nil {
		return fmt.Errorf("loading global config: %w", err)
	}
	globalCfg.Repos[repoDir] = config.RepoEntry{
		Enabled:  true,
		Schedule: repoCfg.Schedule,
	}
	if err := config.SaveGlobalConfig(globalCfg); err != nil {
		return fmt.Errorf("saving global config: %w", err)
	}

	// Print success
	green := color.New(color.FgGreen, color.Bold)
	green.Printf("✓ CommitBook initialized in %s\n", repoDir)
	fmt.Println()
	fmt.Printf("  Config:   %s\n", config.RepoConfigPath(repoDir))
	fmt.Printf("  Logs:     %s\n", logsDir)
	fmt.Printf("  Schedule: Every hour (default)\n")
	fmt.Printf("  Branch:   %s\n", branch)
	fmt.Println()
	fmt.Println("Next steps:")
	fmt.Println("  commitbook doctor   — verify system requirements")
	fmt.Println("  commitbook start    — begin automatic commits")

	return nil
}

func updateGitignore(repoDir string) error {
	gitignorePath := filepath.Join(repoDir, ".gitignore")

	var existing string
	data, err := os.ReadFile(gitignorePath)
	if err == nil {
		existing = string(data)
	}

	linesToAdd := []string{
		".CommitBook/Logs/",
		".CommitBook/.lock",
	}

	var additions []string
	for _, line := range linesToAdd {
		if !strings.Contains(existing, line) {
			additions = append(additions, line)
		}
	}

	if len(additions) == 0 {
		return nil
	}

	f, err := os.OpenFile(gitignorePath, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0644)
	if err != nil {
		return err
	}
	defer f.Close()

	// Add a newline separator if the file doesn't end with one
	if len(existing) > 0 && !strings.HasSuffix(existing, "\n") {
		f.WriteString("\n")
	}

	// Add header comment if this is the first time
	if !strings.Contains(existing, "# CommitBook") {
		f.WriteString("\n# CommitBook\n")
	}

	for _, line := range additions {
		f.WriteString(line + "\n")
	}

	return nil
}
