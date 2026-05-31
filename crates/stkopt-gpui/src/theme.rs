//! GPUI theme bridge for persisted app settings.

use gpui::{App, BorrowAppContext, Rgba};
use gpui_px::ChartTheme;
use gpui_ui_kit::{ButtonTheme, Theme, ThemeState, ThemeVariant, with_alpha};

use crate::persistence::ThemeConfig;

pub fn theme_variant_for_config(config: ThemeConfig) -> ThemeVariant {
    match config {
        ThemeConfig::System => ThemeVariant::Dark,
        ThemeConfig::Light => ThemeVariant::Light,
        ThemeConfig::Dark => ThemeVariant::Dark,
        ThemeConfig::Midnight => ThemeVariant::Midnight,
        ThemeConfig::Forest => ThemeVariant::Forest,
        ThemeConfig::BlackAndWhite => ThemeVariant::BlackAndWhite,
    }
}

pub fn apply_theme_config(config: ThemeConfig, cx: &mut App) {
    let variant = theme_variant_for_config(config);
    cx.update_global::<ThemeState, _>(|state, _cx| {
        state.set_variant(variant);
    });
    cx.refresh_windows();
}

pub fn theme_config_value(config: ThemeConfig) -> &'static str {
    match config {
        ThemeConfig::System => "system",
        ThemeConfig::Light => "light",
        ThemeConfig::Dark => "dark",
        ThemeConfig::Midnight => "midnight",
        ThemeConfig::Forest => "forest",
        ThemeConfig::BlackAndWhite => "black-and-white",
    }
}

pub fn theme_config_from_value(value: &str) -> Option<ThemeConfig> {
    match value {
        "system" => Some(ThemeConfig::System),
        "light" => Some(ThemeConfig::Light),
        "dark" => Some(ThemeConfig::Dark),
        "midnight" => Some(ThemeConfig::Midnight),
        "forest" => Some(ThemeConfig::Forest),
        "black-and-white" => Some(ThemeConfig::BlackAndWhite),
        _ => None,
    }
}

pub fn chart_theme_for_ui_theme(theme: &Theme) -> ChartTheme {
    ChartTheme {
        plot_background: theme.surface,
        grid_color: with_alpha(theme.text_muted, 0.24),
        axis_line_color: with_alpha(theme.text_muted, 0.45),
        axis_label_color: theme.text_secondary,
        title_color: theme.text_primary,
        legend_text_color: theme.text_secondary,
    }
}

pub fn button_theme_for_ui_theme(theme: &Theme) -> ButtonTheme {
    let mut button_theme = ButtonTheme::from(theme);
    button_theme.text_on_accent = text_color_on(theme.accent, theme);
    button_theme
}

pub fn rgb_hex_from_theme_color(color: Rgba) -> u32 {
    (to_byte(color.r) << 16) | (to_byte(color.g) << 8) | to_byte(color.b)
}

pub fn text_color_on(background: Rgba, theme: &Theme) -> Rgba {
    if contrast_ratio(background, theme.text_primary)
        >= contrast_ratio(background, theme.background)
    {
        theme.text_primary
    } else {
        theme.background
    }
}

fn to_byte(value: f32) -> u32 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u32
}

fn contrast_ratio(a: Rgba, b: Rgba) -> f32 {
    let lighter = relative_luminance(a).max(relative_luminance(b));
    let darker = relative_luminance(a).min(relative_luminance(b));
    (lighter + 0.05) / (darker + 0.05)
}

fn relative_luminance(color: Rgba) -> f32 {
    fn channel(value: f32) -> f32 {
        if value <= 0.03928 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    }

    0.2126 * channel(color.r) + 0.7152 * channel(color.g) + 0.0722 * channel(color.b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_ui_kit::Theme;

    #[test]
    fn maps_persisted_theme_to_gpui_variant() {
        assert_eq!(
            theme_variant_for_config(ThemeConfig::Light),
            ThemeVariant::Light
        );
        assert_eq!(
            theme_variant_for_config(ThemeConfig::BlackAndWhite),
            ThemeVariant::BlackAndWhite
        );
    }

    #[test]
    fn round_trips_theme_button_values() {
        for theme in [
            ThemeConfig::System,
            ThemeConfig::Light,
            ThemeConfig::Dark,
            ThemeConfig::Midnight,
            ThemeConfig::Forest,
            ThemeConfig::BlackAndWhite,
        ] {
            assert_eq!(
                theme_config_from_value(theme_config_value(theme)),
                Some(theme)
            );
        }
    }

    #[test]
    fn chart_theme_uses_ui_theme_tokens() {
        let theme = Theme::light();
        let chart_theme = chart_theme_for_ui_theme(&theme);

        assert_eq!(chart_theme.plot_background, theme.surface);
        assert_eq!(chart_theme.title_color, theme.text_primary);
        assert_eq!(chart_theme.legend_text_color, theme.text_secondary);
    }

    #[test]
    fn selected_text_prefers_best_theme_contrast() {
        let light = Theme::light();
        assert_eq!(text_color_on(light.accent, &light), light.background);

        let monochrome = Theme::black_and_white();
        assert_eq!(
            text_color_on(monochrome.accent, &monochrome),
            monochrome.text_primary
        );
    }

    #[test]
    fn button_theme_uses_contrasting_accent_text() {
        let light = Theme::light();
        let button_theme = button_theme_for_ui_theme(&light);
        assert_eq!(
            button_theme.text_on_accent,
            text_color_on(light.accent, &light)
        );
    }
}
