//! Validators section view - list and select validators for nomination.

use gpui::prelude::*;
use gpui::*;
use gpui_ui_kit::theme::ThemeExt;
use gpui_ui_kit::*;

use crate::actions::ValidatorSortColumn;
use crate::app::StkoptApp;
use crate::validators::filter_validators;

pub struct ValidatorsSection;

impl ValidatorsSection {
    pub fn render(app: &StkoptApp, cx: &Context<StkoptApp>) -> impl IntoElement {
        let entity = app.entity.clone();
        let is_loading = app.validators_loading;

        // Filter validators based on search query and blocked filter
        let filtered = filter_validators(&app.validators, &app.validator_search, app.show_blocked);
        let filtered_count = filtered.len();
        let total = app.validators.len();
        let selected = app.selected_validators.len();
        let show_blocked = app.show_blocked;

        div()
            .flex()
            .flex_col()
            .gap_6()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(Heading::h1("Validators"))
                    .child(
                        div()
                            .flex()
                            .gap_3()
                            .child(
                                Input::new("validator-search")
                                    .placeholder("Search validators...")
                                    .size(InputSize::Sm)
                                    .value(app.validator_search.clone())
                                    .on_change({
                                        let entity = entity.clone();
                                        move |value: &str, _window, cx| {
                                            let value = value.to_string();
                                            entity.update(cx, |this, cx| {
                                                this.validator_search = value;
                                                cx.notify();
                                            });
                                        }
                                    }),
                            )
                            .child(
                                Button::new(
                                    "btn-toggle-blocked",
                                    if show_blocked {
                                        "Hide Blocked"
                                    } else {
                                        "Show Blocked"
                                    },
                                )
                                .variant(ButtonVariant::Secondary)
                                .size(ButtonSize::Sm)
                                .on_click({
                                    let entity = entity.clone();
                                    move |_window, cx| {
                                        entity.update(cx, |this, cx| {
                                            this.show_blocked = !this.show_blocked;
                                            cx.notify();
                                        });
                                    }
                                }),
                            )
                            .child(
                                Button::new(
                                    "btn-refresh",
                                    if is_loading { "Loading..." } else { "Refresh" },
                                )
                                .variant(ButtonVariant::Secondary)
                                .size(ButtonSize::Sm)
                                .disabled(is_loading)
                                .on_click({
                                    let entity = entity.clone();
                                    move |_window, cx| {
                                        entity.update(cx, |this, cx| {
                                            if let Some(ref handle) = this.chain_handle {
                                                this.validators_loading = true;
                                                cx.notify();
                                                let handle = handle.clone();
                                                crate::gpui_tokio::Tokio::spawn(cx, async move {
                                                    if let Err(e) = handle.fetch_validators().await
                                                    {
                                                        tracing::error!(
                                                            "Failed to refresh validators: {}",
                                                            e
                                                        );
                                                    }
                                                })
                                                .detach();
                                            }
                                        });
                                    }
                                }),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap_4()
                    .child(Badge::new(format!("{} validators", total)))
                    .child(if filtered_count != total {
                        Badge::new(format!("{} shown", filtered_count))
                            .variant(BadgeVariant::Warning)
                    } else {
                        Badge::new(format!("{} shown", filtered_count))
                            .variant(BadgeVariant::Default)
                    })
                    .child(
                        Badge::new(format!("{} selected", selected)).variant(if selected > 0 {
                            BadgeVariant::Success
                        } else {
                            BadgeVariant::Default
                        }),
                    )
                    .when(selected > 0, |el| {
                        let entity = entity.clone();
                        el.child(
                            Button::new(
                                "btn-nominate-selected",
                                format!("Nominate Selected ({})", selected),
                            )
                            .variant(ButtonVariant::Primary)
                            .size(ButtonSize::Sm)
                            .on_click(move |_window, cx| {
                                entity.update(cx, |this, cx| {
                                    let targets: Vec<String> = this
                                        .selected_validators
                                        .iter()
                                        .filter_map(|&idx| this.validators.get(idx))
                                        .map(|v| v.address.clone())
                                        .collect();
                                    if !targets.is_empty() {
                                        this.generate_nominate_qr(targets, cx);
                                    }
                                });
                            }),
                        )
                    }),
            )
            .child(Self::render_validator_list(app, cx, &filtered))
    }

    fn render_validator_list(
        app: &StkoptApp,
        cx: &Context<StkoptApp>,
        filtered: &[(usize, &crate::app::ValidatorInfo)],
    ) -> AnyElement {
        let theme = cx.theme();
        let entity = app.entity.clone();
        let sort_column = app.validator_sort;
        let sort_asc = app.validator_sort_asc;
        let is_loading = app.validators_loading;

        // Show loading indicator when loading and no validators yet
        if is_loading && app.validators.is_empty() {
            return div()
                .p_8()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_2()
                .child(Text::new("⏳").size(TextSize::Xl))
                .child(
                    Text::new("Loading validators...")
                        .size(TextSize::Md)
                        .color(theme.text_secondary),
                )
                .child(
                    Text::new("This may take a moment with light client")
                        .size(TextSize::Sm)
                        .color(theme.text_secondary),
                )
                .into_any_element();
        }

        if filtered.is_empty() {
            return div()
                .p_8()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_2()
                .child(Text::new("📋").size(TextSize::Xl))
                .child(
                    Text::new(if app.validators.is_empty() {
                        "No validators loaded"
                    } else {
                        "No validators match your search"
                    })
                    .size(TextSize::Md)
                    .color(theme.text_secondary),
                )
                .child(
                    Text::new(if app.validators.is_empty() {
                        "Connect to a network to view validators"
                    } else {
                        "Try a different search term"
                    })
                    .size(TextSize::Sm)
                    .color(theme.text_secondary),
                )
                .into_any_element();
        }

        let mut list = div().flex().flex_col();

        // Header row with clickable sort columns
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
                    div().w(px(30.0)).child(
                        Text::new("Sel")
                            .size(TextSize::Xs)
                            .weight(TextWeight::Semibold),
                    ),
                )
                .child(
                    div().w(px(40.0)).child(
                        Text::new("#")
                            .size(TextSize::Sm)
                            .weight(TextWeight::Semibold),
                    ),
                )
                .child(sortable_header(
                    "Validator",
                    ValidatorSortColumn::Name,
                    sort_column,
                    sort_asc,
                    &theme,
                    entity.clone(),
                ))
                .child(sortable_header_fixed(
                    "Total Stake",
                    120.0,
                    ValidatorSortColumn::TotalStake,
                    sort_column,
                    sort_asc,
                    &theme,
                    entity.clone(),
                ))
                .child(sortable_header_fixed(
                    "Own Stake",
                    110.0,
                    ValidatorSortColumn::OwnStake,
                    sort_column,
                    sort_asc,
                    &theme,
                    entity.clone(),
                ))
                .child(sortable_header_fixed(
                    "Nom",
                    60.0,
                    ValidatorSortColumn::NominatorCount,
                    sort_column,
                    sort_asc,
                    &theme,
                    entity.clone(),
                ))
                .child(sortable_header_fixed(
                    "Commission",
                    100.0,
                    ValidatorSortColumn::Commission,
                    sort_column,
                    sort_asc,
                    &theme,
                    entity.clone(),
                ))
                .child(sortable_header_fixed(
                    "APY",
                    80.0,
                    ValidatorSortColumn::Apy,
                    sort_column,
                    sort_asc,
                    &theme,
                    entity.clone(),
                ))
                .child(sortable_header_fixed(
                    "Points",
                    70.0,
                    ValidatorSortColumn::Points,
                    sort_column,
                    sort_asc,
                    &theme,
                    entity.clone(),
                ))
                .child(sortable_header_fixed(
                    "Blocked",
                    70.0,
                    ValidatorSortColumn::Blocked,
                    sort_column,
                    sort_asc,
                    &theme,
                    entity.clone(),
                )),
        );

        // Validator rows (limit to first 200 for performance)
        for (i, (original_idx, validator)) in filtered.iter().take(200).enumerate() {
            let original_idx = *original_idx;
            let is_selected = app.selected_validators.contains(&original_idx);
            let name = validator.name.clone().unwrap_or_else(|| {
                if validator.address.len() >= 8 {
                    validator.address[..8].to_string()
                } else {
                    validator.address.clone()
                }
            });
            let addr_short = if validator.address.len() >= 16 {
                validator.address[..16].to_string()
            } else {
                validator.address.clone()
            };
            let stake_str = format_stake(
                validator.total_stake,
                app.token_symbol(),
                app.token_decimals(),
            );
            let own_stake_str = format_stake(
                validator.own_stake,
                app.token_symbol(),
                app.token_decimals(),
            );
            let commission_str = format!("{:.1}%", validator.commission * 100.0);
            let apy_str = validator
                .apy
                .map(|a| format!("{:.1}%", a * 100.0))
                .unwrap_or_else(|| "-".to_string());
            let points_str = if validator.points > 0 {
                validator.points.to_string()
            } else {
                "-".to_string()
            };
            let blocked_str = if validator.blocked { "Yes" } else { "" };
            let row_bg = if i % 2 == 0 {
                theme.background
            } else {
                theme.surface
            };
            let apy_color = if validator.apy.unwrap_or(0.0) > 0.15 {
                theme.success
            } else {
                theme.text_primary
            };
            let blocked_color = if validator.blocked {
                theme.error
            } else {
                theme.text_primary
            };

            let checkbox_text = if is_selected { "[x]" } else { "[ ]" };
            let selected_count = app.selected_validators.len();
            let entity = entity.clone();

            list =
                list.child(
                    div()
                        .id(SharedString::from(format!("validator-row-{}", i)))
                        .flex()
                        .items_center()
                        .px_4()
                        .py_2()
                        .bg(row_bg)
                        .border_b_1()
                        .border_color(theme.border)
                        .cursor_pointer()
                        .on_click(move |_event, _window, cx| {
                            entity.update(cx, |this, cx| {
                                if let Some(pos) = this
                                    .selected_validators
                                    .iter()
                                    .position(|&i| i == original_idx)
                                {
                                    this.selected_validators.remove(pos);
                                } else if selected_count < 16 {
                                    this.selected_validators.push(original_idx);
                                }
                                cx.notify();
                            });
                        })
                        .child(
                            div().w(px(30.0)).child(
                                Text::new(checkbox_text)
                                    .size(TextSize::Xs)
                                    .color(if is_selected {
                                        theme.accent
                                    } else {
                                        theme.text_secondary
                                    }),
                            ),
                        )
                        .child(
                            div().w(px(40.0)).child(
                                Text::new(format!("{}", i + 1))
                                    .size(TextSize::Sm)
                                    .color(theme.text_secondary),
                            ),
                        )
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .child(Text::new(name).size(TextSize::Sm))
                                .child(
                                    Text::new(addr_short)
                                        .size(TextSize::Xs)
                                        .color(theme.text_secondary),
                                ),
                        )
                        .child(
                            div()
                                .w(px(120.0))
                                .child(Text::new(stake_str).size(TextSize::Sm)),
                        )
                        .child(
                            div()
                                .w(px(110.0))
                                .child(Text::new(own_stake_str).size(TextSize::Sm)),
                        )
                        .child(div().w(px(60.0)).child(
                            Text::new(validator.nominator_count.to_string()).size(TextSize::Sm),
                        ))
                        .child(
                            div()
                                .w(px(100.0))
                                .child(Text::new(commission_str).size(TextSize::Sm)),
                        )
                        .child(
                            div()
                                .w(px(80.0))
                                .child(Text::new(apy_str).size(TextSize::Sm).color(apy_color)),
                        )
                        .child(
                            div()
                                .w(px(70.0))
                                .child(Text::new(points_str).size(TextSize::Sm)),
                        )
                        .child(
                            div().w(px(70.0)).child(
                                Text::new(blocked_str)
                                    .size(TextSize::Sm)
                                    .color(blocked_color),
                            ),
                        ),
                );
        }

        // Show count if more validators exist
        if filtered.len() > 200 {
            list = list.child(
                div().px_4().py_3().child(
                    Text::new(format!("... and {} more validators", filtered.len() - 200))
                        .size(TextSize::Sm)
                        .color(theme.text_secondary),
                ),
            );
        }

        Card::new().content(list).into_any_element()
    }
}

/// Render a sortable column header (flex-1 width).
fn sortable_header(
    label: &'static str,
    column: ValidatorSortColumn,
    current_sort: ValidatorSortColumn,
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
        .id(SharedString::from(format!("sort-{:?}", column)))
        .flex_1()
        .cursor_pointer()
        .on_click(move |_event, _window, cx| {
            entity.update(cx, |this, cx| {
                if this.validator_sort == column {
                    this.validator_sort_asc = !this.validator_sort_asc;
                } else {
                    this.validator_sort = column;
                    this.validator_sort_asc = false; // Default to descending for new column
                }
                // Sort the validators
                crate::validators::sort_validators(
                    &mut this.validators,
                    this.validator_sort,
                    this.validator_sort_asc,
                );
                cx.notify();
            });
        })
        .child(
            Text::new(format!("{}{}", label, indicator))
                .size(TextSize::Sm)
                .weight(TextWeight::Semibold)
                .color(if is_active {
                    theme.accent
                } else {
                    theme.text_primary
                }),
        )
}

/// Render a sortable column header with fixed width.
fn sortable_header_fixed(
    label: &'static str,
    width: f32,
    column: ValidatorSortColumn,
    current_sort: ValidatorSortColumn,
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
        .id(SharedString::from(format!("sort-{:?}", column)))
        .w(px(width))
        .cursor_pointer()
        .on_click(move |_event, _window, cx| {
            entity.update(cx, |this, cx| {
                if this.validator_sort == column {
                    this.validator_sort_asc = !this.validator_sort_asc;
                } else {
                    this.validator_sort = column;
                    this.validator_sort_asc = false;
                }
                crate::validators::sort_validators(
                    &mut this.validators,
                    this.validator_sort,
                    this.validator_sort_asc,
                );
                cx.notify();
            });
        })
        .child(
            Text::new(format!("{}{}", label, indicator))
                .size(TextSize::Sm)
                .weight(TextWeight::Semibold)
                .color(if is_active {
                    theme.accent
                } else {
                    theme.text_primary
                }),
        )
}

fn format_stake(stake: u128, symbol: &str, decimals: u8) -> String {
    let divisor = 10u128.pow(decimals as u32);
    let whole = stake / divisor;
    if whole >= 1_000_000 {
        format!("{:.2}M {}", whole as f64 / 1_000_000.0, symbol)
    } else if whole >= 1_000 {
        format!("{:.2}K {}", whole as f64 / 1_000.0, symbol)
    } else {
        format!("{:.2} {}", stake as f64 / divisor as f64, symbol)
    }
}
