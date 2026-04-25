use anyhow::Result;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            LogLevel::Debug => "DEBUG",
            LogLevel::Info => "INFO",
            LogLevel::Warn => "WARN",
            LogLevel::Error => "ERROR",
        }
    }
}

/// Sink for activity log entries.
///
/// The sync pipeline only calls `info`, `warn`, and `error`. Desktop wiring
/// uses the file-backed `FileLogger`; mobile hosts supply a callback-backed
/// implementation across the FFI boundary.
pub trait Logger: Send + Sync {
    fn emit(&self, level: LogLevel, message: &str) -> Result<()>;

    fn info(&self, message: &str) -> Result<()> {
        self.emit(LogLevel::Info, message)
    }
    fn warn(&self, message: &str) -> Result<()> {
        self.emit(LogLevel::Warn, message)
    }
    fn error(&self, message: &str) -> Result<()> {
        self.emit(LogLevel::Error, message)
    }
    fn debug(&self, message: &str) -> Result<()> {
        self.emit(LogLevel::Debug, message)
    }
}
