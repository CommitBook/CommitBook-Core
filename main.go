package main

import (
	"os"

	"github.com/ZAAI-com/CommitBook/cmd"
)

var version = "dev"

func main() {
	cmd.SetVersion(version)
	if err := cmd.Execute(); err != nil {
		os.Exit(1)
	}
}
