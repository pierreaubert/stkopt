//! Log viewer component.

use gpui::Styled; // Explicit import to ensure traits are visible
use gpui::prelude::*;
use gpui::*;
use gpui_ui_kit::theme::ThemeExt;
use gpui_ui_kit::*;

use crate::log::{LogBuffer, LogLevel};

pub struct LogsView;

impl LogsView {
    pub fn render(
        buffer: &LogBuffer,
        min_level: LogLevel,
        cx: &Context<crate::app::StkoptApp>,
        entity: Entity<crate::app::StkoptApp>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let all_lines = buffer.get_lines();
        let lines: Vec<_> = all_lines
            .into_iter()
            .filter(|line| line.level >= min_level)
            .collect();

        gpui::div()
            .id("logs-scroll")
            .flex()
            .flex_col()
            .gap_1()
            .p_3()
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
                    .pb_1()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(Heading::h3("Application Logs"))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(Self::render_level_filter(min_level, entity.clone()))
                            .child(
                                Text::new(format!("{} events", lines.len()))
                                    .size(TextSize::Xs)
                                    .color(theme.text_secondary),
                            )
                            .child(
                                Button::new("btn-close-logs", "Close")
                                    .variant(ButtonVariant::Ghost)
                                    .size(ButtonSize::Xs)
                                    .on_click(move |_window, cx| {
                                        entity.update(cx, |this, cx| {
                                            this.set_logs_visible(false, cx);
                                        });
                                    }),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .pt_1()
                    .font_family("JetBrains Mono") // Monospace if available, or system mono
                    .children(lines.into_iter().rev().map(|line| {
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
                            .gap_1()
                            .text_size(px(10.0))
                            .child(
                                Text::new(line.timestamp.format("%H:%M:%S").to_string())
                                    .color(theme.text_secondary)
                                    .weight(TextWeight::Light),
                            )
                            .child(
                                div().w(px(50.0)).child(
                                    Text::new(line.level.as_str())
                                        .color(color)
                                        .weight(TextWeight::Bold),
                                ),
                            )
                            .child(
                                div()
                                    .w(px(90.0))
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .child(Text::new(line.target).color(theme.text_secondary)),
                            )
                            .child(Text::new(line.message).color(theme.text_primary))
                    })),
            )
    }

    fn render_level_filter(
        current: LogLevel,
        entity: Entity<crate::app::StkoptApp>,
    ) -> impl IntoElement {
        let levels = [
            (LogLevel::Trace, "Trace"),
            (LogLevel::Debug, "Debug"),
            (LogLevel::Info, "Info"),
            (LogLevel::Warn, "Warn"),
            (LogLevel::Error, "Error"),
        ];

        div()
            .id("log-level-filter")
            .flex()
            .items_center()
            .gap_1()
            .children(levels.into_iter().enumerate().map(|(idx, (level, label))| {
                let is_active = level == current;
                let variant = if is_active {
                    ButtonVariant::Primary
                } else {
                    ButtonVariant::Ghost
                };
                let entity = entity.clone();
                Button::new(("log-level", idx), label)
                    .variant(variant)
                    .size(ButtonSize::Xs)
                    .on_click(move |_window, cx| {
                        entity.update(cx, |this, cx| {
                            this.set_log_level_filter(level, cx);
                        });
                    })
            }))
    }
}
