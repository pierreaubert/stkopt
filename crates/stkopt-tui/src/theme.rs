//! Theme support with auto-detection for dark/light terminals.

use ratatui::style::Color;

/// Application theme (dark or light).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Theme {
    #[default]
    Dark,
    Light,
}

impl Theme {
    /// Detect the terminal theme based on background luminance.
    pub fn detect() -> Self {
        match terminal_light::luma() {
            Ok(luma) if luma > 0.5 => {
                tracing::info!("Detected light terminal (luma: {:.2})", luma);
                Theme::Light
            }
            Ok(luma) => {
                tracing::info!("Detected dark terminal (luma: {:.2})", luma);
                Theme::Dark
            }
            Err(e) => {
                tracing::debug!("Could not detect terminal theme: {}, defaulting to dark", e);
                Theme::Dark
            }
        }
    }

    /// Get the color palette for this theme.
    pub fn palette(&self) -> Palette {
        match self {
            Theme::Dark => Palette::dark(),
            Theme::Light => Palette::light(),
        }
    }
}

/// Color palette for the application.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    // Base colors
    pub fg: Color,
    pub fg_dim: Color,
    pub bg: Color,
    pub border: Color,

    // Accent colors
    pub primary: Color,
    pub secondary: Color,
    pub accent: Color,

    // Status colors
    pub success: Color,
    pub warning: Color,
    pub error: Color,

    // Data visualization
    pub graph_high: Color,
    pub graph_mid: Color,
    pub graph_low: Color,

    // UI elements
    pub selection: Color,
    pub highlight: Color,
    pub muted: Color,

    // Tab colors
    pub tab_active: Color,
    pub tab_inactive: Color,
}

impl Palette {
    /// Dark theme palette (for dark terminal backgrounds).
    pub fn dark() -> Self {
        Self {
            // Base colors
            fg: Color::White,
            fg_dim: Color::Gray,
            bg: Color::Reset, // Use terminal's background
            border: Color::DarkGray,

            // Accent colors
            primary: Color::Cyan,
            secondary: Color::Blue,
            accent: Color::Magenta,

            // Status colors
            success: Color::Green,
            warning: Color::Yellow,
            error: Color::Red,

            // Data visualization
            graph_high: Color::Green,
            graph_mid: Color::Yellow,
            graph_low: Color::Red,

            // UI elements
            selection: Color::LightBlue,
            highlight: Color::Yellow,
            muted: Color::DarkGray,

            // Tab colors
            tab_active: Color::Cyan,
            tab_inactive: Color::DarkGray,
        }
    }

    /// Light theme palette (for light terminal backgrounds).
    pub fn light() -> Self {
        Self {
            // Base colors - darker colors for light backgrounds
            fg: Color::Black,
            fg_dim: Color::DarkGray,
            bg: Color::Reset, // Use terminal's background
            border: Color::Gray,

            // Accent colors - more saturated for visibility
            primary: Color::Rgb(0, 128, 128), // Teal
            secondary: Color::Rgb(0, 0, 139), // Dark blue
            accent: Color::Rgb(128, 0, 128),  // Purple

            // Status colors - darker variants
            success: Color::Rgb(0, 128, 0),    // Dark green
            warning: Color::Rgb(184, 134, 11), // Dark goldenrod
            error: Color::Rgb(178, 34, 34),    // Firebrick

            // Data visualization
            graph_high: Color::Rgb(0, 128, 0),   // Dark green
            graph_mid: Color::Rgb(184, 134, 11), // Dark goldenrod
            graph_low: Color::Rgb(178, 34, 34),  // Firebrick

            // UI elements
            selection: Color::Rgb(70, 130, 180), // Steel blue
            highlight: Color::Rgb(184, 134, 11), // Dark goldenrod
            muted: Color::Gray,

            // Tab colors
            tab_active: Color::Rgb(0, 128, 128), // Teal
            tab_inactive: Color::Gray,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_theme_default_is_dark() {
        assert_eq!(Theme::default(), Theme::Dark);
    }

    #[test]
    fn test_theme_equality() {
        assert_eq!(Theme::Dark, Theme::Dark);
        assert_eq!(Theme::Light, Theme::Light);
        assert_ne!(Theme::Dark, Theme::Light);
    }

    #[test]
    fn test_theme_clone() {
        let theme = Theme::Dark;
        let theme_clone = theme;
        assert_eq!(theme, theme_clone);
    }

    #[test]
    fn test_dark_palette() {
        let palette = Theme::Dark.palette();
        assert_eq!(palette.fg, Color::White);
        assert_eq!(palette.primary, Color::Cyan);
    }

    #[test]
    fn test_light_palette() {
        let palette = Theme::Light.palette();
        assert_eq!(palette.fg, Color::Black);
    }

    #[test]
    fn test_palettes_have_different_fg() {
        let dark = Palette::dark();
        let light = Palette::light();
        assert_ne!(dark.fg, light.fg);
    }

    #[test]
    fn test_dark_palette_all_colors() {
        let p = Palette::dark();
        // Verify all colors are set
        assert_eq!(p.fg, Color::White);
        assert_eq!(p.fg_dim, Color::Gray);
        assert_eq!(p.bg, Color::Reset);
        assert_eq!(p.border, Color::DarkGray);
        assert_eq!(p.primary, Color::Cyan);
        assert_eq!(p.secondary, Color::Blue);
        assert_eq!(p.accent, Color::Magenta);
        assert_eq!(p.success, Color::Green);
        assert_eq!(p.warning, Color::Yellow);
        assert_eq!(p.error, Color::Red);
        assert_eq!(p.graph_high, Color::Green);
        assert_eq!(p.graph_mid, Color::Yellow);
        assert_eq!(p.graph_low, Color::Red);
        assert_eq!(p.selection, Color::LightBlue);
        assert_eq!(p.highlight, Color::Yellow);
        assert_eq!(p.muted, Color::DarkGray);
        assert_eq!(p.tab_active, Color::Cyan);
        assert_eq!(p.tab_inactive, Color::DarkGray);
    }

    #[test]
    fn test_light_palette_all_colors() {
        let p = Palette::light();
        // Verify all colors are set
        assert_eq!(p.fg, Color::Black);
        assert_eq!(p.fg_dim, Color::DarkGray);
        assert_eq!(p.bg, Color::Reset);
        assert_eq!(p.border, Color::Gray);
        // RGB colors for light theme
        assert_eq!(p.primary, Color::Rgb(0, 128, 128));
        assert_eq!(p.secondary, Color::Rgb(0, 0, 139));
        assert_eq!(p.accent, Color::Rgb(128, 0, 128));
        assert_eq!(p.success, Color::Rgb(0, 128, 0));
        assert_eq!(p.warning, Color::Rgb(184, 134, 11));
        assert_eq!(p.error, Color::Rgb(178, 34, 34));
    }

    #[test]
    fn test_palette_clone() {
        let p = Palette::dark();
        let p_clone = p;
        assert_eq!(p.fg, p_clone.fg);
        assert_eq!(p.primary, p_clone.primary);
    }

    #[test]
    fn test_theme_detect() {
        // Just verify detect() doesn't panic
        let _theme = Theme::detect();
    }

    #[test]
    fn test_theme_palette_consistency() {
        // Dark theme should have light foreground
        let dark_palette = Theme::Dark.palette();
        assert_eq!(dark_palette.fg, Color::White);

        // Light theme should have dark foreground
        let light_palette = Theme::Light.palette();
        assert_eq!(light_palette.fg, Color::Black);
    }

    #[test]
    fn test_palette_status_colors_distinct() {
        let p = Palette::dark();
        // Success, warning, and error should be different
        assert_ne!(p.success, p.warning);
        assert_ne!(p.warning, p.error);
        assert_ne!(p.success, p.error);
    }

    #[test]
    fn test_palette_graph_colors_distinct() {
        let p = Palette::dark();
        // Graph colors should represent different severity
        assert_ne!(p.graph_high, p.graph_mid);
        assert_ne!(p.graph_mid, p.graph_low);
    }
}
