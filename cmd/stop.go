package cmd

import (
	"fmt"

	"github.com/ZAAI-com/CommitBook/internal/config"
	"github.com/ZAAI-com/CommitBook/internal/scheduler"
	"github.com/fatih/color"
	"github.com/spf13/cobra"
)

var stopCmd = &cobra.Command{
	Use:   "stop",
	Short: "Stop automatic commits for this repository",
	Long:  `Unloads the macOS launchd agent and stops automatic commits. Configuration and logs are preserved.`,
	RunE:  runStop,
}

func init() {
	rootCmd.AddCommand(stopCmd)
}

func runStop(cmd *cobra.Command, args []string) error {
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

	// Check if already stopped
	if repoCfg.SchedulerID == "" && !scheduler.IsLoaded(repoDir) {
		color.Yellow("CommitBook is not running for this repository.")
		return nil
	}

	// Uninstall the launchd job
	if err := scheduler.Uninstall(repoDir); err != nil {
		return fmt.Errorf("uninstalling scheduler: %w", err)
	}

	// Update config
	repoCfg.Enabled = false
	repoCfg.SchedulerID = ""
	if err := config.SaveRepoConfig(repoDir, repoCfg); err != nil {
		return fmt.Errorf("saving config: %w", err)
	}

	// Update global config
	globalCfg, err := config.LoadGlobalConfig()
	if err == nil {
		if entry, ok := globalCfg.Repos[repoDir]; ok {
			entry.Enabled = false
			globalCfg.Repos[repoDir] = entry
			config.SaveGlobalConfig(globalCfg)
		}
	}

	green := color.New(color.FgGreen, color.Bold)
	green.Println("✓ CommitBook stopped")
	fmt.Println("  Configuration and logs are preserved.")
	fmt.Println("  Run 'commitbook start' to resume.")

	return nil
}
