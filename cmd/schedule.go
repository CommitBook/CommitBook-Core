package cmd

import (
	"fmt"
	"os"

	"github.com/ZAAI-com/CommitBook/internal/config"
	"github.com/ZAAI-com/CommitBook/internal/scheduler"
	"github.com/fatih/color"
	"github.com/spf13/cobra"
)

var scheduleCmd = &cobra.Command{
	Use:   "schedule [expression]",
	Short: "Set the auto-commit schedule",
	Long: `Set the auto-commit schedule using a preset or cron expression.

` + scheduler.ListPresets() + `

Custom: any valid 5-field cron expression (e.g., "*/10 * * * *")`,
	Args: cobra.ExactArgs(1),
	RunE: runSchedule,
}

func init() {
	rootCmd.AddCommand(scheduleCmd)
}

func runSchedule(cmd *cobra.Command, args []string) error {
	repoDir, err := config.ResolveRepoPath(repoPath)
	if err != nil {
		return fmt.Errorf("resolving repo path: %w", err)
	}

	if !config.IsRepoSetUp(repoDir) {
		return fmt.Errorf("repo not set up — run 'commitbook setup' first")
	}

	// Resolve the schedule expression
	newSchedule, err := scheduler.ResolveSchedule(args[0])
	if err != nil {
		return fmt.Errorf("invalid schedule: %w\n\n%s", err, scheduler.ListPresets())
	}

	// Load current config
	repoCfg, err := config.LoadRepoConfig(repoDir)
	if err != nil {
		return err
	}

	oldSchedule := repoCfg.Schedule
	if oldSchedule == newSchedule {
		fmt.Printf("Schedule is already set to %s\n", scheduler.DescribeSchedule(newSchedule))
		return nil
	}

	// Update config
	repoCfg.Schedule = newSchedule
	if err := config.SaveRepoConfig(repoDir, repoCfg); err != nil {
		return fmt.Errorf("saving config: %w", err)
	}

	// Update global config
	globalCfg, err := config.LoadGlobalConfig()
	if err == nil {
		if entry, ok := globalCfg.Repos[repoDir]; ok {
			entry.Schedule = newSchedule
			globalCfg.Repos[repoDir] = entry
			config.SaveGlobalConfig(globalCfg)
		}
	}

	// If currently running, reinstall with new schedule
	if repoCfg.SchedulerID != "" && scheduler.IsLoaded(repoDir) {
		binPath, err := os.Executable()
		if err != nil {
			return fmt.Errorf("cannot determine binary path: %w", err)
		}
		pathEnv := os.Getenv("PATH")

		schedulerID, err := scheduler.Install(repoDir, newSchedule, binPath, pathEnv)
		if err != nil {
			return fmt.Errorf("reinstalling scheduler: %w", err)
		}
		repoCfg.SchedulerID = schedulerID
		config.SaveRepoConfig(repoDir, repoCfg)

		color.Green("✓ Schedule updated and scheduler restarted")
	} else {
		color.Green("✓ Schedule updated")
	}

	fmt.Printf("  Old: %s\n", scheduler.DescribeSchedule(oldSchedule))
	fmt.Printf("  New: %s\n", scheduler.DescribeSchedule(newSchedule))

	return nil
}
