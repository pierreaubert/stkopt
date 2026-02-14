//! History section view - staking history and rewards chart.

use gpui::prelude::*;
use gpui::*;
use gpui_px::{ChartTheme, line};
use gpui_ui_kit::theme::ThemeExt;
use gpui_ui_kit::*;

use crate::app::StkoptApp;

pub struct HistorySection;

impl HistorySection {
    pub fn render(app: &mut StkoptApp, cx: &mut Context<StkoptApp>) -> impl IntoElement {
        let theme = cx.theme();
        let entity = app.entity.clone();
        let is_loading = app.history_loading;
        let has_account = app.watched_account.is_some();
        let symbol = app.token_symbol();
        let decimals = app.token_decimals();

        let (total_rewards, avg_apy, eras_count) = if !app.staking_history.is_empty() {
            let total: u128 = app.staking_history.iter().map(|h| h.reward).sum();
            let avg_apy: f64 = app.staking_history.iter().map(|h| h.apy).sum::<f64>()
                / app.staking_history.len() as f64;
            (
                format_balance(total, symbol, decimals),
                format!("{:.2}%", avg_apy * 100.0),
                app.staking_history.len().to_string(),
            )
        } else {
            (format!("-- {}", symbol), "--%".to_string(), "0".to_string())
        };

        let refresh_button = Button::new(
            "btn-refresh-history",
            if is_loading { "Loading..." } else { "Refresh" },
        )
        .variant(ButtonVariant::Secondary)
        .size(ButtonSize::Sm)
        .disabled(!has_account || is_loading)
        .on_click({
            let entity = entity.clone();
            move |_window, cx| {
                entity.update(cx, |this, cx| {
                    this.load_history(cx);
                });
            }
        });

        div()
            .flex()
            .flex_col()
            .gap_6()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(Heading::h1("Staking History"))
                    .child(refresh_button),
            )
            .child(
                div()
                    .flex()
                    .gap_4()
                    .child(stat_card("Total Rewards", total_rewards, &theme))
                    .child(stat_card_success("Average APY", avg_apy, &theme))
                    .child(stat_card("Eras Tracked", eras_count, &theme)),
            )
            .child(if is_loading {
                // Show loading indicator
                div()
                    .p_8()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .child(Text::new("⏳").size(TextSize::Xl))
                    .child(
                        Text::new("Loading staking history...")
                            .size(TextSize::Md)
                            .color(theme.text_secondary),
                    )
                    .into_any_element()
            } else {
                Self::render_apy_chart(app, &theme)
            })
            .child(if is_loading {
                div().into_any_element()
            } else {
                Self::render_history_table(app, &theme)
            })
    }

    fn render_apy_chart(app: &StkoptApp, theme: &gpui_ui_kit::theme::Theme) -> AnyElement {
        if app.staking_history.len() < 2 {
            return div().into_any_element();
        }

        // Prepare data for the chart
        let x_data: Vec<f64> = app.staking_history.iter().map(|h| h.era as f64).collect();
        let y_data: Vec<f64> = app.staking_history.iter().map(|h| h.apy * 100.0).collect();

        let dark_theme = ChartTheme {
            plot_background: gpui::rgb(0x000000),
            grid_color: gpui::rgba(0xffffff33),
            axis_line_color: gpui::rgba(0xffffff55),
            axis_label_color: gpui::rgba(0xffffffcc),
            title_color: gpui::rgba(0xffffffee),
            legend_text_color: gpui::rgba(0xffffffcc),
        };

        // Compute chart width from available space: viewport - sidebar(220) - content padding(2*24) - card padding(2*16)
        let chart_width = (app.viewport_width - 220.0 - 48.0 - 32.0).max(300.0);
        let chart_height = chart_width / 2.0;

        // Build the line chart
        match line(&x_data, &y_data)
            .title("APY Over Time (%)")
            .color(0x22c55e) // Green color matching theme.success
            .stroke_width(2.0)
            .show_points(true)
            .size(chart_width, chart_height)
            .theme(dark_theme)
            .build()
        {
            Ok(chart) => Card::new()
                .content(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(Heading::h3("APY Trend"))
                        .child(chart),
                )
                .into_any_element(),
            Err(_) => div()
                .p_4()
                .child(
                    Text::new("Unable to render chart")
                        .size(TextSize::Sm)
                        .color(theme.text_secondary),
                )
                .into_any_element(),
        }
    }

    fn render_history_table(app: &StkoptApp, theme: &gpui_ui_kit::theme::Theme) -> AnyElement {
        if app.staking_history.is_empty() {
            return div()
                .p_8()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_2()
                .child(Text::new("📊").size(TextSize::Xl))
                .child(
                    Text::new("No history data")
                        .size(TextSize::Md)
                        .color(theme.text_secondary),
                )
                .child(
                    Text::new("Connect to a network and watch an account to see staking history")
                        .size(TextSize::Sm)
                        .color(theme.text_secondary),
                )
                .into_any_element();
        }

        let mut list = div().flex().flex_col();

        // Header row
        list = list.child(
            div()
                .flex()
                .items_center()
                .px_4()
                .py_3()
                .bg(theme.surface)
                .border_b_1()
                .border_color(theme.border)
                .child(
                    div().w(px(80.0)).child(
                        Text::new("Era")
                            .size(TextSize::Sm)
                            .weight(TextWeight::Semibold),
                    ),
                )
                .child(
                    div().w(px(100.0)).child(
                        Text::new("Date")
                            .size(TextSize::Sm)
                            .weight(TextWeight::Semibold),
                    ),
                )
                .child(
                    div().flex_1().child(
                        Text::new("Staked")
                            .size(TextSize::Sm)
                            .weight(TextWeight::Semibold),
                    ),
                )
                .child(
                    div().w(px(120.0)).child(
                        Text::new("Rewards")
                            .size(TextSize::Sm)
                            .weight(TextWeight::Semibold),
                    ),
                )
                .child(
                    div().w(px(80.0)).child(
                        Text::new("APY")
                            .size(TextSize::Sm)
                            .weight(TextWeight::Semibold),
                    ),
                ),
        );

        // History rows (show last 30 eras)
        let symbol = app.token_symbol();
        let decimals = app.token_decimals();
        for (i, point) in app.staking_history.iter().rev().take(30).enumerate() {
            let staked_str = format_balance(point.bonded, symbol, decimals);
            let rewards_str = format_balance(point.reward, symbol, decimals);
            let apy_str = format!("{:.2}%", point.apy * 100.0);
            let date_str = point.date.clone().unwrap_or_else(|| "-".to_string());
            let row_bg = if i % 2 == 0 {
                theme.background
            } else {
                theme.surface
            };
            let apy_color = if point.apy > 0.15 {
                theme.success
            } else {
                theme.text_primary
            };

            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .px_4()
                    .py_2()
                    .bg(row_bg)
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        div().w(px(80.0)).child(
                            Text::new(format!("#{}", point.era))
                                .size(TextSize::Sm)
                                .color(theme.text_secondary),
                        ),
                    )
                    .child(
                        div().w(px(100.0)).child(
                            Text::new(date_str)
                                .size(TextSize::Sm)
                                .color(theme.text_secondary),
                        ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .child(Text::new(staked_str).size(TextSize::Sm)),
                    )
                    .child(
                        div().w(px(120.0)).child(
                            Text::new(rewards_str)
                                .size(TextSize::Sm)
                                .color(theme.success),
                        ),
                    )
                    .child(
                        div()
                            .w(px(80.0))
                            .child(Text::new(apy_str).size(TextSize::Sm).color(apy_color)),
                    ),
            );
        }

        // Show count if more history exists
        if app.staking_history.len() > 30 {
            list = list.child(
                div().px_4().py_3().child(
                    Text::new(format!(
                        "Showing last 30 of {} eras",
                        app.staking_history.len()
                    ))
                    .size(TextSize::Sm)
                    .color(theme.text_secondary),
                ),
            );
        }

        Card::new().content(list).into_any_element()
    }
}

fn stat_card(
    label: &'static str,
    value: String,
    theme: &gpui_ui_kit::theme::Theme,
) -> impl IntoElement {
    Card::new().content(
        div()
            .flex()
            .flex_col()
            .gap_1()
            .min_w(px(150.0))
            .child(
                Text::new(label)
                    .size(TextSize::Sm)
                    .color(theme.text_secondary),
            )
            .child(Text::new(value).size(TextSize::Lg).weight(TextWeight::Bold)),
    )
}

fn stat_card_success(
    label: &'static str,
    value: String,
    theme: &gpui_ui_kit::theme::Theme,
) -> impl IntoElement {
    Card::new().content(
        div()
            .flex()
            .flex_col()
            .gap_1()
            .min_w(px(150.0))
            .child(
                Text::new(label)
                    .size(TextSize::Sm)
                    .color(theme.text_secondary),
            )
            .child(
                Text::new(value)
                    .size(TextSize::Lg)
                    .weight(TextWeight::Bold)
                    .color(theme.success),
            ),
    )
}

fn format_balance(amount: u128, symbol: &str, decimals: u8) -> String {
    let divisor = 10u128.pow(decimals as u32);
    let frac_divisor = 10u128.pow(decimals.saturating_sub(4) as u32);
    let whole = amount / divisor;
    let frac = (amount % divisor) / frac_divisor;
    format!("{}.{:04} {}", whole, frac, symbol)
}
