//! Pool operations modal.
//!
//! Modal dialog for performing nomination pool operations.

use gpui::prelude::*;
use gpui::*;
use gpui_ui_kit::theme::ThemeExt;
use gpui_ui_kit::*;

use crate::app::{PoolOperation, StkoptApp};

/// Pool modal component.
pub struct PoolModal;

impl PoolModal {
    pub fn render(app: &mut StkoptApp, cx: &mut Context<StkoptApp>) -> impl IntoElement {
        let theme = cx.theme();
        let entity = app.entity.clone();
        let operation = app.pool_operation;

        div()
            .id("pool-modal-overlay")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .id("pool-modal-bg")
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x00000088))
                    .on_mouse_down(MouseButton::Left, {
                        let entity = entity.clone();
                        move |_event, _window, cx| {
                            entity.update(cx, |this, cx| {
                                this.show_pool_modal = false;
                                cx.notify();
                            });
                        }
                    }),
            )
            .child(
                div()
                    .id("pool-modal-content")
                    .relative()
                    .w(px(450.0))
                    .bg(theme.surface)
                    .rounded_lg()
                    .border_1()
                    .border_color(theme.border)
                    .shadow_lg()
                    .child(Self::render_header(operation, app, cx))
                    .child(Self::render_body(app, cx))
                    .child(Self::render_footer(app, cx)),
            )
    }

    fn render_header(
        operation: PoolOperation,
        app: &StkoptApp,
        cx: &Context<StkoptApp>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let pool_info = if let Some(id) = app.selected_pool_id {
            format!("Pool #{}", id)
        } else {
            "Nomination Pool".to_string()
        };

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
                Text::new(pool_info)
                    .size(TextSize::Sm)
                    .color(theme.text_secondary),
            )
    }

    fn render_body(app: &StkoptApp, cx: &Context<StkoptApp>) -> impl IntoElement {
        let theme = cx.theme();
        let entity = app.entity.clone();
        let operation = app.pool_operation;
        let symbol = app.token_symbol();
        let decimals = app.token_decimals();

        // Get available balance for validation
        let available_balance = app
            .staking_info
            .as_ref()
            .map(|i| i.transferable)
            .unwrap_or(0);
        let divisor = 10u128.pow(decimals as u32);

        // Validate amount
        let amount_error = if operation.requires_amount() && !app.pool_amount_input.is_empty() {
            match app.pool_amount_input.parse::<f64>() {
                Ok(val) if val <= 0.0 => Some("Amount must be greater than 0"),
                Ok(val) => {
                    let amount_planck = (val * divisor as f64) as u128;
                    if amount_planck > available_balance {
                        Some("Insufficient balance")
                    } else {
                        None
                    }
                }
                Err(_) => Some("Invalid amount format"),
            }
        } else {
            None
        };

        let mut body = div().flex().flex_col().gap_4().p_4();

        // Show balance info
        if let Some(ref info) = app.staking_info {
            let available = format_balance(info.transferable, symbol, decimals);

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
        }

        // Show selected pool info for Join operation
        if operation == PoolOperation::Join
            && let Some(pool_id) = app.selected_pool_id
            && let Some(pool) = app.pools.iter().find(|p| p.id == pool_id)
        {
            body = body.child(
                div()
                    .p_3()
                    .rounded_md()
                    .bg(theme.background)
                    .border_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(Text::new(pool.name.clone()).size(TextSize::Md))
                            .child(
                                Text::new(format!("{} members", pool.member_count))
                                    .size(TextSize::Sm)
                                    .color(theme.text_secondary),
                            ),
                    ),
            );
        }

        // Amount input for operations that require it
        if operation.requires_amount() {
            let error_color = theme.error;

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
                                Input::new("pool-amount-input")
                                    .placeholder("0.0")
                                    .size(InputSize::Md)
                                    .value(app.pool_amount_input.clone())
                                    .on_text_change({
                                        let entity = entity.clone();
                                        move |value: String, _window, cx| {
                                            entity.update(cx, |this, cx| {
                                                this.pool_amount_input = value;
                                                cx.notify();
                                            });
                                        }
                                    }),
                            )
                            .child(Text::new(symbol).size(TextSize::Md)),
                    )
                    .when_some(amount_error, |div, error| {
                        div.child(Text::new(error).size(TextSize::Sm).color(error_color))
                    }),
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

    fn render_footer(app: &mut StkoptApp, cx: &mut Context<StkoptApp>) -> impl IntoElement {
        let theme = cx.theme();
        let entity = app.entity.clone();
        let operation = app.pool_operation;
        let amount_str = app.pool_amount_input.clone();
        let has_amount = !amount_str.is_empty() || !operation.requires_amount();
        let has_pool = app.selected_pool_id.is_some() || operation != PoolOperation::Join;

        div()
            .flex()
            .items_center()
            .justify_end()
            .gap_3()
            .p_4()
            .border_t_1()
            .border_color(theme.border)
            .child(
                Button::new("btn-cancel-pool", "Cancel")
                    .variant(ButtonVariant::Secondary)
                    .on_click({
                        let entity = entity.clone();
                        move |_window, cx| {
                            entity.update(cx, |this, cx| {
                                this.show_pool_modal = false;
                                cx.notify();
                            });
                        }
                    }),
            )
            .child(
                Button::new("btn-generate-pool-qr", "Generate QR")
                    .variant(ButtonVariant::Primary)
                    .disabled(!has_amount || !has_pool)
                    .on_click({
                        let entity = entity.clone();
                        move |_window, cx| {
                            entity.update(cx, |this, cx| {
                                this.generate_pool_qr(cx);
                            });
                        }
                    }),
            )
    }

    fn operation_icon(operation: PoolOperation) -> &'static str {
        match operation {
            PoolOperation::Join => "🏊",
            PoolOperation::BondExtra => "➕",
            PoolOperation::ClaimPayout => "🎁",
            PoolOperation::Unbond => "🔓",
            PoolOperation::Withdraw => "💸",
        }
    }

    fn operation_description(operation: PoolOperation) -> &'static str {
        match operation {
            PoolOperation::Join => {
                "Join this nomination pool with the specified amount. Your stake will be managed by the pool."
            }
            PoolOperation::BondExtra => "Add more tokens to your existing pool stake.",
            PoolOperation::ClaimPayout => "Claim your pending pool rewards.",
            PoolOperation::Unbond => {
                "Start unbonding tokens from the pool. They will be available to withdraw after the unbonding period."
            }
            PoolOperation::Withdraw => "Withdraw tokens that have completed the unbonding period.",
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
