//! History section view - staking history and rewards chart.

use gpui::prelude::*;
use gpui::{
    AnyElement, Bounds, Context, PathBuilder, Pixels, Point, Rgba, Window, canvas, div, point, px,
    quad, size, transparent_black,
};
use gpui_ui_kit::theme::ThemeExt;
use gpui_ui_kit::*;

use crate::app::{HistoryPoint, StkoptApp};

pub struct HistorySection;

const Y_AXIS_WIDTH: f32 = 56.0;
const CHART_GAP: f32 = 12.0;
const MAX_X_TICKS: usize = 6;

impl HistorySection {
    pub fn render(app: &mut StkoptApp, cx: &mut Context<StkoptApp>) -> impl IntoElement {
        let theme = cx.theme();
        let entity = app.entity.clone();
        let is_loading = app.history_loading;
        let has_account = app.watched_account.is_some();
        let data_ready = app.data_download_complete();
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
        .size(ButtonSize::Xs)
        .disabled(!has_account || is_loading || !data_ready)
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
                    .gap_3()
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
                    .gap_1()
                    .child(Text::new("⏳").size(TextSize::Lg))
                    .child(
                        Text::new("Loading staking history...")
                            .size(TextSize::Xs)
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

        let chart_points: Vec<ApyChartPoint> = app
            .staking_history
            .iter()
            .map(ApyChartPoint::from)
            .collect();
        let max_apy = chart_points
            .iter()
            .map(|point| point.apy_percent)
            .fold(0.0_f64, f64::max);
        let y_max = nice_axis_max(max_apy);
        let y_ticks = y_axis_ticks(y_max);
        let x_tick_indices = chart_tick_indices(chart_points.len(), MAX_X_TICKS);
        let x_tick_labels: Vec<String> = x_tick_indices
            .iter()
            .filter_map(|index| chart_points.get(*index).map(|point| point.label.clone()))
            .collect();

        let chart_width = (app.viewport_width - 220.0 - 48.0 - 32.0).max(300.0);
        let plot_width = (chart_width - Y_AXIS_WIDTH - CHART_GAP).max(240.0);
        let plot_height = (chart_width * 0.42).clamp(220.0, 360.0);

        Card::new()
            .content(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(Heading::h3("APY Trend"))
                    .child(
                        div()
                            .w(px(chart_width))
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .child(render_y_axis_labels(&y_ticks, theme, plot_height))
                                    .child(render_apy_plot(
                                        chart_points,
                                        x_tick_indices,
                                        y_max,
                                        theme,
                                        plot_width,
                                        plot_height,
                                    )),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .child(div().w(px(Y_AXIS_WIDTH)))
                                    .child(render_x_axis_labels(&x_tick_labels, theme, plot_width)),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_history_table(app: &StkoptApp, theme: &gpui_ui_kit::theme::Theme) -> AnyElement {
        if app.staking_history.is_empty() {
            return div()
                .p_8()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_1()
                .child(Text::new("📊").size(TextSize::Lg))
                .child(
                    Text::new("No history data")
                        .size(TextSize::Xs)
                        .color(theme.text_secondary),
                )
                .child(
                    Text::new("Connect to a network and watch an account to see staking history")
                        .size(TextSize::Xs)
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
                .px_3()
                .py_2()
                .bg(theme.surface)
                .border_b_1()
                .border_color(theme.border)
                .child(
                    div().w(px(80.0)).child(
                        Text::new("Era")
                            .size(TextSize::Xs)
                            .weight(TextWeight::Semibold),
                    ),
                )
                .child(
                    div().w(px(100.0)).child(
                        Text::new("Date")
                            .size(TextSize::Xs)
                            .weight(TextWeight::Semibold),
                    ),
                )
                .child(
                    div().flex_1().child(
                        Text::new("Staked")
                            .size(TextSize::Xs)
                            .weight(TextWeight::Semibold),
                    ),
                )
                .child(
                    div().w(px(120.0)).child(
                        Text::new("Rewards")
                            .size(TextSize::Xs)
                            .weight(TextWeight::Semibold),
                    ),
                )
                .child(
                    div().w(px(80.0)).child(
                        Text::new("APY")
                            .size(TextSize::Xs)
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
                    .px_3()
                    .py_1()
                    .bg(row_bg)
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        div().w(px(80.0)).child(
                            Text::new(format!("#{}", point.era))
                                .size(TextSize::Xs)
                                .color(theme.text_secondary),
                        ),
                    )
                    .child(
                        div().w(px(100.0)).child(
                            Text::new(date_str)
                                .size(TextSize::Xs)
                                .color(theme.text_secondary),
                        ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .child(Text::new(staked_str).size(TextSize::Xs)),
                    )
                    .child(
                        div().w(px(120.0)).child(
                            Text::new(rewards_str)
                                .size(TextSize::Xs)
                                .color(theme.success),
                        ),
                    )
                    .child(
                        div()
                            .w(px(80.0))
                            .child(Text::new(apy_str).size(TextSize::Xs).color(apy_color)),
                    ),
            );
        }

        // Show count if more history exists
        if app.staking_history.len() > 30 {
            list = list.child(
                div().px_3().py_2().child(
                    Text::new(format!(
                        "Showing last 30 of {} eras",
                        app.staking_history.len()
                    ))
                    .size(TextSize::Xs)
                    .color(theme.text_secondary),
                ),
            );
        }

        Card::new().content(list).into_any_element()
    }
}

#[derive(Clone)]
struct ApyChartPoint {
    label: String,
    apy_percent: f64,
}

impl From<&HistoryPoint> for ApyChartPoint {
    fn from(point: &HistoryPoint) -> Self {
        let apy_percent = if point.apy.is_finite() {
            (point.apy * 100.0).max(0.0)
        } else {
            0.0
        };

        Self {
            label: format_history_axis_label(point),
            apy_percent,
        }
    }
}

fn render_y_axis_labels(
    ticks: &[f64],
    theme: &gpui_ui_kit::theme::Theme,
    plot_height: f32,
) -> AnyElement {
    let mut column = div()
        .w(px(Y_AXIS_WIDTH))
        .h(px(plot_height))
        .flex()
        .flex_col()
        .justify_between()
        .items_end()
        .pr_1();

    for tick in ticks {
        column = column.child(
            Text::new(format_percent_tick(*tick))
                .size(TextSize::Xs)
                .color(theme.text_secondary),
        );
    }

    column.into_any_element()
}

fn render_x_axis_labels(
    labels: &[String],
    theme: &gpui_ui_kit::theme::Theme,
    plot_width: f32,
) -> AnyElement {
    let mut row = div()
        .w(px(plot_width))
        .flex()
        .items_center()
        .justify_between();

    for label in labels {
        row = row.child(
            Text::new(label.clone())
                .size(TextSize::Xs)
                .color(theme.text_secondary),
        );
    }

    row.into_any_element()
}

fn render_apy_plot(
    points: Vec<ApyChartPoint>,
    x_tick_indices: Vec<usize>,
    y_max: f64,
    theme: &gpui_ui_kit::theme::Theme,
    plot_width: f32,
    plot_height: f32,
) -> AnyElement {
    let line_color = theme.success;
    let marker_color = theme.success;
    let grid_color = with_alpha(theme.border, 0.55);
    let axis_color = with_alpha(theme.text_muted, 0.85);
    let background = theme.background;
    let border = theme.border;

    div()
        .w(px(plot_width))
        .h(px(plot_height))
        .rounded_md()
        .bg(background)
        .border_1()
        .border_color(border)
        .child(
            canvas(
                move |_, _, _| {},
                move |bounds, _, window, _| {
                    paint_chart_grid(
                        bounds,
                        &x_tick_indices,
                        points.len(),
                        grid_color,
                        axis_color,
                        window,
                    );
                    paint_apy_line(bounds, &points, y_max, line_color, marker_color, window);
                },
            )
            .size_full(),
        )
        .into_any_element()
}

fn paint_chart_grid(
    bounds: Bounds<Pixels>,
    x_tick_indices: &[usize],
    point_count: usize,
    grid_color: Rgba,
    axis_color: Rgba,
    window: &mut Window,
) {
    let left = bounds.origin.x;
    let right = bounds.origin.x + bounds.size.width;
    let top = bounds.origin.y;
    let bottom = bounds.origin.y + bounds.size.height;

    for tick in 0..=4 {
        let y = bottom - px((tick as f32 / 4.0) * f32::from(bounds.size.height));
        paint_stroke(
            window,
            point(left, y),
            point(right, y),
            if tick == 0 { axis_color } else { grid_color },
            if tick == 0 { 1.25 } else { 1.0 },
        );
    }

    paint_stroke(
        window,
        point(left, top),
        point(left, bottom),
        axis_color,
        1.25,
    );

    for index in x_tick_indices {
        let x = x_position(*index, point_count, bounds);
        paint_stroke(window, point(x, top), point(x, bottom), grid_color, 1.0);
    }
}

fn paint_apy_line(
    bounds: Bounds<Pixels>,
    points: &[ApyChartPoint],
    y_max: f64,
    line_color: Rgba,
    marker_color: Rgba,
    window: &mut Window,
) {
    if points.len() < 2 {
        return;
    }

    let mut builder = PathBuilder::stroke(px(2.0));
    for (index, chart_point) in points.iter().enumerate() {
        let point =
            chart_point_position(index, chart_point.apy_percent, points.len(), y_max, bounds);
        if index == 0 {
            builder.move_to(point);
        } else {
            builder.line_to(point);
        }
    }

    if let Ok(path) = builder.build() {
        window.paint_path(path, line_color);
    }

    for (index, chart_point) in points.iter().enumerate() {
        let center =
            chart_point_position(index, chart_point.apy_percent, points.len(), y_max, bounds);
        let radius = px(3.0);
        window.paint_quad(quad(
            Bounds {
                origin: point(center.x - radius, center.y - radius),
                size: size(radius * 2.0, radius * 2.0),
            },
            radius,
            marker_color,
            px(0.0),
            transparent_black(),
            Default::default(),
        ));
    }
}

fn paint_stroke(
    window: &mut Window,
    from: Point<Pixels>,
    to: Point<Pixels>,
    color: Rgba,
    width: f32,
) {
    let mut builder = PathBuilder::stroke(px(width));
    builder.move_to(from);
    builder.line_to(to);
    if let Ok(path) = builder.build() {
        window.paint_path(path, color);
    }
}

fn chart_point_position(
    index: usize,
    apy_percent: f64,
    point_count: usize,
    y_max: f64,
    bounds: Bounds<Pixels>,
) -> Point<Pixels> {
    let x = x_position(index, point_count, bounds);
    let normalized_y = if y_max > 0.0 {
        (apy_percent / y_max).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let y = bounds.origin.y + bounds.size.height
        - px((normalized_y as f32) * f32::from(bounds.size.height));
    point(x, y)
}

fn x_position(index: usize, point_count: usize, bounds: Bounds<Pixels>) -> Pixels {
    if point_count <= 1 {
        return bounds.origin.x + bounds.size.width / 2.0;
    }

    let normalized_x = index as f32 / (point_count - 1) as f32;
    bounds.origin.x + px(normalized_x * f32::from(bounds.size.width))
}

fn nice_axis_max(max_value: f64) -> f64 {
    if !max_value.is_finite() || max_value <= 0.0 {
        return 1.0;
    }

    let padded = max_value * 1.12;
    let magnitude = 10_f64.powf(padded.log10().floor());
    let normalized = padded / magnitude;
    let nice_normalized = if normalized <= 1.0 {
        1.0
    } else if normalized <= 2.0 {
        2.0
    } else if normalized <= 5.0 {
        5.0
    } else {
        10.0
    };

    nice_normalized * magnitude
}

fn y_axis_ticks(y_max: f64) -> Vec<f64> {
    (0..=4)
        .rev()
        .map(|tick| y_max * tick as f64 / 4.0)
        .collect()
}

fn chart_tick_indices(point_count: usize, max_ticks: usize) -> Vec<usize> {
    if point_count == 0 || max_ticks == 0 {
        return Vec::new();
    }
    if point_count <= max_ticks {
        return (0..point_count).collect();
    }

    let last_index = point_count - 1;
    let steps = max_ticks - 1;
    let mut indices = Vec::with_capacity(max_ticks);
    for tick in 0..max_ticks {
        let index = ((tick * last_index) + (steps / 2)) / steps;
        if indices.last().copied() != Some(index) {
            indices.push(index);
        }
    }
    indices
}

fn format_history_axis_label(point: &HistoryPoint) -> String {
    point
        .date
        .as_deref()
        .and_then(format_mm_dd_date)
        .unwrap_or_else(|| format!("#{}", point.era))
}

fn format_mm_dd_date(date: &str) -> Option<String> {
    if let (Some(month), Some(day)) = (date.get(5..7), date.get(8..10))
        && matches!(date.as_bytes().get(4), Some(b'-' | b'/'))
        && matches!(date.as_bytes().get(7), Some(b'-' | b'/'))
        && month.bytes().all(|b| b.is_ascii_digit())
        && day.bytes().all(|b| b.is_ascii_digit())
    {
        return Some(format!("{}-{}", month, day));
    }

    if let (Some(month), Some(day), Some(prefix)) = (date.get(4..6), date.get(6..8), date.get(0..8))
        && prefix.bytes().all(|b| b.is_ascii_digit())
    {
        return Some(format!("{}-{}", month, day));
    }

    if let Some(mm_dd) = date.get(0..5)
        && matches!(mm_dd.as_bytes().get(2), Some(b'-' | b'/'))
        && mm_dd
            .bytes()
            .enumerate()
            .all(|(index, byte)| index == 2 || byte.is_ascii_digit())
    {
        return Some(mm_dd.replace('/', "-"));
    }

    None
}

fn format_percent_tick(value: f64) -> String {
    if value >= 10.0 || (value - value.round()).abs() < 0.05 {
        format!("{:.0}%", value)
    } else {
        format!("{:.1}%", value)
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
                    .size(TextSize::Xs)
                    .color(theme.text_secondary),
            )
            .child(Text::new(value).size(TextSize::Xl).weight(TextWeight::Bold)),
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
                    .size(TextSize::Xs)
                    .color(theme.text_secondary),
            )
            .child(
                Text::new(value)
                    .size(TextSize::Xl)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_labels_use_mm_dd() {
        assert_eq!(format_mm_dd_date("20260531").as_deref(), Some("05-31"));
        assert_eq!(format_mm_dd_date("2026-05-31").as_deref(), Some("05-31"));
        assert_eq!(format_mm_dd_date("2026/12/07").as_deref(), Some("12-07"));
        assert_eq!(format_mm_dd_date("05-31").as_deref(), Some("05-31"));
    }

    #[test]
    fn date_label_falls_back_to_era() {
        let point = HistoryPoint {
            era: 42,
            date: Some("unknown".to_string()),
            bonded: 0,
            reward: 0,
            apy: 0.0,
        };

        assert_eq!(format_history_axis_label(&point), "#42");
    }

    #[test]
    fn y_axis_ticks_start_at_zero() {
        let ticks = y_axis_ticks(20.0);
        assert_eq!(ticks.last().copied(), Some(0.0));
    }
}
