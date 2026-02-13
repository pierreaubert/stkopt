//! Account section view - account management and details.

use gpui::prelude::*;
use gpui::*;
use gpui_ui_kit::theme::ThemeExt;
use gpui_ui_kit::*;
use tokio::sync::mpsc;

use crate::account::{ValidationResult, validate_address};
use crate::app::{ConnectionStatus, StkoptApp};
use crate::chain::ChainUpdate;
use crate::gpui_tokio::Tokio;

pub struct AccountSection;

impl AccountSection {
    pub fn render(app: &StkoptApp, cx: &Context<StkoptApp>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_6()
            .child(Heading::h1("Account"))
            .child(Self::render_account_input(app, cx))
            .child(Self::render_account_details(app, cx))
            .child(Self::render_address_book(app, cx))
    }

    fn render_account_input(app: &StkoptApp, cx: &Context<StkoptApp>) -> impl IntoElement {
        let theme = cx.theme();
        let entity = app.entity.clone();

        let mut content = div()
            .flex()
            .flex_col()
            .gap_4()
            .child(Heading::h3("Watch Account"))
            .child(
                Text::new("Enter a Polkadot address to monitor staking activity")
                    .size(TextSize::Sm)
                    .color(theme.text_secondary),
            )
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(
                        Input::new("account-input")
                            .placeholder("Enter address (e.g., 15oF4u...)")
                            .size(InputSize::Md)
                            .value(app.account_input.clone())
                            .on_text_change({
                                let entity = entity.clone();
                                move |value: String, _window, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.account_input = value;
                                        this.account_error = None;
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .child(
                        Button::new("btn-clear", "Clear")
                            .variant(ButtonVariant::Secondary)
                            .on_click({
                                let entity = entity.clone();
                                move |_window, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.account_input.clear();
                                        this.account_error = None;
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .child(
                        Button::new("btn-watch", "Watch")
                            .variant(ButtonVariant::Primary)
                            .on_click({
                                let entity = entity.clone();
                                move |_window, cx| {
                                    entity.update(cx, |this, cx| {
                                        let input = this.account_input.clone();

                                        match validate_address(&input) {
                                            ValidationResult::Valid(_addr_type) => {
                                                this.watched_account = Some(input.clone());
                                                this.account_error = None;
                                                this.add_to_address_book(input.clone());
                                                this.save_config();

                                                // Fetch account data if connected
                                                if this.connection_status == ConnectionStatus::Connected
                                                    && let Some(ref handle) = this.chain_handle
                                                {
                                                    let handle = handle.clone();
                                                    let address = input.clone();
                                                    let entity = this.entity.clone();
                                                    let mut async_cx = cx.to_async();

                                                    cx.spawn(move |_this: gpui::WeakEntity<StkoptApp>, _cx: &mut gpui::AsyncApp| async move {
                                                        let result = handle.fetch_account(address).await;
                                                        let _ = entity.update(&mut async_cx, |this, cx: &mut Context<StkoptApp>| {
                                                            match result {
                                                                Ok(account_data) => {
                                                                    this.apply_chain_update(ChainUpdate::AccountLoaded(account_data), cx);
                                                                }
                                                                Err(e) => {
                                                                    tracing::error!("Failed to fetch account: {}", e);
                                                                    this.connection_error = Some(format!("Failed to fetch account: {}", e));
                                                                    cx.notify();
                                                                }
                                                            }
                                                        });
                                                    }).detach();
                                                }

                                                this.current_section = crate::app::Section::Dashboard;
                                            }
                                            ValidationResult::Invalid(msg) => {
                                                this.account_error = Some(msg);
                                            }
                                            ValidationResult::Empty => {
                                                this.account_error = Some("Please enter an address".to_string());
                                            }
                                        }
                                        cx.notify();
                                    });
                                }
                            }),
                    ),
            );

        // Show error message if present
        if let Some(ref error) = app.account_error {
            content = content.child(
                Text::new(error.clone())
                    .size(TextSize::Sm)
                    .color(theme.error),
            );
        }

        Card::new().content(content)
    }

    fn render_account_details(app: &StkoptApp, cx: &Context<StkoptApp>) -> AnyElement {
        let theme = cx.theme();

        if let Some(ref address) = app.watched_account {
            let addr_display = truncate_address(address);
            let network_label = app.network.label();
            let status = if app.staking_info.as_ref().is_some_and(|i| i.is_nominating) {
                "Nominating"
            } else {
                "Not Nominating"
            };

            Card::new()
                .content(
                    div()
                        .flex()
                        .flex_col()
                        .gap_4()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(Heading::h3("Watched Account"))
                                .child(Badge::new("Active").variant(BadgeVariant::Success)),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .child(detail_row("Address", addr_display, &theme))
                                .child(detail_row("Network", network_label.to_string(), &theme))
                                .child(detail_row("Status", status.to_string(), &theme)),
                        ),
                )
                .into_any_element()
        } else {
            // Return empty element instead of "No account selected" message
            div().into_any_element()
        }
    }

    fn render_address_book(app: &StkoptApp, cx: &Context<StkoptApp>) -> AnyElement {
        let theme = cx.theme();
        let entity = app.entity.clone();

        // Filter address book entries for current network
        let entries: Vec<_> = app
            .address_book
            .iter()
            .filter(|a| a.network == app.network)
            .collect();

        if entries.is_empty() {
            return div().into_any_element();
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
                    div().flex_1().child(
                        Text::new("Address")
                            .size(TextSize::Sm)
                            .weight(TextWeight::Semibold),
                    ),
                )
                .child(
                    div().w(px(100.0)).child(
                        Text::new("Network")
                            .size(TextSize::Sm)
                            .weight(TextWeight::Semibold),
                    ),
                )
                .child(
                    div().w(px(80.0)).child(
                        Text::new("Actions")
                            .size(TextSize::Sm)
                            .weight(TextWeight::Semibold),
                    ),
                ),
        );

        // Address rows
        for (i, entry) in entries.iter().enumerate() {
            let row_bg = if i % 2 == 0 {
                theme.background
            } else {
                theme.surface
            };
            let address = entry.address.clone();
            let is_active = app.watched_account.as_ref() == Some(&address);

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
                            .flex_1()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                Text::new(truncate_address(&address))
                                    .size(TextSize::Sm)
                                    .weight(if is_active { TextWeight::Semibold } else { TextWeight::Normal }),
                            )
                            .when(is_active, |el| {
                                el.child(Badge::new("watching").variant(BadgeVariant::Success).size(BadgeSize::Sm))
                            }),
                    )
                    .child(
                        div()
                            .w(px(100.0))
                            .child(Text::new(entry.network.symbol()).size(TextSize::Sm).color(theme.text_secondary)),
                    )
                    .child(
                        div()
                            .w(px(80.0))
                            .flex()
                            .gap_2()
                            .child({
                                let entity = entity.clone();
                                let addr = address.clone();
                                Button::new(SharedString::from(format!("watch-{}", i)), "Watch")
                                    .variant(ButtonVariant::Secondary)
                                    .size(ButtonSize::Sm)
                                    .on_click(move |_window, cx| {
                                        let addr = addr.clone();
                                        entity.update(cx, |this, cx| {
                                            this.watched_account = Some(addr.clone());
                                            this.account_input = addr.clone();
                                            this.save_config();

                                            // Fetch account data if connected
                                            if this.connection_status == ConnectionStatus::Connected
                                                && let Some(ref handle) = this.chain_handle
                                            {
                                                let handle = handle.clone();
                                                let entity = this.entity.clone();
                                                let mut async_cx = cx.to_async();

                                                cx.spawn(move |_this: gpui::WeakEntity<StkoptApp>, _cx: &mut gpui::AsyncApp| async move {
                                                    let result = handle.fetch_account(addr).await;
                                                    let _ = entity.update(&mut async_cx, |this, cx: &mut Context<StkoptApp>| {
                                                        match result {
                                                            Ok(account_data) => {
                                                                this.apply_chain_update(ChainUpdate::AccountLoaded(account_data), cx);
                                                            }
                                                            Err(e) => {
                                                                tracing::error!("Failed to fetch account: {}", e);
                                                                this.connection_error = Some(format!("Failed to fetch account: {}", e));
                                                                cx.notify();
                                                            }
                                                        }
                                                    });
                                                }).detach();
                                            }

                                            this.current_section = crate::app::Section::Dashboard;
                                            cx.notify();
                                        });
                                    })
                            })
                            .child({
                                let entity = entity.clone();
                                let addr = address.clone();
                                Button::new(SharedString::from(format!("remove-{}", i)), "X")
                                    .variant(ButtonVariant::Secondary)
                                    .size(ButtonSize::Sm)
                                    .on_click(move |_window, cx| {
                                        let addr = addr.clone();
                                        entity.update(cx, |this, cx| {
                                            this.remove_from_address_book(&addr);
                                            cx.notify();
                                        });
                                    })
                            }),
                    ),
            );
        }

        Card::new()
            .content(
                div()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(Heading::h3("Saved Accounts"))
                    .child(list),
            )
            .into_any_element()
    }
}

fn detail_row(
    label: &'static str,
    value: String,
    theme: &gpui_ui_kit::theme::Theme,
) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .py_2()
        .border_b_1()
        .border_color(theme.border)
        .child(
            Text::new(label)
                .size(TextSize::Sm)
                .color(theme.text_secondary),
        )
        .child(
            Text::new(value)
                .size(TextSize::Sm)
                .weight(TextWeight::Medium),
        )
}

fn truncate_address(address: &str) -> String {
    if address.len() > 16 {
        format!("{}...{}", &address[..8], &address[address.len() - 8..])
    } else {
        address.to_string()
    }
}

/// Spawns a background loop that processes chain updates and updates the UI.
/// Uses Tokio::spawn to receive updates and push them to a shared queue.
#[allow(dead_code)]
fn spawn_chain_update_loop(
    cx: &mut Context<StkoptApp>,
    entity: WeakEntity<StkoptApp>,
    update_rx: mpsc::Receiver<ChainUpdate>,
) {
    // Get a clone of the pending_updates queue from the entity
    let pending_updates = if let Some(strong) = entity.upgrade() {
        strong.read(cx).pending_updates.clone()
    } else {
        return;
    };

    // Spawn a Tokio task that receives updates and pushes them to the shared queue
    Tokio::spawn(cx, async move {
        let mut rx = update_rx;
        while let Some(update) = rx.recv().await {
            tracing::info!("Chain update received: {:?}", update);
            // Push update to the shared queue
            if let Ok(mut queue) = pending_updates.lock() {
                queue.push(update);
            }
            // Note: We can't call cx.notify() from here, but the render loop
            // will pick up pending updates on the next frame
        }
        tracing::info!("Chain update channel closed");
    })
    .detach();
}
