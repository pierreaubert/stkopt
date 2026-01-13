//! Optimization section view - validator selection optimization.

use gpui::prelude::*;
use gpui::*;
use gpui_ui_kit::theme::ThemeExt;
use gpui_ui_kit::*;

use crate::app::StkoptApp;
use crate::optimization::{optimize_selection, OptimizationCriteria, SelectionStrategy};

pub struct OptimizationSection;

impl OptimizationSection {
    pub fn render(app: &StkoptApp, cx: &Context<StkoptApp>) -> impl IntoElement {
        let theme = cx.theme();
        let entity = app.entity.clone();
        let entity2 = app.entity.clone();
        let selected_count = app.selected_validators.len();
        let validator_count = app.validators.len();
        let current_strategy = app.optimization_strategy;

        div()
            .flex()
            .flex_col()
            .gap_6()
            .child(Heading::h1("Optimization"))
            .child(
                Text::new("Automatically select validators based on your preferences")
                    .size(TextSize::Md)
                    .color(theme.text_secondary),
            )
            .child(
                div()
                    .flex()
                    .gap_4()
                    .child(Badge::new(format!("{} validators available", validator_count)))
                    .child(
                        Badge::new(format!("{} selected", selected_count))
                            .variant(if selected_count > 0 {
                                BadgeVariant::Success
                            } else {
                                BadgeVariant::Default
                            }),
                    ),
            )
            .child(
                Card::new().content(
                    div()
                        .flex()
                        .flex_col()
                        .gap_4()
                        .child(Heading::h3("Selection Strategy"))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_3()
                                .children(SelectionStrategy::all().iter().map(|strategy| {
                                    let strategy_value = *strategy;
                                    let is_selected = strategy_value == current_strategy;
                                    let entity = app.entity.clone();

                                    strategy_option_clickable(
                                        strategy.label(),
                                        strategy.description(),
                                        is_selected,
                                        &theme,
                                        move |_window, cx| {
                                            entity.update(cx, |this, cx| {
                                                this.optimization_strategy = strategy_value;
                                                cx.notify();
                                            });
                                        }
                                    )
                                })),
                        ),
                ),
            )
            .child(
                Card::new().content(
                    div()
                        .flex()
                        .flex_col()
                        .gap_4()
                        .child(Heading::h3("Parameters"))
                        .child(
                            div()
                                .flex()
                                .gap_4()
                                .child(param_field("Max Validators", &app.optimization_target_count.to_string(), &theme))
                                .child(param_field("Max Commission (%)", &format!("{:.0}", app.optimization_max_commission * 100.0), &theme)),
                        ),
                ),
            )
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(
                        Button::new("btn-optimize", "Run Optimization")
                            .variant(ButtonVariant::Primary)
                            .size(ButtonSize::Lg)
                            .disabled(validator_count == 0)
                            .on_click(move |_window, cx| {
                                entity.update(cx, |this, cx| {
                                    let criteria = OptimizationCriteria {
                                        max_commission: this.optimization_max_commission,
                                        exclude_blocked: true,
                                        target_count: this.optimization_target_count,
                                        strategy: this.optimization_strategy,
                                    };
                                    let result = optimize_selection(&this.validators, &criteria);
                                    this.selected_validators = result.selected_indices.into_iter().collect();
                                    this.optimization_result = Some(result.estimated_apy_avg);
                                    cx.notify();
                                });
                            }),
                    )
                    .child(
                        Button::new("btn-clear", "Clear Selection")
                            .variant(ButtonVariant::Secondary)
                            .size(ButtonSize::Lg)
                            .disabled(selected_count == 0)
                            .on_click(move |_window, cx| {
                                entity2.update(cx, |this, cx| {
                                    this.selected_validators.clear();
                                    this.optimization_result = None;
                                    this.qr_payload = None;
                                    cx.notify();
                                });
                            }),
                    )
                    .child(
                        Button::new("btn-generate-qr", "Generate QR Code")
                            .variant(ButtonVariant::Primary)
                            .size(ButtonSize::Lg)
                            .disabled(selected_count == 0)
                            .on_click({
                                let entity3 = app.entity.clone();
                                move |_window, cx| {
                                    entity3.update(cx, |this, cx| {
                                        // Get selected validator addresses
                                        let targets: Vec<String> = this.selected_validators
                                            .iter()
                                            .filter_map(|&idx| this.validators.get(idx))
                                            .map(|v| v.address.clone())
                                            .collect();
                                        
                                        if !targets.is_empty() {
                                            // Build nominate transaction
                                            let tx = crate::transactions::build_nominate_tx(
                                                &targets,
                                                0, // nonce (would come from chain)
                                                "0x91b171bb158e2d3848fa23a9f1c25182fb8e20313b2c1eb49219da7a70ce90c3", // Polkadot genesis
                                                "0x91b171bb158e2d3848fa23a9f1c25182fb8e20313b2c1eb49219da7a70ce90c3", // block hash
                                                1000000, // spec_version
                                                1, // tx_version
                                            );
                                            let qr = crate::transactions::QrPayload::for_signing(&tx);
                                            this.qr_payload = Some(qr.to_hex());
                                        }
                                        cx.notify();
                                    });
                                }
                            }),
                    ),
            )
            .child(Self::render_results(app, &theme))
            .child(Self::render_qr_section(app, &theme))
    }

    fn render_results(app: &StkoptApp, theme: &gpui_ui_kit::theme::Theme) -> AnyElement {
        if app.selected_validators.is_empty() {
            return div()
                .p_6()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    Text::new("Run optimization to select validators")
                        .size(TextSize::Sm)
                        .color(theme.text_secondary),
                )
                .into_any_element();
        }

        let mut list = div().flex().flex_col();

        // Header
        list = list.child(
            div()
                .flex()
                .items_center()
                .px_4()
                .py_3()
                .bg(theme.surface)
                .border_b_1()
                .border_color(theme.border)
                .child(div().w(px(40.0)).child(Text::new("#").size(TextSize::Sm).weight(TextWeight::Semibold)))
                .child(
                    div()
                        .flex_1()
                        .child(Text::new("Selected Validator").size(TextSize::Sm).weight(TextWeight::Semibold)),
                )
                .child(
                    div()
                        .w(px(100.0))
                        .child(Text::new("Commission").size(TextSize::Sm).weight(TextWeight::Semibold)),
                )
                .child(
                    div()
                        .w(px(80.0))
                        .child(Text::new("APY").size(TextSize::Sm).weight(TextWeight::Semibold)),
                ),
        );

        // Selected validator rows
        for (i, &idx) in app.selected_validators.iter().enumerate() {
            if let Some(validator) = app.validators.get(idx) {
                let name = validator.name.clone().unwrap_or_else(|| validator.address[..8].to_string());
                let commission_str = format!("{:.1}%", validator.commission * 100.0);
                let apy_str = validator.apy.map(|a| format!("{:.1}%", a)).unwrap_or_else(|| "-".to_string());
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
                                .w(px(40.0))
                                .child(Text::new(format!("{}", i + 1)).size(TextSize::Sm).color(theme.text_secondary)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .child(Text::new(name).size(TextSize::Sm)),
                        )
                        .child(
                            div()
                                .w(px(100.0))
                                .child(Text::new(commission_str).size(TextSize::Sm)),
                        )
                        .child(
                            div()
                                .w(px(80.0))
                                .child(Text::new(apy_str).size(TextSize::Sm).color(theme.success)),
                        ),
                );
            }
        }

        // Summary
        if let Some(avg_apy) = app.optimization_result {
            list = list.child(
                div()
                    .px_4()
                    .py_3()
                    .bg(theme.surface_hover)
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        Text::new(format!("{} validators selected", app.selected_validators.len()))
                            .size(TextSize::Sm)
                            .weight(TextWeight::Semibold),
                    )
                    .child(
                        Text::new(format!("Estimated avg APY: {:.1}%", avg_apy))
                            .size(TextSize::Sm)
                            .weight(TextWeight::Semibold)
                            .color(theme.success),
                    ),
            );
        }

        Card::new().content(list).into_any_element()
    }

    fn render_qr_section(app: &StkoptApp, theme: &gpui_ui_kit::theme::Theme) -> AnyElement {
        if let Some(ref payload) = app.qr_payload {
            Card::new()
                .content(
                    div()
                        .flex()
                        .flex_col()
                        .gap_4()
                        .child(Heading::h3("Transaction QR Code"))
                        .child(
                            Text::new("Scan with Polkadot Vault to sign the nominate transaction")
                                .size(TextSize::Sm)
                                .color(theme.text_secondary),
                        )
                        .child(
                            div()
                                .p_4()
                                .bg(theme.surface)
                                .rounded_md()
                                .border_1()
                                .border_color(theme.border)
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_2()
                                        .child(
                                            Text::new("QR Payload (hex):")
                                                .size(TextSize::Xs)
                                                .color(theme.text_secondary),
                                        )
                                        .child(
                                            div()
                                                .p_2()
                                                .bg(theme.background)
                                                .rounded_sm()
                                                .overflow_hidden()
                                                .child(
                                                    Text::new(if payload.len() > 64 {
                                                        format!("{}...", &payload[..64])
                                                    } else {
                                                        payload.clone()
                                                    })
                                                    .size(TextSize::Xs),
                                                ),
                                        )
                                        .child(
                                            Text::new(format!("Payload size: {} bytes", payload.len() / 2))
                                                .size(TextSize::Xs)
                                                .color(theme.text_secondary),
                                        ),
                                ),
                        )
                        .child(
                            Text::new("Note: QR code rendering requires gpui-px integration")
                                .size(TextSize::Xs)
                                .color(theme.text_secondary),
                        ),
                )
                .into_any_element()
        } else {
            div().into_any_element()
        }
    }
}

fn strategy_option_clickable<F>(
    title: &'static str,
    description: &'static str,
    selected: bool,
    theme: &gpui_ui_kit::theme::Theme,
    on_click: F,
) -> Stateful<Div>
where
    F: Fn(&mut Window, &mut App) + 'static,
{
    let border = if selected { theme.accent } else { theme.border };
    let bg = if selected { theme.surface_hover } else { theme.surface };

    div()
        .id(SharedString::from(format!("strategy-{}", title)))
        .flex()
        .items_start()
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
                .mt(px(2.0))
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
                .child(Text::new(title).size(TextSize::Sm).weight(TextWeight::Medium))
                .child(
                    Text::new(description)
                        .size(TextSize::Xs)
                        .color(theme.text_secondary),
                ),
        )
}

fn param_field(
    label: &'static str,
    value: &str,
    theme: &gpui_ui_kit::theme::Theme,
) -> impl IntoElement {
    let value = SharedString::from(value.to_string());
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            Text::new(label)
                .size(TextSize::Sm)
                .color(theme.text_secondary),
        )
        .child(
            div()
                .px_3()
                .py_2()
                .bg(theme.surface)
                .border_1()
                .border_color(theme.border)
                .rounded_md()
                .child(Text::new(value).size(TextSize::Sm)),
        )
}
