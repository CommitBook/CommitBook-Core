package cmd

import (
	"github.com/spf13/cobra"
)

var setCmd = &cobra.Command{
	Use:   "set",
	Short: "Configure CommitBook settings",
	Long:  `Parent command for configuring CommitBook settings like schedule.`,
}

func init() {
	rootCmd.AddCommand(setCmd)
}
