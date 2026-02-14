//! Dashboard section view - overview of staking status.

use gpui::prelude::*;
use gpui::*;
use gpui_ui_kit::theme::ThemeExt;
use gpui_ui_kit::*;

use crate::app::{StakingOperation, StkoptApp};

pub struct DashboardSection;

impl DashboardSection {
    pub fn render(app: &StkoptApp, cx: &Context<StkoptApp>) -> impl IntoElement {
        tracing::debug!("[DASHBOARD] render called");
        let theme = cx.theme();
        let symbol = app.token_symbol();
        let decimals = app.token_decimals();
        let entity = app.entity.clone();

        let (total_balance, bonded, unbonding, rewards) = if let Some(ref info) = app.staking_info {
            (
                format_balance(info.total_balance, symbol, decimals),
                format_balance(info.bonded, symbol, decimals),
                format_balance(info.unbonding, symbol, decimals),
                format_balance(info.rewards_pending, symbol, decimals),
            )
        } else {
            (
                format!("-- {}", symbol),
                format!("-- {}", symbol),
                format!("-- {}", symbol),
                format!("-- {}", symbol),
            )
        };

        div()
            .id("dashboard-section-root")
            .flex()
            .flex_col()
            .gap_6()
            .on_mouse_down(MouseButton::Left, |_event, _window, _cx| {
                tracing::info!("[DASHBOARD] Section root clicked!");
            })
            .child(Heading::h1("Dashboard"))
            .child(Text::new("Overview of your staking activity").size(TextSize::Md))
            .child(
                div()
                    .flex()
                    .gap_4()
                    .child(stat_card("Total Balance", total_balance, "💰", &theme))
                    .child(stat_card("Bonded", bonded, "🔒", &theme))
                    .child(stat_card("Unbonding", unbonding, "⏳", &theme))
                    .child(stat_card("Pending Rewards", rewards, "🎁", &theme)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(Heading::h2("Quick Actions"))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_3()
                            .child(
                                Button::new("btn-bond", "Bond")
                                    .variant(ButtonVariant::Primary)
                                    .on_click({
                                        let entity = entity.clone();
                                        move |_window, cx| {
                                            entity.update(cx, |this, cx| {
                                                this.open_staking_modal(StakingOperation::Bond, cx);
                                            });
                                        }
                                    }),
                            )
                            .child(
                                Button::new("btn-unbond", "Unbond")
                                    .variant(ButtonVariant::Secondary)
                                    .on_click({
                                        let entity = entity.clone();
                                        move |_window, cx| {
                                            entity.update(cx, |this, cx| {
                                                this.open_staking_modal(
                                                    StakingOperation::Unbond,
                                                    cx,
                                                );
                                            });
                                        }
                                    }),
                            )
                            .child(
                                Button::new("btn-rebond", "Rebond")
                                    .variant(ButtonVariant::Secondary)
                                    .on_click({
                                        let entity = entity.clone();
                                        move |_window, cx| {
                                            entity.update(cx, |this, cx| {
                                                this.open_staking_modal(
                                                    StakingOperation::Rebond,
                                                    cx,
                                                );
                                            });
                                        }
                                    }),
                            )
                            .child(
                                Button::new("btn-claim", "Claim Rewards")
                                    .variant(ButtonVariant::Secondary)
                                    .on_click({
                                        let entity = entity.clone();
                                        move |_window, cx| {
                                            entity.update(cx, |this, cx| {
                                                // Route to pool claim if pool rewards > 0
                                                let has_pool_rewards = this
                                                    .staking_info
                                                    .as_ref()
                                                    .is_some_and(|info| info.rewards_pending > 0);
                                                if has_pool_rewards {
                                                    this.open_pool_modal(
                                                        crate::app::PoolOperation::ClaimPayout,
                                                        None,
                                                        cx,
                                                    );
                                                } else {
                                                    this.tx_status_message = Some(
                                                        "Staking rewards are auto-distributed. Use pool claim for pool rewards.".to_string(),
                                                    );
                                                    cx.notify();
                                                }
                                            });
                                        }
                                    }),
                            )
                            .child(
                                Button::new("btn-nominate", "Nominate")
                                    .variant(ButtonVariant::Secondary)
                                    .on_click({
                                        let entity = entity.clone();
                                        move |_window, cx| {
                                            entity.update(cx, |this, cx| {
                                                // Navigate to Optimization section
                                                this.current_section =
                                                    crate::app::Section::Optimization;
                                                cx.notify();
                                            });
                                        }
                                    }),
                            )
                            .child(
                                Button::new("btn-set-payee", "Set Payee")
                                    .variant(ButtonVariant::Secondary)
                                    .on_click({
                                        let entity = entity.clone();
                                        move |_window, cx| {
                                            entity.update(cx, |this, cx| {
                                                this.open_staking_modal(
                                                    StakingOperation::SetPayee,
                                                    cx,
                                                );
                                            });
                                        }
                                    }),
                            ),
                    ),
            )
    }
}

fn stat_card(
    title: &'static str,
    value: String,
    icon: &'static str,
    theme: &gpui_ui_kit::theme::Theme,
) -> impl IntoElement {
    Card::new().content(
        div()
            .flex()
            .flex_col()
            .gap_2()
            .min_w(px(180.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(Text::new(icon).size(TextSize::Lg))
                    .child(
                        Text::new(title)
                            .size(TextSize::Sm)
                            .color(theme.text_secondary),
                    ),
            )
            .child(Text::new(value).size(TextSize::Xl).weight(TextWeight::Bold)),
    )
}

fn format_balance(amount: u128, symbol: &str, decimals: u8) -> String {
    let divisor = 10u128.pow(decimals as u32);
    let whole = amount / divisor;
    if whole >= 1_000_000 {
        let m = whole as f64 / 1_000_000.0;
        format!("{:.1}m{}", m, symbol)
    } else if whole >= 10_000 {
        let k = whole as f64 / 1_000.0;
        format!("{:.0}k{}", k, symbol)
    } else {
        let frac_divisor = 10u128.pow(decimals.saturating_sub(4) as u32);
        let frac = (amount % divisor) / frac_divisor;
        format!("{}.{:04} {}", whole, frac, symbol)
    }
}
