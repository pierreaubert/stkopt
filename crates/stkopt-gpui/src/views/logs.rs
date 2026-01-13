//! Log viewer component.

use gpui::Styled; // Explicit import to ensure traits are visible
use gpui::prelude::*;
use gpui::*;
use gpui_ui_kit::theme::ThemeExt;
use gpui_ui_kit::*;

use crate::log::{LogBuffer, LogLevel};

pub struct LogsView;

impl LogsView {
    pub fn render(buffer: &LogBuffer, cx: &Context<crate::app::StkoptApp>) -> impl IntoElement {
        let theme = cx.theme();
        let lines = buffer.get_lines();

        gpui::div()
            .id("logs-scroll")
            .flex()
            .flex_col()
            .gap_1()
            .p_4()
            .h_full()
            .overflow_y_scroll()
            .bg(theme.surface)
            .border_t_1()
            .border_color(theme.border)
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .pb_2()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(Heading::h3("Application Logs"))
                    .child(
                        Text::new(format!("{} events", lines.len()))
                            .size(TextSize::Xs)
                            .color(theme.text_secondary),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .pt_2()
                    .font_family("JetBrains Mono") // Monospace if available, or system mono
                    .children(lines.into_iter().rev().map(|line| {
                        // Render logs in reverse order (newest first) or scroll to bottom?
                        // TUI usually does oldest to newest. Let's do newest first for easy viewing without auto-scroll logic.
                        // Actually console usually appends at bottom.
                        // Let's stick to natural order (oldest first) but user has to scroll.
                        // Wait, reverse is often better for "tail".
                        // Let's do newest atop for now as it's easier to see new stuff.

                        let color = match line.level {
                            LogLevel::Trace => theme.text_secondary,
                            LogLevel::Debug => theme.text_primary,
                            LogLevel::Info => theme.accent,
                            LogLevel::Warn => theme.warning,
                            LogLevel::Error => theme.error,
                        };

                        div()
                            .flex()
                            .items_start()
                            .gap_2()
                            .text_xs()
                            .child(
                                Text::new(line.timestamp.format("%H:%M:%S").to_string())
                                    .color(theme.text_secondary)
                                    .weight(TextWeight::Light),
                            )
                            .child(
                                div().w(px(35.0)).child(
                                    Text::new(line.level.as_str())
                                        .color(color)
                                        .weight(TextWeight::Bold),
                                ),
                            )
                            .child(
                                div()
                                    .w(px(100.0))
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .child(Text::new(line.target).color(theme.text_secondary)),
                            )
                            .child(Text::new(line.message).color(theme.text_primary))
                    })),
            )
    }
}
