//! QR code modal for transaction signing.
//!
//! Three-tab modal: QR Code display | Scan Signature | Submit

use gpui::prelude::*;
use gpui::*;
use gpui_ui_kit::theme::ThemeExt;
use gpui_ui_kit::*;
use qrcode::{EcLevel, QrCode, Version};

use crate::app::{QrModalTab, StkoptApp};

/// QR modal component.
pub struct QrModal;

impl QrModal {
    pub fn render(app: &mut StkoptApp, cx: &mut Context<StkoptApp>) -> impl IntoElement {
        let theme = cx.theme();
        let entity = app.entity.clone();

        div()
            .id("qr-modal-overlay")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .id("qr-modal-bg")
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x00000088))
                    .on_mouse_down(MouseButton::Left, {
                        let entity = entity.clone();
                        move |_event, _window, cx| {
                            entity.update(cx, |this, cx| {
                                this.show_qr_modal = false;
                                this.pending_tx_payload = None;
                                this.stop_camera(cx);
                                cx.notify();
                            });
                        }
                    }),
            )
            .child(
                div()
                    .id("qr-modal-content")
                    .relative()
                    .w(px(500.0))
                    .bg(theme.surface)
                    .rounded_lg()
                    .border_1()
                    .border_color(theme.border)
                    .shadow_lg()
                    .child(Self::render_header(app, cx))
                    .child(Self::render_tabs(app, cx))
                    .child(Self::render_content(app, cx))
                    .child(Self::render_footer(app, cx)),
            )
    }

    fn render_header(app: &StkoptApp, cx: &Context<StkoptApp>) -> impl IntoElement {
        let theme = cx.theme();

        let description = app
            .pending_tx_payload
            .as_ref()
            .map(|p| p.description.clone())
            .unwrap_or_else(|| "Transaction".to_string());

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
                    .child(Text::new("📱").size(TextSize::Lg))
                    .child(Heading::h2("Sign Transaction").into_any_element()),
            )
            .child(
                Text::new(description)
                    .size(TextSize::Sm)
                    .color(theme.text_secondary),
            )
    }

    fn render_tabs(app: &mut StkoptApp, cx: &mut Context<StkoptApp>) -> impl IntoElement {
        let theme = cx.theme();
        let entity = app.entity.clone();
        let current_tab = app.qr_modal_tab;

        div()
            .flex()
            .border_b_1()
            .border_color(theme.border)
            .children(QrModalTab::all().iter().map(|tab| {
                let is_selected = *tab == current_tab;
                let tab_value = *tab;

                div()
                    .id(SharedString::from(format!("qr-tab-{:?}", tab)))
                    .flex_1()
                    .px_4()
                    .py_3()
                    .cursor_pointer()
                    .text_center()
                    .border_b_2()
                    .border_color(if is_selected {
                        theme.accent
                    } else {
                        rgba(0x00000000)
                    })
                    .bg(if is_selected {
                        rgba(0x3b82f610)
                    } else {
                        rgba(0x00000000)
                    })
                    .on_mouse_down(MouseButton::Left, {
                        let entity = entity.clone();
                        move |_event, _window, cx| {
                            entity.update(cx, |this, cx| {
                                this.qr_modal_tab = tab_value;
                                cx.notify();
                            });
                        }
                    })
                    .child(
                        Text::new(tab.label())
                            .size(TextSize::Sm)
                            .color(if is_selected {
                                theme.text_primary
                            } else {
                                theme.text_secondary
                            }),
                    )
            }))
    }

    fn render_content(app: &StkoptApp, cx: &Context<StkoptApp>) -> impl IntoElement {
        let theme = cx.theme();

        let content = match app.qr_modal_tab {
            QrModalTab::QrCode => Self::render_qr_tab(app, cx),
            QrModalTab::ScanSignature => Self::render_scan_tab(app, cx),
            QrModalTab::Submit => Self::render_submit_tab(app, cx),
        };

        div()
            .min_h(px(300.0))
            .p_4()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .bg(theme.background)
            .child(content)
    }

    fn render_qr_tab(app: &StkoptApp, cx: &Context<StkoptApp>) -> Div {
        let theme = cx.theme();

        if let Some(ref payload) = app.pending_tx_payload {
            let qr_data = &payload.qr_data;

            // Try to generate QR code with different versions
            let qr_result = Self::generate_qr_code(qr_data);

            match qr_result {
                Ok(qr_element) => div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_4()
                    .child(qr_element)
                    .child(
                        Text::new("Scan this QR code with Polkadot Vault")
                            .size(TextSize::Sm)
                            .color(theme.text_secondary),
                    )
                    .child(
                        Text::new(format!("({} bytes)", qr_data.len()))
                            .size(TextSize::Xs)
                            .color(theme.text_secondary),
                    ),
                Err(e) => div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_4()
                    .child(
                        div()
                            .w(px(250.0))
                            .h(px(250.0))
                            .bg(gpui::rgb(0xffffff))
                            .rounded_lg()
                            .border_1()
                            .border_color(theme.border)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                Text::new(format!("QR Error:\n{}", e))
                                    .size(TextSize::Xs)
                                    .color(gpui::rgb(0xcc0000)),
                            ),
                    )
                    .child(
                        Text::new(format!("Payload size: {} bytes", qr_data.len()))
                            .size(TextSize::Sm)
                            .color(theme.text_secondary),
                    ),
            }
        } else {
            div()
                .flex()
                .items_center()
                .justify_center()
                .child(Text::new("No transaction payload").color(theme.text_secondary))
        }
    }

    /// Generate a QR code element from binary data.
    fn generate_qr_code(data: &[u8]) -> Result<Div, String> {
        // Try different QR versions to find one that fits
        let qr_versions = [
            Version::Normal(15),
            Version::Normal(20),
            Version::Normal(25),
            Version::Normal(30),
            Version::Normal(35),
            Version::Normal(40),
        ];

        let mut last_error = String::new();
        for version in qr_versions {
            match QrCode::with_version(data, version, EcLevel::L) {
                Ok(qr) => {
                    return Ok(Self::render_qr_grid(&qr));
                }
                Err(e) => {
                    last_error = format!("{}", e);
                }
            }
        }

        Err(format!("Data too large for QR: {}", last_error))
    }

    /// Render a QR code as a grid of colored divs.
    fn render_qr_grid(qr: &QrCode) -> Div {
        let modules = qr.to_colors();
        let size = qr.width();

        // Calculate module size - aim for ~250px total with quiet zone
        let quiet_zone = 4; // Standard QR quiet zone
        let total_modules = size + (quiet_zone * 2);
        let module_size = (250.0 / total_modules as f64).floor().max(2.0);
        let total_size = module_size * total_modules as f64;

        let white = gpui::rgb(0xffffff);
        let black = gpui::rgb(0x000000);

        let mut rows = div()
            .flex()
            .flex_col()
            .bg(white)
            .p(px(module_size as f32 * quiet_zone as f32));

        // Build rows
        for y in 0..size {
            let mut row = div().flex();
            for x in 0..size {
                let idx = y * size + x;
                let is_dark = modules
                    .get(idx)
                    .map(|c| *c == qrcode::Color::Dark)
                    .unwrap_or(false);
                let color = if is_dark { black } else { white };

                row = row.child(
                    div()
                        .w(px(module_size as f32))
                        .h(px(module_size as f32))
                        .bg(color),
                );
            }
            rows = rows.child(row);
        }

        div()
            .w(px(total_size as f32))
            .h(px(total_size as f32))
            .rounded_lg()
            .overflow_hidden()
            .border_1()
            .border_color(gpui::rgb(0xdddddd))
            .child(rows)
    }

    fn render_scan_tab(app: &StkoptApp, cx: &Context<StkoptApp>) -> Div {
        let theme = cx.theme();
        let entity = app.entity.clone();
        let is_scanning = app.qr_reader.is_some();

        let mut content = div().flex().flex_col().items_center().gap_4();

        // Camera preview area
        let preview_area = if let Some(ref preview) = app.camera_preview {
            // Show actual camera preview (simplified - just show status)
            div()
                .w(px(320.0))
                .h(px(240.0))
                .bg(gpui::rgb(0x1a1a1a))
                .rounded_lg()
                .border_1()
                .border_color(if preview.qr_bounds.is_some() {
                    theme.success
                } else {
                    theme.border
                })
                .flex()
                .items_center()
                .justify_center()
                .child(
                    Text::new(if preview.qr_bounds.is_some() {
                        "QR Code Detected!"
                    } else {
                        "Scanning..."
                    })
                    .size(TextSize::Md)
                    .color(if preview.qr_bounds.is_some() {
                        theme.success
                    } else {
                        theme.text_secondary
                    }),
                )
        } else {
            // Placeholder when camera not started
            div()
                .w(px(320.0))
                .h(px(240.0))
                .bg(gpui::rgb(0x1a1a1a))
                .rounded_lg()
                .border_1()
                .border_color(theme.border)
                .flex()
                .items_center()
                .justify_center()
                .child(
                    Text::new("Camera Preview")
                        .size(TextSize::Sm)
                        .color(gpui::rgb(0x666666)),
                )
        };

        content = content.child(preview_area);

        content = content.child(
            Text::new("Scan the signature QR from Polkadot Vault")
                .size(TextSize::Sm)
                .color(theme.text_secondary),
        );

        // Camera control button
        if is_scanning {
            content = content.child(
                Button::new("btn-stop-camera", "Stop Camera")
                    .variant(ButtonVariant::Secondary)
                    .on_click({
                        let entity = entity.clone();
                        move |_window, cx| {
                            entity.update(cx, |this, cx| {
                                this.stop_camera(cx);
                            });
                        }
                    }),
            );
        } else {
            content = content.child(
                Button::new("btn-start-camera", "Start Camera")
                    .variant(ButtonVariant::Primary)
                    .on_click({
                        let entity = entity.clone();
                        move |_window, cx| {
                            entity.update(cx, |this, cx| {
                                this.start_camera(cx);
                            });
                        }
                    }),
            );
        }

        content
    }

    fn render_submit_tab(app: &StkoptApp, cx: &Context<StkoptApp>) -> Div {
        let theme = cx.theme();

        let mut content = div().flex().flex_col().items_center().gap_4();

        if let Some(ref status) = app.tx_status_message {
            content = content.child(
                div()
                    .p_4()
                    .rounded_lg()
                    .bg(rgba(0x22c55e20))
                    .child(Text::new(status.clone()).size(TextSize::Sm)),
            );
        } else {
            content = content.child(
                div().p_4().rounded_lg().bg(rgba(0xfbbf2420)).child(
                    Text::new("Scan the signed QR code first")
                        .size(TextSize::Sm)
                        .color(theme.text_secondary),
                ),
            );
        }

        content = content.child(
            Button::new("btn-submit-tx", "Submit Transaction")
                .variant(ButtonVariant::Primary)
                .disabled(app.tx_status_message.is_none()),
        );

        content
    }

    fn render_footer(app: &mut StkoptApp, cx: &mut Context<StkoptApp>) -> impl IntoElement {
        let theme = cx.theme();
        let entity = app.entity.clone();

        div()
            .flex()
            .items_center()
            .justify_between()
            .p_4()
            .border_t_1()
            .border_color(theme.border)
            .child(
                Text::new("Use Polkadot Vault for secure signing")
                    .size(TextSize::Xs)
                    .color(theme.text_secondary),
            )
            .child(
                Button::new("btn-close-qr", "Close")
                    .variant(ButtonVariant::Secondary)
                    .on_click({
                        let entity = entity.clone();
                        move |_window, cx| {
                            entity.update(cx, |this, cx| {
                                this.show_qr_modal = false;
                                this.pending_tx_payload = None;
                                this.stop_camera(cx);
                                cx.notify();
                            });
                        }
                    }),
            )
    }
}
