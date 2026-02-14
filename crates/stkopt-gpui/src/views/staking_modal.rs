//! Staking operations modal.
//!
//! Modal dialog for performing staking operations like bond, unbond, etc.

use gpui::prelude::*;
use gpui::*;
use gpui_ui_kit::theme::ThemeExt;
use gpui_ui_kit::*;

use crate::app::{StakingOperation, StkoptApp};

/// Staking modal component.
pub struct StakingModal;

impl StakingModal {
    pub fn render(app: &mut StkoptApp, cx: &mut Context<StkoptApp>) -> impl IntoElement {
        let theme = cx.theme();
        let entity = app.entity.clone();
        let operation = app.staking_operation;

        div()
            .id("staking-modal-overlay")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .id("staking-modal-bg")
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x00000088))
                    .on_mouse_down(MouseButton::Left, {
                        let entity = entity.clone();
                        move |_event, _window, cx| {
                            entity.update(cx, |this, cx| {
                                this.show_staking_modal = false;
                                cx.notify();
                            });
                        }
                    }),
            )
            .child(
                div()
                    .id("staking-modal-content")
                    .relative()
                    .w(px(450.0))
                    .bg(theme.surface)
                    .rounded_lg()
                    .border_1()
                    .border_color(theme.border)
                    .shadow_lg()
                    .child(Self::render_header(operation, cx))
                    .child(Self::render_body(app, cx))
                    .child(Self::render_footer(app, cx)),
            )
    }

    fn render_header(operation: StakingOperation, cx: &Context<StkoptApp>) -> impl IntoElement {
        let theme = cx.theme();

        div()
            .flex()
            .items_center()
            .justify_between()
            .p_4()
            .border_b_1()
            .border_color(theme.border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(Text::new(Self::operation_icon(operation)).size(TextSize::Lg))
                    .child(Heading::h2(operation.label()).into_any_element()),
            )
            .child(
                Text::new("Press Esc to close")
                    .size(TextSize::Sm)
                    .color(theme.text_secondary),
            )
    }

    fn render_body(app: &StkoptApp, cx: &Context<StkoptApp>) -> impl IntoElement {
        let theme = cx.theme();
        let entity = app.entity.clone();
        let operation = app.staking_operation;
        let symbol = app.token_symbol();
        let decimals = app.token_decimals();

        let mut body = div().flex().flex_col().gap_4().p_4();

        // Show balance info
        if let Some(ref info) = app.staking_info {
            let available = format_balance(info.transferable, symbol, decimals);
            let bonded = format_balance(info.bonded, symbol, decimals);

            body = body.child(
                div()
                    .flex()
                    .justify_between()
                    .child(
                        Text::new("Available Balance:")
                            .size(TextSize::Sm)
                            .color(theme.text_secondary),
                    )
                    .child(Text::new(available).size(TextSize::Sm)),
            );

            body = body.child(
                div()
                    .flex()
                    .justify_between()
                    .child(
                        Text::new("Currently Bonded:")
                            .size(TextSize::Sm)
                            .color(theme.text_secondary),
                    )
                    .child(Text::new(bonded).size(TextSize::Sm)),
            );
        }

        // Amount input for operations that require it
        if operation.requires_amount() {
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        Text::new("Amount")
                            .size(TextSize::Sm)
                            .color(theme.text_secondary),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                Input::new("staking-amount-input")
                                    .placeholder("0.0")
                                    .size(InputSize::Md)
                                    .value(app.staking_amount_input.clone())
                                    .on_text_change({
                                        let entity = entity.clone();
                                        move |value: String, _window, cx| {
                                            entity.update(cx, |this, cx| {
                                                this.staking_amount_input = value;
                                                cx.notify();
                                            });
                                        }
                                    }),
                            )
                            .child(Text::new(symbol).size(TextSize::Md)),
                    ),
            );
        }

        // SetPayee: reward destination picker
        if operation == StakingOperation::SetPayee {
            let current_dest = &app.rewards_destination;
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        Text::new("Reward Destination")
                            .size(TextSize::Sm)
                            .color(theme.text_secondary),
                    )
                    .child(Self::render_payee_option(
                        "Staked",
                        "Rewards are automatically restaked (compounding)",
                        matches!(current_dest, stkopt_chain::RewardDestination::Staked),
                        &theme,
                        {
                            let entity = entity.clone();
                            move |_window, cx| {
                                entity.update(cx, |this, cx| {
                                    this.rewards_destination =
                                        stkopt_chain::RewardDestination::Staked;
                                    cx.notify();
                                });
                            }
                        },
                    ))
                    .child(Self::render_payee_option(
                        "Stash",
                        "Rewards sent to stash account (not restaked)",
                        matches!(current_dest, stkopt_chain::RewardDestination::Stash),
                        &theme,
                        {
                            let entity = entity.clone();
                            move |_window, cx| {
                                entity.update(cx, |this, cx| {
                                    this.rewards_destination =
                                        stkopt_chain::RewardDestination::Stash;
                                    cx.notify();
                                });
                            }
                        },
                    ))
                    .child(Self::render_payee_option(
                        "None",
                        "Rewards are burned (not recommended)",
                        matches!(current_dest, stkopt_chain::RewardDestination::None),
                        &theme,
                        {
                            let entity = entity.clone();
                            move |_window, cx| {
                                entity.update(cx, |this, cx| {
                                    this.rewards_destination =
                                        stkopt_chain::RewardDestination::None;
                                    cx.notify();
                                });
                            }
                        },
                    )),
            );
        }

        // Operation description
        body = body.child(
            div().p_3().rounded_md().bg(rgba(0x3b82f620)).child(
                Text::new(Self::operation_description(operation))
                    .size(TextSize::Sm)
                    .color(theme.text_secondary),
            ),
        );

        body
    }

    fn render_payee_option<F>(
        label: &'static str,
        description: &'static str,
        selected: bool,
        theme: &gpui_ui_kit::theme::Theme,
        on_click: F,
    ) -> Stateful<Div>
    where
        F: Fn(&mut Window, &mut App) + 'static,
    {
        let border = if selected { theme.accent } else { theme.border };
        let bg = if selected {
            theme.surface_hover
        } else {
            theme.surface
        };

        div()
            .id(SharedString::from(format!("payee-{}", label)))
            .flex()
            .items_center()
            .gap_3()
            .p_3()
            .rounded_md()
            .border_1()
            .border_color(border)
            .bg(bg)
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, move |_event, window, cx| {
                on_click(window, cx);
            })
            .child(
                div()
                    .w(px(16.0))
                    .h(px(16.0))
                    .rounded_full()
                    .border_2()
                    .border_color(border)
                    .when(selected, |s| {
                        s.child(
                            div()
                                .w(px(8.0))
                                .h(px(8.0))
                                .m(px(2.0))
                                .rounded_full()
                                .bg(theme.accent),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        Text::new(label)
                            .size(TextSize::Sm)
                            .weight(TextWeight::Medium),
                    )
                    .child(
                        Text::new(description)
                            .size(TextSize::Xs)
                            .color(theme.text_secondary),
                    ),
            )
    }

    fn render_footer(app: &mut StkoptApp, cx: &mut Context<StkoptApp>) -> impl IntoElement {
        let theme = cx.theme();
        let entity = app.entity.clone();
        let operation = app.staking_operation;
        let amount_str = app.staking_amount_input.clone();
        let has_amount = !amount_str.is_empty() || !operation.requires_amount();

        div()
            .flex()
            .items_center()
            .justify_end()
            .gap_3()
            .p_4()
            .border_t_1()
            .border_color(theme.border)
            .child(
                Button::new("btn-cancel", "Cancel")
                    .variant(ButtonVariant::Secondary)
                    .on_click({
                        let entity = entity.clone();
                        move |_window, cx| {
                            entity.update(cx, |this, cx| {
                                this.show_staking_modal = false;
                                cx.notify();
                            });
                        }
                    }),
            )
            .child(
                Button::new("btn-generate-qr", "Generate QR")
                    .variant(ButtonVariant::Primary)
                    .disabled(!has_amount)
                    .on_click({
                        let entity = entity.clone();
                        move |_window, cx| {
                            entity.update(cx, |this, cx| {
                                this.generate_staking_qr(cx);
                            });
                        }
                    }),
            )
    }

    fn operation_icon(operation: StakingOperation) -> &'static str {
        match operation {
            StakingOperation::Bond => "🔒",
            StakingOperation::Unbond => "🔓",
            StakingOperation::BondExtra => "➕",
            StakingOperation::Rebond => "🔄",
            StakingOperation::WithdrawUnbonded => "💸",
            StakingOperation::Nominate => "✓",
            StakingOperation::Chill => "❄️",
            StakingOperation::ClaimRewards => "🎁",
            StakingOperation::SetPayee => "⚙️",
        }
    }

    fn operation_description(operation: StakingOperation) -> &'static str {
        match operation {
            StakingOperation::Bond => {
                "Lock tokens for staking. Bonded tokens cannot be transferred until unbonded."
            }
            StakingOperation::Unbond => {
                "Start unbonding tokens. They will be available to withdraw after the unbonding period."
            }
            StakingOperation::BondExtra => "Add more tokens to your existing stake.",
            StakingOperation::Rebond => "Re-bond tokens that are currently unbonding.",
            StakingOperation::WithdrawUnbonded => {
                "Withdraw tokens that have completed the unbonding period."
            }
            StakingOperation::Nominate => "Select validators to nominate with your bonded stake.",
            StakingOperation::Chill => "Stop nominating and remove your nominations.",
            StakingOperation::ClaimRewards => "Claim pending staking rewards.",
            StakingOperation::SetPayee => {
                "Change where your staking rewards are sent. 'Staked' compounds rewards automatically."
            }
        }
    }
}

fn format_balance(amount: u128, symbol: &str, decimals: u8) -> String {
    let divisor = 10u128.pow(decimals as u32);
    let frac_divisor = 10u128.pow(decimals.saturating_sub(4) as u32);
    let whole = amount / divisor;
    let frac = (amount % divisor) / frac_divisor;
    format!("{}.{:04} {}", whole, frac, symbol)
}
