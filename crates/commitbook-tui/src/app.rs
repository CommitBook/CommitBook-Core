use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use commitbook_engine::config::local::LocalConfig;
use commitbook_engine::config::{CommitMode, ConflictMode};
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
    pub repository_status: Option<commitbook_engine::inspection::RepositoryStatus>,
    pub preview: Option<commitbook_engine::inspection::CommitPreview>,
    pub preview_scroll: usize,
    pub status_scroll: usize,
    pub repo_path: PathBuf,
    pub active_panel: Panel,
    pub running: bool,
    pub scheduler: cron::SchedulerHealth,
    pub scheduler_warning: Option<String>,
    pub schedule: String,
    pub schedule_desc: String,
    pub branch: String,
    /// `[commit]` and `[conflicts]` as shown in the config panel, e.g.
    /// `timestamp` or `ai (claude)`.
    pub commit: String,
    pub conflicts: String,
    pub log_keep: String,
    pub last_commit: Option<String>,
    pub log_lines: Vec<LogEntry>,
    pub log_scroll: usize,
    pub providers: Vec<(String, String, bool)>,
    pub changes: ChangesSummary,
    pub current_branch: String,
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
            repository_status: None,
            preview: None,
            preview_scroll: 0,
            status_scroll: 0,
            repo_path: repo_path.to_path_buf(),
            active_panel: Panel::Status,
            running: false,
            scheduler: cron::SchedulerHealth::Stopped,
            scheduler_warning: None,
            schedule: String::new(),
            schedule_desc: String::new(),
            branch: "main".to_string(),
            commit: String::new(),
            conflicts: String::new(),
            log_keep: String::new(),
            last_commit: None,
            log_lines: Vec::new(),
            log_scroll: 0,
            providers: Vec::new(),
            changes: ChangesSummary::default(),
            current_branch: String::new(),
            action_error: None,
            quit: false,
        }
    }

    pub fn refresh(&mut self) {
        // Fail closed on config errors, including after AI was previously enabled.
        let mut provider_keys = Vec::new();
        if let Ok(config) = LocalConfig::load_read_only(&self.repo_path) {
            provider_keys = commitbook_engine::ai::commit_provider_keys(
                config.commit.mode,
                config.commit.agent,
            );
            provider_keys.retain(|key| key != "fallback");
            self.schedule = config.sync.schedule.clone();
            self.schedule_desc = cron::describe_schedule(&config.sync.schedule);
            self.branch = config.git.branch.clone();
            self.commit = match config.commit.mode {
                CommitMode::Timestamp => "timestamp".into(),
                CommitMode::Ai => format!("ai ({})", config.commit.agent),
            };
            self.conflicts = match config.conflicts.mode {
                ConflictMode::Ai | ConflictMode::Review => {
                    format!("{} ({})", config.conflicts.mode, config.conflicts.agent)
                }
                mode => mode.to_string(),
            };
            self.log_keep = config.logs.keep.to_string();
        } else {
            self.schedule.clear();
            self.schedule_desc = "Unknown (configuration error)".into();
            self.branch = "unknown".into();
            self.commit = "unknown".into();
            self.conflicts = "unknown".into();
            self.log_keep = "unknown".into();
        }

        self.repository_status = Some(commitbook_engine::inspection::RepositoryStatus::read(
            &self.repo_path,
        ));
        self.last_commit = self
            .repository_status
            .as_ref()
            .and_then(|s| s.last_commit.clone());
        if self.preview.is_some() {
            self.preview = Some(commitbook_engine::inspection::preview(&self.repo_path));
        }

        // Check scheduler state
        self.scheduler = cron::health(&self.repo_path);
        self.running = self.scheduler.is_loaded();
        self.scheduler_warning = self.scheduler.warning(
            self.repository_status
                .as_ref()
                .and_then(|s| s.schedule.as_deref()),
            self.repository_status
                .as_ref()
                .and_then(|s| s.last_attempt_at.as_deref()),
            chrono::Utc::now(),
        );

        // Load git info
        if let Ok(repo) = GitRepo::open(&self.repo_path) {
            self.current_branch = repo.current_branch().unwrap_or_else(|_| "unknown".into());
            self.changes = repo.changes_summary().unwrap_or_default();
        }

        // Load log entries
        {
            let logger = FileLogger::read_only(&self.repo_path);
            if let Ok(lines) = logger.read_entries(100, 0) {
                self.log_lines = lines.iter().filter_map(|l| LogEntry::parse(l)).collect();
            }
        }

        self.providers.clear();
        if provider_keys.is_empty() {
            return;
        }
        // Check agent availability only when commit messages use AI.
        let chain = commitbook_engine::ai::ProviderChain::new();
        self.providers = chain.check_availability(&provider_keys);
    }

    pub fn handle_key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        if self.preview.is_some() {
            match code {
                KeyCode::Esc => {
                    self.preview = None;
                    self.preview_scroll = 0;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.preview_scroll = self.preview_scroll.saturating_sub(1)
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.preview_scroll = (self.preview_scroll + 1).min(
                        self.preview
                            .as_ref()
                            .map_or(0, |p| p.entries.len() + p.blockers.len() + 8),
                    )
                }
                KeyCode::Char('r') => self.refresh(),
                KeyCode::Char('q') => self.quit = true,
                _ => (),
            }
            return;
        }
        match code {
            KeyCode::Char('p') => {
                self.preview = Some(commitbook_engine::inspection::preview(&self.repo_path));
                self.preview_scroll = 0;
            }
            KeyCode::Up | KeyCode::Char('k') if self.active_panel == Panel::Status => {
                self.status_scroll = self.status_scroll.saturating_sub(1)
            }
            KeyCode::Down | KeyCode::Char('j') if self.active_panel == Panel::Status => {
                self.status_scroll = (self.status_scroll + 1).min(30)
            }
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
            settings::start_scheduler(&self.repo_path, &context)
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
