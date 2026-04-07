package cmd

import (
	"fmt"

	"github.com/spf13/cobra"
)

var (
	repoPath string
	verbose  bool
	appVersion string
)

var rootCmd = &cobra.Command{
	Use:   "commitbook",
	Short: "Automated git commits for markdown notebooks",
	Long: `CommitBook turns any git repository into a self-saving notebook
by scheduling automatic commits and pushes with AI-powered commit messages.`,
}

func Execute() error {
	return rootCmd.Execute()
}

func SetVersion(v string) {
	appVersion = v
	rootCmd.Version = v
}

func init() {
	rootCmd.PersistentFlags().StringVar(&repoPath, "repo", "", "path to the git repository (default: current directory)")
	rootCmd.PersistentFlags().BoolVarP(&verbose, "verbose", "v", false, "enable verbose output")

	rootCmd.SetVersionTemplate(fmt.Sprintf("commitbook version %s\n", "{{.Version}}"))
}
