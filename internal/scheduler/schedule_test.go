package scheduler

import (
	"testing"
)

func TestResolveSchedulePresets(t *testing.T) {
	tests := []struct {
		input string
		want  string
	}{
		{"hourly", "0 * * * *"},
		{"every-5m", "*/5 * * * *"},
		{"every-15m", "*/15 * * * *"},
		{"every-30m", "*/30 * * * *"},
		{"every-1h", "0 * * * *"},
		{"HOURLY", "0 * * * *"}, // case insensitive
	}

	for _, tt := range tests {
		t.Run(tt.input, func(t *testing.T) {
			got, err := ResolveSchedule(tt.input)
			if err != nil {
				t.Fatalf("ResolveSchedule(%q): %v", tt.input, err)
			}
			if got != tt.want {
				t.Errorf("got %q, want %q", got, tt.want)
			}
		})
	}
}

func TestResolveScheduleRawCron(t *testing.T) {
	got, err := ResolveSchedule("*/10 * * * *")
	if err != nil {
		t.Fatal(err)
	}
	if got != "*/10 * * * *" {
		t.Errorf("got %q", got)
	}
}

func TestResolveScheduleInvalid(t *testing.T) {
	_, err := ResolveSchedule("not-a-preset")
	if err == nil {
		t.Error("expected error for invalid input")
	}
}

func TestValidateCron(t *testing.T) {
	valid := []string{
		"* * * * *",
		"0 * * * *",
		"*/5 * * * *",
		"0 9 * * 1-5",
		"30 8,12,18 * * *",
	}
	for _, expr := range valid {
		if err := ValidateCron(expr); err != nil {
			t.Errorf("ValidateCron(%q) should be valid: %v", expr, err)
		}
	}

	invalid := []string{
		"",
		"* * *",
		"60 * * * *",
		"* 25 * * *",
		"a b c d e",
	}
	for _, expr := range invalid {
		if err := ValidateCron(expr); err == nil {
			t.Errorf("ValidateCron(%q) should be invalid", expr)
		}
	}
}

func TestCronToIntervalSeconds(t *testing.T) {
	tests := []struct {
		expr string
		want int
	}{
		{"*/5 * * * *", 300},
		{"*/15 * * * *", 900},
		{"*/30 * * * *", 1800},
		{"0 * * * *", 3600},
		{"0 */4 * * *", 14400},
	}

	for _, tt := range tests {
		t.Run(tt.expr, func(t *testing.T) {
			got := CronToIntervalSeconds(tt.expr)
			if got != tt.want {
				t.Errorf("CronToIntervalSeconds(%q) = %d, want %d", tt.expr, got, tt.want)
			}
		})
	}
}

func TestDescribeSchedule(t *testing.T) {
	tests := []struct {
		expr string
		want string
	}{
		{"*/5 * * * *", "Every 5 minutes"},
		{"0 * * * *", "Every hour"},
		{"*/30 * * * *", "Every 30 minutes"},
	}

	for _, tt := range tests {
		t.Run(tt.expr, func(t *testing.T) {
			got := DescribeSchedule(tt.expr)
			if got != tt.want {
				t.Errorf("got %q, want %q", got, tt.want)
			}
		})
	}
}
