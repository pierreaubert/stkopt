//! Error handling and display utilities for the GPUI app.

use std::fmt;

/// Application error types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppError {
    /// Network connection error.
    Connection(String),
    /// Invalid address format.
    InvalidAddress(String),
    /// Chain query failed.
    ChainQuery(String),
    /// Transaction building failed.
    TransactionBuild(String),
    /// File I/O error.
    FileIO(String),
    /// Configuration error.
    Config(String),
    /// Validation error.
    Validation(String),
    /// Unknown error.
    Unknown(String),
}

impl AppError {
    /// Get the error category for display.
    pub fn category(&self) -> &'static str {
        match self {
            AppError::Connection(_) => "Connection Error",
            AppError::InvalidAddress(_) => "Invalid Address",
            AppError::ChainQuery(_) => "Chain Query Error",
            AppError::TransactionBuild(_) => "Transaction Error",
            AppError::FileIO(_) => "File Error",
            AppError::Config(_) => "Configuration Error",
            AppError::Validation(_) => "Validation Error",
            AppError::Unknown(_) => "Error",
        }
    }

    /// Get the error message.
    pub fn message(&self) -> &str {
        match self {
            AppError::Connection(msg)
            | AppError::InvalidAddress(msg)
            | AppError::ChainQuery(msg)
            | AppError::TransactionBuild(msg)
            | AppError::FileIO(msg)
            | AppError::Config(msg)
            | AppError::Validation(msg)
            | AppError::Unknown(msg) => msg,
        }
    }

    /// Check if this is a recoverable error.
    pub fn is_recoverable(&self) -> bool {
        match self {
            AppError::Connection(_) => true,
            AppError::InvalidAddress(_) => true,
            AppError::ChainQuery(_) => true,
            AppError::TransactionBuild(_) => true,
            AppError::FileIO(_) => false,
            AppError::Config(_) => false,
            AppError::Validation(_) => true,
            AppError::Unknown(_) => false,
        }
    }

    /// Get a suggested action for the user.
    pub fn suggestion(&self) -> &'static str {
        match self {
            AppError::Connection(_) => "Check your internet connection and try again.",
            AppError::InvalidAddress(_) => "Please enter a valid SS58 address.",
            AppError::ChainQuery(_) => "The chain may be temporarily unavailable. Try again later.",
            AppError::TransactionBuild(_) => "Check your transaction parameters and try again.",
            AppError::FileIO(_) => "Check file permissions and disk space.",
            AppError::Config(_) => "Reset configuration to defaults.",
            AppError::Validation(_) => "Check your input and try again.",
            AppError::Unknown(_) => "Please try again or restart the application.",
        }
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.category(), self.message())
    }
}

impl std::error::Error for AppError {}

/// Error severity level for display styling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorSeverity {
    /// Informational message.
    Info,
    /// Warning that doesn't block operation.
    Warning,
    /// Error that blocks the current operation.
    Error,
    /// Critical error requiring immediate attention.
    Critical,
}

impl ErrorSeverity {
    /// Get the label for this severity.
    pub fn label(&self) -> &'static str {
        match self {
            ErrorSeverity::Info => "Info",
            ErrorSeverity::Warning => "Warning",
            ErrorSeverity::Error => "Error",
            ErrorSeverity::Critical => "Critical",
        }
    }

    /// Get the icon for this severity.
    pub fn icon(&self) -> &'static str {
        match self {
            ErrorSeverity::Info => "ℹ️",
            ErrorSeverity::Warning => "⚠️",
            ErrorSeverity::Error => "❌",
            ErrorSeverity::Critical => "🚨",
        }
    }
}

/// A displayable notification/toast message.
#[derive(Debug, Clone)]
pub struct Notification {
    pub severity: ErrorSeverity,
    pub title: String,
    pub message: String,
    pub dismissible: bool,
}

impl Notification {
    /// Create an info notification.
    pub fn info(title: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: ErrorSeverity::Info,
            title: title.into(),
            message: message.into(),
            dismissible: true,
        }
    }

    /// Create a warning notification.
    pub fn warning(title: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: ErrorSeverity::Warning,
            title: title.into(),
            message: message.into(),
            dismissible: true,
        }
    }

    /// Create an error notification.
    pub fn error(title: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: ErrorSeverity::Error,
            title: title.into(),
            message: message.into(),
            dismissible: true,
        }
    }

    /// Create a critical notification.
    pub fn critical(title: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: ErrorSeverity::Critical,
            title: title.into(),
            message: message.into(),
            dismissible: false,
        }
    }

    /// Create from an AppError.
    pub fn from_error(error: &AppError) -> Self {
        Self {
            severity: if error.is_recoverable() {
                ErrorSeverity::Error
            } else {
                ErrorSeverity::Critical
            },
            title: error.category().to_string(),
            message: error.message().to_string(),
            dismissible: error.is_recoverable(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_error_category() {
        let err = AppError::Connection("timeout".to_string());
        assert_eq!(err.category(), "Connection Error");
    }

    #[test]
    fn test_app_error_message() {
        let err = AppError::InvalidAddress("bad format".to_string());
        assert_eq!(err.message(), "bad format");
    }

    #[test]
    fn test_app_error_is_recoverable() {
        assert!(AppError::Connection("test".to_string()).is_recoverable());
        assert!(!AppError::FileIO("test".to_string()).is_recoverable());
    }

    #[test]
    fn test_app_error_suggestion() {
        let err = AppError::InvalidAddress("test".to_string());
        assert!(err.suggestion().contains("SS58"));
    }

    #[test]
    fn test_app_error_display() {
        let err = AppError::Validation("missing field".to_string());
        let display = format!("{}", err);
        assert!(display.contains("Validation Error"));
        assert!(display.contains("missing field"));
    }

    #[test]
    fn test_error_severity_labels() {
        assert_eq!(ErrorSeverity::Info.label(), "Info");
        assert_eq!(ErrorSeverity::Warning.label(), "Warning");
        assert_eq!(ErrorSeverity::Error.label(), "Error");
        assert_eq!(ErrorSeverity::Critical.label(), "Critical");
    }

    #[test]
    fn test_error_severity_icons() {
        assert!(!ErrorSeverity::Info.icon().is_empty());
        assert!(!ErrorSeverity::Warning.icon().is_empty());
        assert!(!ErrorSeverity::Error.icon().is_empty());
        assert!(!ErrorSeverity::Critical.icon().is_empty());
    }

    #[test]
    fn test_notification_info() {
        let n = Notification::info("Title", "Message");
        assert_eq!(n.severity, ErrorSeverity::Info);
        assert!(n.dismissible);
    }

    #[test]
    fn test_notification_critical() {
        let n = Notification::critical("Title", "Message");
        assert_eq!(n.severity, ErrorSeverity::Critical);
        assert!(!n.dismissible);
    }

    #[test]
    fn test_notification_from_error() {
        let err = AppError::Connection("timeout".to_string());
        let n = Notification::from_error(&err);
        assert_eq!(n.title, "Connection Error");
        assert!(n.dismissible);
    }

    #[test]
    fn test_notification_from_critical_error() {
        let err = AppError::FileIO("permission denied".to_string());
        let n = Notification::from_error(&err);
        assert_eq!(n.severity, ErrorSeverity::Critical);
        assert!(!n.dismissible);
    }
}
