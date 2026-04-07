package scheduler

import (
	"fmt"
	"strconv"
	"strings"
)

// Presets maps friendly names to cron expressions.
var Presets = map[string]string{
	"every-5m":  "*/5 * * * *",
	"every-15m": "*/15 * * * *",
	"every-30m": "*/30 * * * *",
	"every-1h":  "0 * * * *",
	"hourly":    "0 * * * *",
	"every-4h":  "0 */4 * * *",
	"daily":     "0 9 * * *",
}

// ResolveSchedule converts a preset name or cron expression to a cron expression.
func ResolveSchedule(input string) (string, error) {
	input = strings.TrimSpace(strings.ToLower(input))

	if cron, ok := Presets[input]; ok {
		return cron, nil
	}

	// Treat as raw cron expression
	if err := ValidateCron(input); err != nil {
		return "", err
	}
	return input, nil
}

// ValidateCron validates a 5-field cron expression.
func ValidateCron(expr string) error {
	fields := strings.Fields(expr)
	if len(fields) != 5 {
		return fmt.Errorf("cron expression must have 5 fields, got %d: %q", len(fields), expr)
	}

	names := []string{"minute", "hour", "day-of-month", "month", "day-of-week"}
	maxValues := []int{59, 23, 31, 12, 7}

	for i, field := range fields {
		if err := validateCronField(field, maxValues[i]); err != nil {
			return fmt.Errorf("invalid %s field %q: %w", names[i], field, err)
		}
	}
	return nil
}

// CronToIntervalSeconds converts common cron patterns to a launchd StartInterval.
func CronToIntervalSeconds(expr string) int {
	fields := strings.Fields(expr)
	if len(fields) != 5 {
		return 3600 // default: hourly
	}

	minute := fields[0]
	hour := fields[1]

	// */N * * * * -> every N minutes
	if strings.HasPrefix(minute, "*/") {
		n, err := strconv.Atoi(strings.TrimPrefix(minute, "*/"))
		if err == nil && n > 0 {
			return n * 60
		}
	}

	// 0 */N * * * -> every N hours
	if minute == "0" && strings.HasPrefix(hour, "*/") {
		n, err := strconv.Atoi(strings.TrimPrefix(hour, "*/"))
		if err == nil && n > 0 {
			return n * 3600
		}
	}

	// 0 * * * * -> every hour
	if minute == "0" && hour == "*" {
		return 3600
	}

	// Default: hourly
	return 3600
}

// DescribeSchedule returns a human-readable description of a cron expression.
func DescribeSchedule(expr string) string {
	// Check presets first (reverse lookup)
	for name, cron := range Presets {
		if cron == expr {
			switch name {
			case "every-5m":
				return "Every 5 minutes"
			case "every-15m":
				return "Every 15 minutes"
			case "every-30m":
				return "Every 30 minutes"
			case "every-1h", "hourly":
				return "Every hour"
			case "every-4h":
				return "Every 4 hours"
			case "daily":
				return "Daily at 9:00 AM"
			}
		}
	}

	// Try to describe common patterns
	fields := strings.Fields(expr)
	if len(fields) != 5 {
		return expr
	}

	minute := fields[0]
	hour := fields[1]

	if strings.HasPrefix(minute, "*/") {
		n := strings.TrimPrefix(minute, "*/")
		return fmt.Sprintf("Every %s minutes", n)
	}

	if minute == "0" && strings.HasPrefix(hour, "*/") {
		n := strings.TrimPrefix(hour, "*/")
		return fmt.Sprintf("Every %s hours", n)
	}

	return fmt.Sprintf("Cron: %s", expr)
}

// ListPresets returns a formatted string of available presets.
func ListPresets() string {
	return `Available presets:
  every-5m   — Every 5 minutes
  every-15m  — Every 15 minutes
  every-30m  — Every 30 minutes
  hourly     — Every hour (default)
  every-4h   — Every 4 hours
  daily      — Daily at 9:00 AM`
}

func validateCronField(field string, maxVal int) error {
	if field == "*" {
		return nil
	}

	// */N
	if strings.HasPrefix(field, "*/") {
		n, err := strconv.Atoi(strings.TrimPrefix(field, "*/"))
		if err != nil {
			return fmt.Errorf("invalid step value")
		}
		if n < 1 || n > maxVal {
			return fmt.Errorf("step value %d out of range (1-%d)", n, maxVal)
		}
		return nil
	}

	// N-M
	if strings.Contains(field, "-") {
		parts := strings.SplitN(field, "-", 2)
		low, err := strconv.Atoi(parts[0])
		if err != nil {
			return fmt.Errorf("invalid range start")
		}
		high, err := strconv.Atoi(parts[1])
		if err != nil {
			return fmt.Errorf("invalid range end")
		}
		if low < 0 || high > maxVal || low > high {
			return fmt.Errorf("range %d-%d out of bounds (0-%d)", low, high, maxVal)
		}
		return nil
	}

	// N,M,...
	for _, part := range strings.Split(field, ",") {
		n, err := strconv.Atoi(strings.TrimSpace(part))
		if err != nil {
			return fmt.Errorf("invalid value %q", part)
		}
		if n < 0 || n > maxVal {
			return fmt.Errorf("value %d out of range (0-%d)", n, maxVal)
		}
	}
	return nil
}
