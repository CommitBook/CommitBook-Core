package scheduler

import (
	"bytes"
	"crypto/sha256"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"text/template"
)

const plistTemplate = `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{{.Label}}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{{.BinPath}}</string>
        <string>run</string>
        <string>--repo</string>
        <string>{{.RepoPath}}</string>
    </array>
    <key>StartInterval</key>
    <integer>{{.IntervalSeconds}}</integer>
    <key>StandardOutPath</key>
    <string>{{.StdoutLog}}</string>
    <key>StandardErrorPath</key>
    <string>{{.StderrLog}}</string>
    <key>RunAtLoad</key>
    <false/>
    <key>EnvironmentVariables</key>
    <dict>
        <key>PATH</key>
        <string>{{.PathEnv}}</string>
    </dict>
</dict>
</plist>
`

// PlistData holds the data for the launchd plist template.
type PlistData struct {
	Label           string
	BinPath         string
	RepoPath        string
	IntervalSeconds int
	StdoutLog       string
	StderrLog       string
	PathEnv         string
}

// PlistLabel generates a unique launchd label for the given repo path.
func PlistLabel(repoPath string) string {
	absPath, err := filepath.Abs(repoPath)
	if err != nil {
		absPath = repoPath
	}
	hash := sha256.Sum256([]byte(absPath))
	return fmt.Sprintf("com.commitbook.%x", hash[:6])
}

// PlistPath returns the path where the plist file should be written.
func PlistPath(repoPath string) (string, error) {
	home, err := os.UserHomeDir()
	if err != nil {
		return "", fmt.Errorf("cannot determine home directory: %w", err)
	}
	label := PlistLabel(repoPath)
	return filepath.Join(home, "Library", "LaunchAgents", label+".plist"), nil
}

// Install creates and loads a launchd plist for the given repo.
// Returns the plist label as the scheduler ID.
func Install(repoPath, schedule, binPath, pathEnv string) (string, error) {
	absRepo, err := filepath.Abs(repoPath)
	if err != nil {
		return "", fmt.Errorf("resolving repo path: %w", err)
	}

	label := PlistLabel(absRepo)
	plistPath, err := PlistPath(absRepo)
	if err != nil {
		return "", err
	}

	interval := CronToIntervalSeconds(schedule)
	logsDir := filepath.Join(absRepo, ".CommitBook", "Logs")

	data := PlistData{
		Label:           label,
		BinPath:         binPath,
		RepoPath:        absRepo,
		IntervalSeconds: interval,
		StdoutLog:       filepath.Join(logsDir, "launchd-stdout.log"),
		StderrLog:       filepath.Join(logsDir, "launchd-stderr.log"),
		PathEnv:         pathEnv,
	}

	// Render plist
	tmpl, err := template.New("plist").Parse(plistTemplate)
	if err != nil {
		return "", fmt.Errorf("parsing plist template: %w", err)
	}

	var buf bytes.Buffer
	if err := tmpl.Execute(&buf, data); err != nil {
		return "", fmt.Errorf("rendering plist: %w", err)
	}

	// Ensure LaunchAgents directory exists
	if err := os.MkdirAll(filepath.Dir(plistPath), 0755); err != nil {
		return "", fmt.Errorf("creating LaunchAgents directory: %w", err)
	}

	// Unload existing (ignore errors — may not exist)
	exec.Command("launchctl", "unload", plistPath).Run()

	// Write plist
	if err := os.WriteFile(plistPath, buf.Bytes(), 0644); err != nil {
		return "", fmt.Errorf("writing plist: %w", err)
	}

	// Load plist
	cmd := exec.Command("launchctl", "load", plistPath)
	var stderr bytes.Buffer
	cmd.Stderr = &stderr
	if err := cmd.Run(); err != nil {
		return "", fmt.Errorf("launchctl load: %s", strings.TrimSpace(stderr.String()))
	}

	return label, nil
}

// Uninstall unloads and removes the launchd plist for the given repo.
func Uninstall(repoPath string) error {
	absRepo, err := filepath.Abs(repoPath)
	if err != nil {
		return fmt.Errorf("resolving repo path: %w", err)
	}

	plistPath, err := PlistPath(absRepo)
	if err != nil {
		return err
	}

	// Unload (ignore errors — may not be loaded)
	exec.Command("launchctl", "unload", plistPath).Run()

	// Remove plist file
	if err := os.Remove(plistPath); err != nil && !os.IsNotExist(err) {
		return fmt.Errorf("removing plist: %w", err)
	}

	return nil
}

// IsLoaded checks if a launchd job with the given label is loaded.
func IsLoaded(repoPath string) bool {
	absRepo, err := filepath.Abs(repoPath)
	if err != nil {
		return false
	}
	label := PlistLabel(absRepo)
	cmd := exec.Command("launchctl", "list", label)
	return cmd.Run() == nil
}
