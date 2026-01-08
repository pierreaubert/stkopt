//! Shared log buffer for capturing tracing events to display in the TUI.

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
                tracing::error!("Log buffer mutex poisoned, dropping log line");
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

    /// Get the number of log lines.
    pub fn len(&self) -> usize {
        match self.inner.lock() {
            Ok(buffer) => buffer.len(),
            Err(_) => 0,
        }
    }

    /// Check if buffer is empty.
    #[allow(dead_code)]
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

        self.buffer.push(LogLine {
            level,
            target,
            message,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_level_as_str() {
        assert_eq!(LogLevel::Trace.as_str(), "TRACE");
        assert_eq!(LogLevel::Debug.as_str(), "DEBUG");
        assert_eq!(LogLevel::Info.as_str(), "INFO");
        assert_eq!(LogLevel::Warn.as_str(), "WARN");
        assert_eq!(LogLevel::Error.as_str(), "ERROR");
    }

    #[test]
    fn test_log_level_equality() {
        assert_eq!(LogLevel::Info, LogLevel::Info);
        assert_ne!(LogLevel::Info, LogLevel::Error);
    }

    #[test]
    fn test_log_level_clone() {
        let level = LogLevel::Warn;
        let level_clone = level;
        assert_eq!(level, level_clone);
    }

    #[test]
    fn test_log_buffer_new() {
        let buffer = LogBuffer::new();
        assert!(buffer.is_empty());
        assert_eq!(buffer.len(), 0);
    }

    #[test]
    fn test_log_buffer_default() {
        let buffer = LogBuffer::default();
        assert!(buffer.is_empty());
    }

    #[test]
    fn test_log_buffer_push_and_get() {
        let buffer = LogBuffer::new();

        buffer.push(LogLine {
            level: LogLevel::Info,
            target: "test".to_string(),
            message: "Hello".to_string(),
        });

        assert_eq!(buffer.len(), 1);
        assert!(!buffer.is_empty());

        let lines = buffer.get_lines();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].message, "Hello");
        assert_eq!(lines[0].target, "test");
        assert_eq!(lines[0].level, LogLevel::Info);
    }

    #[test]
    fn test_log_buffer_multiple_entries() {
        let buffer = LogBuffer::new();

        for i in 0..10 {
            buffer.push(LogLine {
                level: LogLevel::Debug,
                target: "test".to_string(),
                message: format!("Message {}", i),
            });
        }

        assert_eq!(buffer.len(), 10);
        let lines = buffer.get_lines();
        assert_eq!(lines[0].message, "Message 0");
        assert_eq!(lines[9].message, "Message 9");
    }

    #[test]
    fn test_log_buffer_capacity_limit() {
        let buffer = LogBuffer::new();

        // Push more than MAX_LOG_LINES entries
        for i in 0..MAX_LOG_LINES + 100 {
            buffer.push(LogLine {
                level: LogLevel::Info,
                target: "test".to_string(),
                message: format!("Message {}", i),
            });
        }

        // Should be capped at MAX_LOG_LINES
        assert_eq!(buffer.len(), MAX_LOG_LINES);

        // First entry should be the 100th one (0-99 were removed)
        let lines = buffer.get_lines();
        assert_eq!(lines[0].message, "Message 100");
    }

    #[test]
    fn test_log_buffer_clone() {
        let buffer = LogBuffer::new();
        buffer.push(LogLine {
            level: LogLevel::Error,
            target: "test".to_string(),
            message: "Error!".to_string(),
        });

        let buffer_clone = buffer.clone();

        // Both should have the same content
        assert_eq!(buffer.len(), buffer_clone.len());

        // But they share the same Arc, so adding to one affects both
        buffer.push(LogLine {
            level: LogLevel::Info,
            target: "test".to_string(),
            message: "New".to_string(),
        });
        assert_eq!(buffer_clone.len(), 2);
    }

    #[test]
    fn test_log_line_clone() {
        let line = LogLine {
            level: LogLevel::Warn,
            target: "my_target".to_string(),
            message: "Warning message".to_string(),
        };
        let line_clone = line.clone();

        assert_eq!(line.level, line_clone.level);
        assert_eq!(line.target, line_clone.target);
        assert_eq!(line.message, line_clone.message);
    }

    #[test]
    fn test_log_buffer_layer_new() {
        let buffer = LogBuffer::new();
        let _layer = LogBufferLayer::new(buffer);
        // Just verify it can be created
    }

    #[test]
    fn test_message_visitor_record_debug() {
        let visitor = MessageVisitor::default();

        // Create a mock field - we can't easily test this without the tracing internals
        // but we can verify the default state
        assert!(visitor.message.is_none());
    }

    #[test]
    fn test_log_buffer_thread_safety() {
        use std::thread;

        let buffer = LogBuffer::new();
        let buffer_clone = buffer.clone();

        let handle = thread::spawn(move || {
            for i in 0..100 {
                buffer_clone.push(LogLine {
                    level: LogLevel::Debug,
                    target: "thread".to_string(),
                    message: format!("Thread message {}", i),
                });
            }
        });

        // Push from main thread too
        for i in 0..100 {
            buffer.push(LogLine {
                level: LogLevel::Info,
                target: "main".to_string(),
                message: format!("Main message {}", i),
            });
        }

        handle.join().unwrap();

        // Should have all 200 messages
        assert_eq!(buffer.len(), 200);
    }
}
