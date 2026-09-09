use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use commitbook_engine::config::local::LocalConfig;
use commitbook_engine::cron::{self, SystemScheduler};
use commitbook_engine::git::{ChangesSummary, GitRepo};
use commitbook_engine::logger::FileLogger;
use commitbook_engine::settings::{self, SchedulerContext};

const TICK_RATE: Duration = Duration::from_secs(5);
const POLL_RATE: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Status,
    Logs,
    Config,
    Providers,
}

impl Panel {
    pub fn next(self) -> Self {
        match self {
            Panel::Status => Panel::Logs,
            Panel::Logs => Panel::Config,
            Panel::Config => Panel::Providers,
            Panel::Providers => Panel::Status,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Panel::Status => Panel::Providers,
            Panel::Logs => Panel::Status,
            Panel::Config => Panel::Logs,
            Panel::Providers => Panel::Config,
        }
    }
}

pub struct App {
    pub repo_path: PathBuf,
    pub active_panel: Panel,
    pub running: bool,
    pub schedule: String,
    pub schedule_desc: String,
    pub auto_push: bool,
    pub branch: String,
    pub last_commit: Option<String>,
    pub log_lines: Vec<LogEntry>,
    pub log_scroll: usize,
    pub providers: Vec<(String, String, bool)>,
    pub changes: ChangesSummary,
    pub current_branch: String,
    pub enabled: bool,
    pub log_level: String,
    /// Outcome of the most recent start/stop action, cleared on success.
    pub action_error: Option<String>,
    pub quit: bool,
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: String,
    pub message: String,
}

impl LogEntry {
    fn parse(line: &str) -> Option<Self> {
        let v: serde_json::Value = serde_json::from_str(line).ok()?;
        Some(Self {
            timestamp: v["ts"].as_str().unwrap_or("").to_string(),
            level: v["level"].as_str().unwrap_or("INFO").to_string(),
            message: v["msg"].as_str().unwrap_or("").to_string(),
        })
    }
}

impl App {
    pub fn new(repo_path: &Path) -> Self {
        let mut app = Self::blank(repo_path);
        app.refresh();
        app
    }

    /// An app with empty state and no repository inspection performed.
    pub fn blank(repo_path: &Path) -> Self {
        Self {
            repo_path: repo_path.to_path_buf(),
            active_panel: Panel::Status,
            running: false,
            schedule: String::new(),
            schedule_desc: String::new(),
            auto_push: true,
            branch: "main".to_string(),
            last_commit: None,
            log_lines: Vec::new(),
            log_scroll: 0,
            providers: Vec::new(),
            changes: ChangesSummary::default(),
            current_branch: String::new(),
            enabled: true,
            log_level: "info".to_string(),
            action_error: None,
            quit: false,
        }
    }

    pub fn refresh(&mut self) {
        // Fail closed on config errors, including after AI was previously enabled.
        let mut ai_messages = false;
        // Load local config
        if let Ok(config) = LocalConfig::load(&self.repo_path) {
            ai_messages = config.commit.ai_messages;
            self.schedule = config.schedule.clone();
            self.schedule_desc = cron::describe_schedule(&config.schedule);
            self.auto_push = config.git.auto_push;
            self.branch = config.git.branch.clone();
            self.last_commit = None; // moved to state.toml
            self.enabled = config.enabled;
            self.log_level = config.logging.level.clone();
        }

        // Check scheduler state
        self.running = cron::is_loaded(&self.repo_path);

        // Load git info
        if let Ok(repo) = GitRepo::open(&self.repo_path) {
            self.current_branch = repo.current_branch().unwrap_or_else(|_| "unknown".into());
            self.changes = repo.changes_summary().unwrap_or_default();
        }

        // Load log entries
        if let Ok(logger) = FileLogger::new(&self.repo_path, 30) {
            if let Ok(lines) = logger.read_entries(100, 0) {
                self.log_lines = lines.iter().filter_map(|l| LogEntry::parse(l)).collect();
            }
        }

        self.providers.clear();
        if !ai_messages {
            return;
        }
        // Check provider availability only after opt-in.
        let chain = commitbook_engine::ai::ProviderChain::new();
        let default_keys = vec![
            "gh-copilot".to_string(),
            "claude-cli".to_string(),
            "codex-cli".to_string(),
        ];
        self.providers = chain.check_availability(&default_keys);
    }

    pub fn handle_key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        match code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => self.quit = true,
            KeyCode::Tab => self.active_panel = self.active_panel.next(),
            KeyCode::BackTab => self.active_panel = self.active_panel.prev(),
            KeyCode::Char('r') => self.refresh(),
            KeyCode::Char('s') => self.toggle_scheduler(),
            KeyCode::Up | KeyCode::Char('k')
                if self.active_panel == Panel::Logs && self.log_scroll > 0 =>
            {
                self.log_scroll -= 1;
            }
            KeyCode::Down | KeyCode::Char('j')
                if self.active_panel == Panel::Logs
                    && self.log_scroll < self.log_lines.len().saturating_sub(1) =>
            {
                self.log_scroll += 1;
            }
            _ => {}
        }
    }

    fn toggle_scheduler(&mut self) {
        let context = SchedulerContext::new(&SystemScheduler, settings::current_binary());
        let result = if self.running {
            settings::stop_scheduler(&self.repo_path, &context)
        } else {
            settings::start_scheduler(&self.repo_path, &context).map(|_| ())
        };
        self.action_error = result.err().map(|error| format!("{error:#}"));
        self.refresh();
    }
}

pub fn run(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    repo_path: &Path,
) -> Result<()> {
    let mut app = App::new(repo_path);
    let mut last_tick = Instant::now();

    loop {
        terminal.draw(|f| crate::ui::draw(f, &app))?;

        if event::poll(POLL_RATE)? {
            if let Event::Key(key) = event::read()? {
                app.handle_key(key.code, key.modifiers);
            }
        }

        if app.quit {
            return Ok(());
        }

        if last_tick.elapsed() >= TICK_RATE {
            app.refresh();
            last_tick = Instant::now();
        }
    }
}

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;
