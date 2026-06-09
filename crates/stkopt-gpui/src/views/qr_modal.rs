//! QR code modal for transaction signing.
//!
//! Three-tab modal: QR Code display | Scan Signature | Submit

use gpui::prelude::*;
use gpui::*;
use gpui_ui_kit::theme::ThemeExt;
use gpui_ui_kit::*;
use qrcode::{EcLevel, QrCode, Version};

use crate::app::{QrModalTab, QrTxStatus, StkoptApp};

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
                    .bg(theme.overlay_bg)
                    .on_mouse_down(MouseButton::Left, {
                        let entity = entity.clone();
                        move |_event, _window, cx| {
                            entity.update(cx, |this, cx| {
                                this.close_qr_modal(cx);
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
                    .occlude()
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
                        theme.transparent
                    })
                    .bg(if is_selected {
                        theme.accent_muted
                    } else {
                        theme.transparent
                    })
                    .on_mouse_down(MouseButton::Left, {
                        let entity = entity.clone();
                        move |_event, _window, cx| {
                            entity.update(cx, |this, cx| {
                                this.set_qr_modal_tab(tab_value, cx);
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
            let qr_result = Self::generate_qr_code(qr_data, &theme);

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
                            .bg(theme.surface)
                            .rounded_lg()
                            .border_1()
                            .border_color(theme.border)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                Text::new(format!("QR Error:\n{}", e))
                                    .size(TextSize::Xs)
                                    .color(theme.error),
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
    fn generate_qr_code(data: &[u8], theme: &gpui_ui_kit::theme::Theme) -> Result<Div, String> {
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
                    return Ok(Self::render_qr_grid(&qr, theme));
                }
                Err(e) => {
                    last_error = format!("{}", e);
                }
            }
        }

        Err(format!("Data too large for QR: {}", last_error))
    }

    /// Render a QR code as a grid of colored divs.
    fn render_qr_grid(qr: &QrCode, theme: &gpui_ui_kit::theme::Theme) -> Div {
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
            .border_color(theme.border)
            .child(rows)
    }

    fn render_scan_tab(app: &StkoptApp, cx: &Context<StkoptApp>) -> Div {
        let theme = cx.theme();
        let entity = app.entity.clone();
        let is_scanning = app.qr_reader.is_some();

        let mut content = div().flex().flex_col().items_center().gap_4();

        // Camera preview area
        let preview_area = if let Some(ref preview) = app.camera_preview {
            Self::render_camera_preview(preview, &theme)
        } else {
            // Placeholder when camera not started
            div()
                .w(px(320.0))
                .h(px(240.0))
                .bg(theme.muted)
                .rounded_lg()
                .border_1()
                .border_color(theme.border)
                .flex()
                .items_center()
                .justify_center()
                .child(
                    Text::new("Camera Preview")
                        .size(TextSize::Sm)
                        .color(theme.text_muted),
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
                                this.stop_camera_with_reason("Stop Camera button", cx);
                            });
                        }
                    }),
            );
        } else {
            content = content.child(
                Button::new("btn-start-camera", "Start Camera")
                    .variant(ButtonVariant::Primary)
                    .theme(crate::theme::button_theme_for_ui_theme(&theme))
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

    fn render_camera_preview(
        preview: &crate::qr_reader::CameraPreview,
        theme: &gpui_ui_kit::theme::Theme,
    ) -> Div {
        let rgb_pixels = preview.rgb_pixels.clone();
        let width = preview.width;
        let height = preview.height;
        let qr_bounds = preview.qr_bounds;

        div()
            .w(px(320.0))
            .h(px(240.0))
            .bg(theme.muted)
            .rounded_lg()
            .border_1()
            .border_color(if qr_bounds.is_some() {
                theme.success
            } else {
                theme.border
            })
            .overflow_hidden()
            .child(
                canvas(
                    move |_, _, _| {},
                    move |bounds, _, window, _| {
                        paint_camera_preview(bounds, &rgb_pixels, width, height, window);
                        if let Some(qr_bounds) = qr_bounds {
                            paint_qr_bounds(bounds, qr_bounds, window);
                        }
                    },
                )
                .size_full(),
            )
    }

    fn render_submit_tab(app: &StkoptApp, cx: &Context<StkoptApp>) -> Div {
        let theme = cx.theme();
        let entity = app.entity.clone();

        let mut content = div().flex().flex_col().items_center().gap_4();

        if let Some(ref status) = app.tx_status_message {
            let bg = match app.tx_status {
                QrTxStatus::Ready | QrTxStatus::Submitting | QrTxStatus::Submitted => {
                    theme.success_token().subtle
                }
                QrTxStatus::NotReady | QrTxStatus::Failed => theme.warning_token().subtle,
            };

            content = content.child(
                div().p_4().rounded_lg().bg(bg).child(
                    Text::new(status.clone())
                        .size(TextSize::Sm)
                        .color(theme.text_primary),
                ),
            );
        } else {
            content = content.child(
                div()
                    .p_4()
                    .rounded_lg()
                    .bg(theme.warning_token().subtle)
                    .child(
                        Text::new("Scan the signed QR code first")
                            .size(TextSize::Sm)
                            .color(theme.text_secondary),
                    ),
            );
        }

        content = content.child(
            Button::new("btn-submit-tx", "Submit Transaction")
                .variant(ButtonVariant::Primary)
                .theme(crate::theme::button_theme_for_ui_theme(&theme))
                .disabled(
                    app.signed_extrinsic.is_none()
                        || app.tx_status == QrTxStatus::Submitting
                        || app.tx_status == QrTxStatus::Submitted,
                )
                .on_click({
                    let entity = entity.clone();
                    move |_window, cx| {
                        entity.update(cx, |this, cx| {
                            this.submit_scanned_transaction(cx);
                        });
                    }
                }),
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
                                this.close_qr_modal(cx);
                            });
                        }
                    }),
            )
    }
}

fn paint_camera_preview(
    bounds: Bounds<Pixels>,
    rgb_pixels: &[u8],
    width: usize,
    height: usize,
    window: &mut Window,
) {
    if width == 0
        || height == 0
        || rgb_pixels.len() < width.saturating_mul(height).saturating_mul(3)
    {
        return;
    }

    const COLUMNS: usize = 96;
    const ROWS: usize = 72;

    let cell_width = f32::from(bounds.size.width) / COLUMNS as f32;
    let cell_height = f32::from(bounds.size.height) / ROWS as f32;

    for row in 0..ROWS {
        let src_y = (row * height / ROWS).min(height - 1);
        for column in 0..COLUMNS {
            let src_x = (column * width / COLUMNS).min(width - 1);
            let idx = (src_y * width + src_x) * 3;
            let Some(rgb) = rgb_pixels.get(idx..idx + 3) else {
                continue;
            };

            let color = gpui::rgb(((rgb[0] as u32) << 16) | ((rgb[1] as u32) << 8) | rgb[2] as u32);
            window.paint_quad(fill(
                Bounds {
                    origin: point(
                        bounds.origin.x + px(column as f32 * cell_width),
                        bounds.origin.y + px(row as f32 * cell_height),
                    ),
                    size: size(px(cell_width + 0.5), px(cell_height + 0.5)),
                },
                color,
            ));
        }
    }
}

fn paint_qr_bounds(bounds: Bounds<Pixels>, qr_bounds: [(f32, f32); 4], window: &mut Window) {
    let mut builder = PathBuilder::stroke(px(3.0));
    for (index, (x, y)) in qr_bounds.iter().enumerate() {
        let point = point(
            bounds.origin.x + px(x.clamp(0.0, 1.0) * f32::from(bounds.size.width)),
            bounds.origin.y + px(y.clamp(0.0, 1.0) * f32::from(bounds.size.height)),
        );
        if index == 0 {
            builder.move_to(point);
        } else {
            builder.line_to(point);
        }
    }
    let first = qr_bounds[0];
    builder.line_to(point(
        bounds.origin.x + px(first.0.clamp(0.0, 1.0) * f32::from(bounds.size.width)),
        bounds.origin.y + px(first.1.clamp(0.0, 1.0) * f32::from(bounds.size.height)),
    ));
    if let Ok(path) = builder.build() {
        window.paint_path(path, gpui::rgb(0x22c55e));
    }
}
