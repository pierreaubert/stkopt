//! Pools section view - nomination pools management.

use gpui::prelude::*;
use gpui::*;
use gpui_ui_kit::theme::ThemeExt;
use gpui_ui_kit::*;

use crate::actions::PoolSortColumn;
use crate::app::{PoolOperation, PoolState, StkoptApp};

pub struct PoolsSection;

impl PoolsSection {
    pub fn render(app: &mut StkoptApp, cx: &Context<StkoptApp>) -> impl IntoElement {
        let theme = cx.theme();
        let entity = app.entity.clone();
        let data_ready = app.data_download_complete();

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
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                Input::new("pool-search")
                                    .placeholder("Search pools...")
                                    .size(InputSize::Sm)
                                    .value(app.pool_search.clone())
                                    .on_change({
                                        let entity = entity.clone();
                                        move |value: &str, _window, cx| {
                                            let value = value.to_string();
                                            entity.update(cx, |this, cx| {
                                                this.pool_search = value;
                                                this.pool_filter_cache.invalidate();
                                                cx.notify();
                                            });
                                        }
                                    }),
                            )
                            .child(
                                Button::new("btn-refresh-pools", "Refresh")
                                    .variant(ButtonVariant::Secondary)
                                    .size(ButtonSize::Xs)
                                    .disabled(!data_ready)
                                    .on_click({
                                        let entity = entity.clone();
                                        move |_window, cx| {
                                            entity.update(cx, |this, cx| {
                                                if let Some(ref handle) = this.chain_handle {
                                                    this.pools_loading = true;
                                                    this.pools_progress = 0.1;
                                                    cx.notify();
                                                    let handle = handle.clone();
                                                    let entity = this.entity.clone();
                                                    let mut async_cx = cx.to_async();
                                                    cx.spawn(
                                                        move |_this: gpui::WeakEntity<StkoptApp>,
                                                              _cx: &mut gpui::AsyncApp| async move {
                                                            let result = handle.fetch_pools().await;
                                                            let _ = entity.update(
                                                                &mut async_cx,
                                                                |this, cx: &mut Context<StkoptApp>| {
                                                                    match result {
                                                                        Ok(pools) => {
                                                                            this.apply_chain_update(
                                                                                crate::chain::ChainUpdate::PoolsLoaded(pools),
                                                                                cx,
                                                                            );
                                                                        }
                                                                        Err(e) => {
                                                                            tracing::error!(
                                                                                "Failed to refresh pools: {}",
                                                                                e
                                                                            );
                                                                            this.pools_loading = false;
                                                                            this.connection_error = Some(format!(
                                                                                "Failed to refresh pools: {}",
                                                                                e
                                                                            ));
                                                                            cx.notify();
                                                                        }
                                                                    }
                                                                },
                                                            );
                                                        },
                                                    )
                                                    .detach();
                                                }
                                            });
                                        }
                                    }),
                            ),
                    ),
            )
            .child(
                Text::new("Join a nomination pool to stake with smaller amounts")
                    .size(TextSize::Xs)
                    .color(theme.text_secondary),
            )
            .child(Self::render_pool_list(app, &theme, entity))
    }

    fn render_pool_list(
        app: &mut StkoptApp,
        theme: &gpui_ui_kit::theme::Theme,
        entity: Entity<StkoptApp>,
    ) -> AnyElement {
        if app.pools.is_empty() {
            return div()
                .p_8()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_1()
                .child(Text::new("🏊").size(TextSize::Lg))
                .child(
                    Text::new("No pools loaded")
                        .size(TextSize::Xs)
                        .color(theme.text_secondary),
                )
                .child(
                    Text::new("Connect to a network to view nomination pools")
                        .size(TextSize::Xs)
                        .color(theme.text_secondary),
                )
                .into_any_element();
        }

        let filtered = app.filtered_pools_cached();
        let filtered_count = filtered.len();
        let sort_column = app.pool_sort;
        let sort_asc = app.pool_sort_asc;
        let symbol = app.token_symbol();
        let decimals = app.token_decimals();
        let commands_available = app.commands_available();

        // Build header
        let header = div()
            .flex()
            .items_center()
            .px_3()
            .py_2()
            .bg(theme.surface)
            .border_b_1()
            .border_color(theme.border)
            .child(sortable_header_fixed(
                "ID",
                50.0,
                PoolSortColumn::Id,
                sort_column,
                sort_asc,
                theme,
                entity.clone(),
            ))
            .child(
                div().flex_1().child(
                    Text::new("Pool Name")
                        .size(TextSize::Xs)
                        .weight(TextWeight::Semibold),
                ),
            )
            .child(sortable_header_fixed(
                "Members",
                100.0,
                PoolSortColumn::Members,
                sort_column,
                sort_asc,
                theme,
                entity.clone(),
            ))
            .child(sortable_header_fixed(
                "Total Bonded",
                120.0,
                PoolSortColumn::TotalBonded,
                sort_column,
                sort_asc,
                theme,
                entity.clone(),
            ))
            .child(sortable_header_fixed(
                "State",
                80.0,
                PoolSortColumn::State,
                sort_column,
                sort_asc,
                theme,
                entity.clone(),
            ))
            .child(sortable_header_fixed(
                "APY",
                70.0,
                PoolSortColumn::Apy,
                sort_column,
                sort_asc,
                theme,
                entity.clone(),
            ))
            .child(div().w(px(70.0)).child(Text::new("").size(TextSize::Xs)));

        let mut list = div().flex().flex_col().child(header);

        for (i, (_original_idx, pool)) in filtered.iter().enumerate() {
            let bonded_str = format_bonded(pool.total_bonded, symbol, decimals);
            let state_str = match pool.state {
                PoolState::Open => "Open",
                PoolState::Blocked => "Blocked",
                PoolState::Destroying => "Destroying",
            };
            let state_color = match pool.state {
                PoolState::Open => theme.success,
                PoolState::Blocked => theme.warning,
                PoolState::Destroying => theme.error,
            };
            let row_bg = if i % 2 == 0 {
                theme.background
            } else {
                theme.surface
            };
            let pool_id = pool.id;
            let is_open = pool.state == PoolState::Open;
            let entity = entity.clone();

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
                        div().w(px(50.0)).child(
                            Text::new(format!("#{}", pool.id))
                                .size(TextSize::Xs)
                                .color(theme.text_secondary),
                        ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .child(Text::new(pool.name.clone()).size(TextSize::Xs)),
                    )
                    .child(
                        div()
                            .w(px(100.0))
                            .child(Text::new(format!("{}", pool.member_count)).size(TextSize::Xs)),
                    )
                    .child(
                        div()
                            .w(px(120.0))
                            .child(Text::new(bonded_str).size(TextSize::Xs)),
                    )
                    .child(
                        div()
                            .w(px(80.0))
                            .child(Text::new(state_str).size(TextSize::Xs).color(state_color)),
                    )
                    .child(
                        div().w(px(70.0)).child(
                            Text::new(
                                pool.apy
                                    .map(|a| format!("{:.1}%", a * 100.0))
                                    .unwrap_or_else(|| "-".to_string()),
                            )
                            .size(TextSize::Xs),
                        ),
                    )
                    .child(
                        div().w(px(70.0)).child(
                            Button::new(
                                SharedString::from(format!("btn-join-pool-{}", pool_id)),
                                "Join",
                            )
                            .size(ButtonSize::Xs)
                            .variant(ButtonVariant::Primary)
                            .theme(crate::theme::button_theme_for_ui_theme(theme))
                            .disabled(!is_open || !commands_available)
                            .on_click(move |_window, cx| {
                                entity.update(cx, |this, cx| {
                                    this.open_pool_modal(PoolOperation::Join, Some(pool_id), cx);
                                });
                            }),
                        ),
                    ),
            );
        }

        if filtered_count != app.pools.len() {
            list = list.child(
                div().px_3().py_2().child(
                    Text::new(format!(
                        "Showing {} of {} pools",
                        filtered_count,
                        app.pools.len()
                    ))
                    .size(TextSize::Xs)
                    .color(theme.text_secondary),
                ),
            );
        }

        Card::new().content(list).into_any_element()
    }
}

fn format_bonded(amount: u128, symbol: &str, decimals: u8) -> String {
    let divisor = 10u128.pow(decimals as u32);
    let whole = amount / divisor;
    if whole >= 1_000_000 {
        format!("{:.2}M {}", whole as f64 / 1_000_000.0, symbol)
    } else if whole >= 1_000 {
        format!("{:.2}K {}", whole as f64 / 1_000.0, symbol)
    } else {
        format!("{:.2} {}", amount as f64 / divisor as f64, symbol)
    }
}

/// Render a sortable column header with fixed width.
fn sortable_header_fixed(
    label: &'static str,
    width: f32,
    column: PoolSortColumn,
    current_sort: PoolSortColumn,
    current_asc: bool,
    theme: &gpui_ui_kit::theme::Theme,
    entity: Entity<StkoptApp>,
) -> impl IntoElement {
    let is_active = column == current_sort;
    let indicator = if is_active {
        if current_asc { " ▲" } else { " ▼" }
    } else {
        ""
    };

    div()
        .id(SharedString::from(format!("sort-pool-{:?}", column)))
        .w(px(width))
        .cursor_pointer()
        .on_click(move |_event, _window, cx| {
            entity.update(cx, |this, cx| {
                if this.pool_sort == column {
                    this.pool_sort_asc = !this.pool_sort_asc;
                } else {
                    this.pool_sort = column;
                    this.pool_sort_asc = false; // Default to descending for new column
                }
                this.pool_filter_cache.invalidate();
                cx.notify();
            });
        })
        .child(
            Text::new(format!("{}{}", label, indicator))
                .size(TextSize::Xs)
                .weight(TextWeight::Semibold)
                .color(if is_active {
                    theme.accent
                } else {
                    theme.text_primary
                }),
        )
}
