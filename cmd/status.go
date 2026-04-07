package cmd

import (
	"fmt"
	"time"

	"github.com/ZAAI-com/CommitBook/internal/config"
	"github.com/ZAAI-com/CommitBook/internal/gitops"
	"github.com/ZAAI-com/CommitBook/internal/scheduler"
	"github.com/fatih/color"
	"github.com/spf13/cobra"
)

var statusCmd = &cobra.Command{
	Use:   "status",
	Short: "Show current CommitBook state for this repository",
	Long:  `Displays whether auto-commits are running, the schedule, last commit time, and repository details.`,
	RunE:  runStatus,
}

func init() {
	rootCmd.AddCommand(statusCmd)
}

func runStatus(cmd *cobra.Command, args []string) error {
	repoDir, err := config.ResolveRepoPath(repoPath)
	if err != nil {
		return fmt.Errorf("resolving repo path: %w", err)
	}

	if !config.IsRepoSetUp(repoDir) {
		color.Yellow("CommitBook is not configured for this repository.")
		fmt.Println("Run 'commitbook setup' to initialize.")
		return nil
	}

	repoCfg, err := config.LoadRepoConfig(repoDir)
	if err != nil {
		return err
	}

	// Determine running state
	running := scheduler.IsLoaded(repoDir)

	fmt.Println("CommitBook Status")
	fmt.Println()

	// State
	if running {
		color.New(color.FgGreen, color.Bold).Print("  State:       ")
		color.Green("Running")
	} else {
		color.New(color.FgYellow, color.Bold).Print("  State:       ")
		color.Yellow("Stopped")
	}

	// Schedule
	fmt.Printf("  Schedule:    %s\n", scheduler.DescribeSchedule(repoCfg.Schedule))

	// Last commit
	if repoCfg.LastCommit != "" {
		lastTime, err := time.Parse(time.RFC3339, repoCfg.LastCommit)
		if err == nil {
			ago := time.Since(lastTime).Round(time.Second)
			fmt.Printf("  Last commit: %s (%s ago)\n", lastTime.Local().Format("2006-01-02 15:04:05"), ago)
		} else {
			fmt.Printf("  Last commit: %s\n", repoCfg.LastCommit)
		}
	} else {
		fmt.Println("  Last commit: Never")
	}

	// Next commit (estimated)
	if running && repoCfg.LastCommit != "" {
		lastTime, err := time.Parse(time.RFC3339, repoCfg.LastCommit)
		if err == nil {
			interval := time.Duration(scheduler.CronToIntervalSeconds(repoCfg.Schedule)) * time.Second
			nextTime := lastTime.Add(interval)
			if nextTime.After(time.Now()) {
				until := time.Until(nextTime).Round(time.Second)
				fmt.Printf("  Next commit: %s (in %s)\n", nextTime.Local().Format("15:04:05"), until)
			} else {
				fmt.Println("  Next commit: Imminent")
			}
		}
	} else if running {
		fmt.Println("  Next commit: Pending (first run)")
	} else {
		fmt.Println("  Next commit: N/A (stopped)")
	}

	fmt.Println()

	// Repository info
	fmt.Printf("  Repo:        %s\n", repoDir)
	fmt.Printf("  Branch:      %s\n", repoCfg.Git.Branch)
	fmt.Printf("  Auto-push:   %v\n", repoCfg.Git.AutoPush)

	if remote, err := gitops.RemoteURL(repoDir); err == nil {
		fmt.Printf("  Remote:      %s\n", remote)
	}

	return nil
}
