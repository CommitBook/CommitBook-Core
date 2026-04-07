package cmd

import (
	"fmt"
	"os"

	"github.com/ZAAI-com/CommitBook/internal/config"
	"github.com/ZAAI-com/CommitBook/internal/scheduler"
	"github.com/fatih/color"
	"github.com/spf13/cobra"
)

var startCmd = &cobra.Command{
	Use:   "start",
	Short: "Start automatic commits for this repository",
	Long:  `Creates and loads a macOS launchd agent to run automatic commits on schedule.`,
	RunE:  runStart,
}

func init() {
	rootCmd.AddCommand(startCmd)
}

func runStart(cmd *cobra.Command, args []string) error {
	repoDir, err := config.ResolveRepoPath(repoPath)
	if err != nil {
		return fmt.Errorf("resolving repo path: %w", err)
	}

	if !config.IsRepoSetUp(repoDir) {
		return fmt.Errorf("repo not set up — run 'commitbook setup' first")
	}

	repoCfg, err := config.LoadRepoConfig(repoDir)
	if err != nil {
		return err
	}

	// Check if already running
	if repoCfg.SchedulerID != "" && scheduler.IsLoaded(repoDir) {
		color.Yellow("CommitBook is already running for this repository.")
		fmt.Printf("  Schedule: %s\n", scheduler.DescribeSchedule(repoCfg.Schedule))
		return nil
	}

	// Get the commitbook binary path
	binPath, err := os.Executable()
	if err != nil {
		return fmt.Errorf("cannot determine binary path: %w", err)
	}

	// Capture current PATH for launchd
	pathEnv := os.Getenv("PATH")

	// Install the launchd job
	schedulerID, err := scheduler.Install(repoDir, repoCfg.Schedule, binPath, pathEnv)
	if err != nil {
		return fmt.Errorf("installing scheduler: %w", err)
	}

	// Update config
	repoCfg.Enabled = true
	repoCfg.SchedulerID = schedulerID
	if err := config.SaveRepoConfig(repoDir, repoCfg); err != nil {
		return fmt.Errorf("saving config: %w", err)
	}

	// Update global config
	globalCfg, err := config.LoadGlobalConfig()
	if err == nil {
		globalCfg.Repos[repoDir] = config.RepoEntry{
			Enabled:  true,
			Schedule: repoCfg.Schedule,
		}
		config.SaveGlobalConfig(globalCfg)
	}

	green := color.New(color.FgGreen, color.Bold)
	green.Println("✓ CommitBook started")
	fmt.Printf("  Schedule:  %s\n", scheduler.DescribeSchedule(repoCfg.Schedule))
	fmt.Printf("  Repo:      %s\n", repoDir)
	fmt.Printf("  Branch:    %s\n", repoCfg.Git.Branch)
	fmt.Printf("  Auto-push: yes\n")

	return nil
}
