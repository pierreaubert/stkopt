//! Shared log buffer for capturing tracing events to display in the UI.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tracing_subscriber::Layer;

/// Maximum number of log lines to keep.
const MAX_LOG_LINES: usize = 2000;

/// A log line with level and message.
#[derive(Debug, Clone)]
pub struct LogLine {
    pub level: LogLevel,
    pub target: String,
    pub message: String,
    pub timestamp: chrono::DateTime<chrono::Local>,
}

/// Log level for display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            LogLevel::Trace => "TRACE",
            LogLevel::Debug => "DEBUG",
            LogLevel::Info => "INFO",
            LogLevel::Warn => "WARN",
            LogLevel::Error => "ERROR",
        }
    }
}

/// Shared buffer for storing log lines.
#[derive(Debug, Clone)]
pub struct LogBuffer {
    inner: Arc<Mutex<VecDeque<LogLine>>>,
}

impl Default for LogBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl LogBuffer {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(VecDeque::with_capacity(MAX_LOG_LINES))),
        }
    }

    /// Push a new log line, removing oldest if at capacity.
    pub fn push(&self, line: LogLine) {
        match self.inner.lock() {
            Ok(mut buffer) => {
                if buffer.len() >= MAX_LOG_LINES {
                    buffer.pop_front();
                }
                buffer.push_back(line);
            }
            Err(_) => {
                // tracing::error!("Log buffer mutex poisoned, dropping log line");
                // Don't log to tracing here to avoid recursion loop
            }
        }
    }

    /// Get all log lines.
    pub fn get_lines(&self) -> Vec<LogLine> {
        match self.inner.lock() {
            Ok(buffer) => buffer.iter().cloned().collect(),
            Err(_) => Vec::new(),
        }
    }

    /// Appends lines to the generic buffer if provided.
    /// This optimization avoids full clone if we render incrementally,
    /// but for now we follow TUI approach.
    pub fn len(&self) -> usize {
        match self.inner.lock() {
            Ok(buffer) => buffer.len(),
            Err(_) => 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A tracing layer that writes to our LogBuffer.
pub struct LogBufferLayer {
    buffer: LogBuffer,
}

impl LogBufferLayer {
    pub fn new(buffer: LogBuffer) -> Self {
        Self { buffer }
    }
}

impl<S> Layer<S> for LogBufferLayer
where
    S: tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let metadata = event.metadata();

        let level = match *metadata.level() {
            tracing::Level::TRACE => LogLevel::Trace,
            tracing::Level::DEBUG => LogLevel::Debug,
            tracing::Level::INFO => LogLevel::Info,
            tracing::Level::WARN => LogLevel::Warn,
            tracing::Level::ERROR => LogLevel::Error,
        };

        // Extract the message from the event
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);

        let target = metadata.target().to_string();
        let message = visitor.message.unwrap_or_default();
        let timestamp = chrono::Local::now();

        self.buffer.push(LogLine {
            level,
            target,
            message,
            timestamp,
        });
    }
}

/// Visitor to extract message field from tracing events.
#[derive(Default)]
struct MessageVisitor {
    message: Option<String>,
}

impl tracing::field::Visit for MessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = Some(format!("{:?}", value));
        } else if self.message.is_none() {
            // Capture the first field as message if no explicit message field
            self.message = Some(format!("{:?}", value));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" || self.message.is_none() {
            self.message = Some(value.to_string());
        }
    }
}
