//! Pools section view - nomination pools management.

use gpui::prelude::*;
use gpui::*;
use gpui_ui_kit::theme::ThemeExt;
use gpui_ui_kit::*;

use crate::app::StkoptApp;
use crate::gpui_tokio::Tokio;

pub struct PoolsSection;

impl PoolsSection {
    pub fn render(app: &StkoptApp, cx: &Context<StkoptApp>) -> impl IntoElement {
        let theme = cx.theme();
        let entity = app.entity.clone();

        div()
            .flex()
            .flex_col()
            .gap_6()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(Heading::h1("Nomination Pools"))
                    .child(
                        Button::new("btn-refresh-pools", "Refresh")
                            .variant(ButtonVariant::Secondary)
                            .size(ButtonSize::Sm)
                            .on_click({
                                let entity = entity.clone();
                                move |_window, cx| {
                                    entity.update(cx, |this, cx| {
                                        if let Some(ref handle) = this.chain_handle {
                                            let handle = handle.clone();
                                            Tokio::spawn(cx, async move {
                                                if let Err(e) = handle.fetch_pools().await {
                                                    tracing::error!("Failed to refresh pools: {}", e);
                                                }
                                            }).detach();
                                        }
                                    });
                                }
                            }),
                    ),
            )
            .child(
                Text::new("Join a nomination pool to stake with smaller amounts")
                    .size(TextSize::Md)
                    .color(theme.text_secondary),
            )
            .child(Self::render_pool_list(app, &theme))
    }

    fn render_pool_list(
        app: &StkoptApp,
        theme: &gpui_ui_kit::theme::Theme,
    ) -> AnyElement {
        if app.pools.is_empty() {
            return div()
                .p_8()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_2()
                .child(Text::new("🏊").size(TextSize::Xl))
                .child(
                    Text::new("No pools loaded")
                        .size(TextSize::Md)
                        .color(theme.text_secondary),
                )
                .child(
                    Text::new("Connect to a network to view nomination pools")
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
                .child(div().w(px(50.0)).child(Text::new("ID").size(TextSize::Sm).weight(TextWeight::Semibold)))
                .child(
                    div()
                        .flex_1()
                        .child(Text::new("Pool Name").size(TextSize::Sm).weight(TextWeight::Semibold)),
                )
                .child(
                    div()
                        .w(px(100.0))
                        .child(Text::new("Members").size(TextSize::Sm).weight(TextWeight::Semibold)),
                )
                .child(
                    div()
                        .w(px(120.0))
                        .child(Text::new("Total Bonded").size(TextSize::Sm).weight(TextWeight::Semibold)),
                )
                .child(
                    div()
                        .w(px(80.0))
                        .child(Text::new("State").size(TextSize::Sm).weight(TextWeight::Semibold)),
                ),
        );

        // Pool rows
        for (i, pool) in app.pools.iter().enumerate() {
            let bonded_str = format_bonded(pool.total_bonded);
            let state_str = match pool.state {
                crate::app::PoolState::Open => "Open",
                crate::app::PoolState::Blocked => "Blocked",
                crate::app::PoolState::Destroying => "Destroying",
            };
            let state_color = match pool.state {
                crate::app::PoolState::Open => theme.success,
                crate::app::PoolState::Blocked => theme.warning,
                crate::app::PoolState::Destroying => theme.error,
            };
            let row_bg = if i % 2 == 0 { theme.background } else { theme.surface };

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
                        div()
                            .w(px(50.0))
                            .child(Text::new(format!("#{}", pool.id)).size(TextSize::Sm).color(theme.text_secondary)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .child(Text::new(pool.name.clone()).size(TextSize::Sm)),
                    )
                    .child(
                        div()
                            .w(px(100.0))
                            .child(Text::new(format!("{}", pool.member_count)).size(TextSize::Sm)),
                    )
                    .child(
                        div()
                            .w(px(120.0))
                            .child(Text::new(bonded_str).size(TextSize::Sm)),
                    )
                    .child(
                        div()
                            .w(px(80.0))
                            .child(Text::new(state_str).size(TextSize::Sm).color(state_color)),
                    ),
            );
        }

        Card::new().content(list).into_any_element()
    }
}

fn format_bonded(amount: u128) -> String {
    let dot = amount / 10_000_000_000;
    if dot >= 1_000_000 {
        format!("{:.1}M DOT", dot as f64 / 1_000_000.0)
    } else if dot >= 1_000 {
        format!("{:.1}K DOT", dot as f64 / 1_000.0)
    } else {
        format!("{} DOT", dot)
    }
}
