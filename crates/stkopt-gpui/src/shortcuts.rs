//! Keyboard shortcuts and key binding utilities.

/// Application keyboard shortcuts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shortcut {
    /// Open settings (Cmd+, on macOS, Ctrl+, on Linux/Windows).
    OpenSettings,
    /// Navigate to Dashboard (Cmd+1 / Ctrl+1).
    GoToDashboard,
    /// Navigate to Account (Cmd+2 / Ctrl+2).
    GoToAccount,
    /// Navigate to Validators (Cmd+3 / Ctrl+3).
    GoToValidators,
    /// Navigate to Optimization (Cmd+4 / Ctrl+4).
    GoToOptimization,
    /// Navigate to Pools (Cmd+5 / Ctrl+5).
    GoToPools,
    /// Navigate to History (Cmd+6 / Ctrl+6).
    GoToHistory,
    /// Refresh data (Cmd+R / Ctrl+R).
    Refresh,
    /// Close settings / Go back (Escape).
    Close,
}

impl Shortcut {
    /// Get the display string for this shortcut.
    pub fn display(&self) -> &'static str {
        #[cfg(target_os = "macos")]
        {
            match self {
                Shortcut::OpenSettings => "⌘,",
                Shortcut::GoToDashboard => "⌘1",
                Shortcut::GoToAccount => "⌘2",
                Shortcut::GoToValidators => "⌘3",
                Shortcut::GoToOptimization => "⌘4",
                Shortcut::GoToPools => "⌘5",
                Shortcut::GoToHistory => "⌘6",
                Shortcut::Refresh => "⌘R",
                Shortcut::Close => "Esc",
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            match self {
                Shortcut::OpenSettings => "Ctrl+,",
                Shortcut::GoToDashboard => "Ctrl+1",
                Shortcut::GoToAccount => "Ctrl+2",
                Shortcut::GoToValidators => "Ctrl+3",
                Shortcut::GoToOptimization => "Ctrl+4",
                Shortcut::GoToPools => "Ctrl+5",
                Shortcut::GoToHistory => "Ctrl+6",
                Shortcut::Refresh => "Ctrl+R",
                Shortcut::Close => "Esc",
            }
        }
    }

    /// Get the label for this shortcut.
    pub fn label(&self) -> &'static str {
        match self {
            Shortcut::OpenSettings => "Open Settings",
            Shortcut::GoToDashboard => "Go to Dashboard",
            Shortcut::GoToAccount => "Go to Account",
            Shortcut::GoToValidators => "Go to Validators",
            Shortcut::GoToOptimization => "Go to Optimization",
            Shortcut::GoToPools => "Go to Pools",
            Shortcut::GoToHistory => "Go to History",
            Shortcut::Refresh => "Refresh",
            Shortcut::Close => "Close / Go Back",
        }
    }

    /// Get all shortcuts.
    pub fn all() -> &'static [Shortcut] {
        &[
            Shortcut::OpenSettings,
            Shortcut::GoToDashboard,
            Shortcut::GoToAccount,
            Shortcut::GoToValidators,
            Shortcut::GoToOptimization,
            Shortcut::GoToPools,
            Shortcut::GoToHistory,
            Shortcut::Refresh,
            Shortcut::Close,
        ]
    }
}

/// Check if the current platform uses Cmd (macOS) or Ctrl (others).
pub fn platform_modifier() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "cmd"
    }
    #[cfg(not(target_os = "macos"))]
    {
        "ctrl"
    }
}

/// Get the keystroke string for settings shortcut.
pub fn settings_keystroke() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "cmd-,"
    }
    #[cfg(not(target_os = "macos"))]
    {
        "ctrl-,"
    }
}

/// Shortcut category for grouping in settings display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutCategory {
    General,
    Navigation,
}

impl ShortcutCategory {
    pub fn label(&self) -> &'static str {
        match self {
            ShortcutCategory::General => "General",
            ShortcutCategory::Navigation => "Navigation",
        }
    }
}

impl Shortcut {
    /// Get the category for this shortcut.
    pub fn category(&self) -> ShortcutCategory {
        match self {
            Shortcut::OpenSettings | Shortcut::Refresh | Shortcut::Close => {
                ShortcutCategory::General
            }
            _ => ShortcutCategory::Navigation,
        }
    }
}

/// Get shortcuts grouped by category.
pub fn shortcuts_by_category() -> Vec<(ShortcutCategory, Vec<Shortcut>)> {
    vec![
        (
            ShortcutCategory::General,
            vec![Shortcut::OpenSettings, Shortcut::Refresh, Shortcut::Close],
        ),
        (
            ShortcutCategory::Navigation,
            vec![
                Shortcut::GoToDashboard,
                Shortcut::GoToAccount,
                Shortcut::GoToValidators,
                Shortcut::GoToOptimization,
                Shortcut::GoToPools,
                Shortcut::GoToHistory,
            ],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shortcut_display_not_empty() {
        for shortcut in Shortcut::all() {
            assert!(!shortcut.display().is_empty());
        }
    }

    #[test]
    fn test_shortcut_label_not_empty() {
        for shortcut in Shortcut::all() {
            assert!(!shortcut.label().is_empty());
        }
    }

    #[test]
    fn test_shortcut_all_count() {
        assert_eq!(Shortcut::all().len(), 9);
    }

    #[test]
    fn test_platform_modifier() {
        let modifier = platform_modifier();
        assert!(modifier == "cmd" || modifier == "ctrl");
    }

    #[test]
    fn test_settings_keystroke() {
        let keystroke = settings_keystroke();
        assert!(keystroke.contains(","));
    }

    #[test]
    fn test_shortcuts_by_category() {
        let grouped = shortcuts_by_category();
        assert_eq!(grouped.len(), 2);

        let total: usize = grouped.iter().map(|(_, v)| v.len()).sum();
        assert_eq!(total, Shortcut::all().len());
    }

    #[test]
    fn test_shortcut_category_labels() {
        assert_eq!(ShortcutCategory::General.label(), "General");
        assert_eq!(ShortcutCategory::Navigation.label(), "Navigation");
    }
}
