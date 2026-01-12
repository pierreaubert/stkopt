//! Help overlay view.

use gpui::prelude::*;
use gpui::*;
use gpui_ui_kit::theme::ThemeExt;
use gpui_ui_kit::*;

use crate::app::StkoptApp;
use crate::shortcuts::{shortcuts_by_category, Shortcut};

/// Help overlay component.
pub struct HelpOverlay;

impl HelpOverlay {
    pub fn render(app: &mut StkoptApp, cx: &mut Context<StkoptApp>) -> impl IntoElement {
        let theme = cx.theme();
        let entity = app.entity.clone();

        div()
            .id("help-overlay")
            .absolute()
            .inset_0()
            .bg(rgba(0x00000088))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(MouseButton::Left, {
                let entity = entity.clone();
                move |_event, _window, cx| {
                    entity.update(cx, |this, cx| {
                        this.show_help = false;
                        cx.notify();
                    });
                }
            })
            .child(
                div()
                    .id("help-content")
                    .w(px(600.0))
                    .max_h(px(500.0))
                    .bg(theme.surface)
                    .rounded_lg()
                    .border_1()
                    .border_color(theme.border)
                    .shadow_lg()
                    .overflow_y_scroll()
                    .on_mouse_down(MouseButton::Left, |_event, _window, _cx| {
                        // Stop propagation - don't close when clicking inside
                    })
                    .child(Self::render_header(cx))
                    .child(Self::render_shortcuts_section(cx))
                    .child(Self::render_tips_section(cx))
                    .child(Self::render_footer(cx)),
            )
    }

    fn render_header(cx: &Context<StkoptApp>) -> impl IntoElement {
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
                    .child(Text::new("❓").size(TextSize::Lg))
                    .child(Heading::h2("Help").into_any_element()),
            )
            .child(
                Text::new("Press ? or Esc to close")
                    .size(TextSize::Sm)
                    .color(theme.text_secondary),
            )
    }

    fn render_shortcuts_section(cx: &Context<StkoptApp>) -> impl IntoElement {
        let theme = cx.theme();
        let grouped = shortcuts_by_category();

        let mut section = div().flex().flex_col().gap_4().p_4();

        section = section.child(Heading::h3("Keyboard Shortcuts").into_any_element());

        for (category, shortcuts) in grouped {
            let mut category_div = div()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    Text::new(category.label())
                        .size(TextSize::Sm)
                        .color(theme.text_secondary),
                );

            for shortcut in shortcuts {
                category_div = category_div.child(Self::render_shortcut_row(shortcut, cx));
            }

            section = section.child(category_div);
        }

        section
    }

    fn render_shortcut_row(shortcut: Shortcut, cx: &Context<StkoptApp>) -> impl IntoElement {
        let theme = cx.theme();

        div()
            .flex()
            .items_center()
            .justify_between()
            .py_1()
            .child(Text::new(shortcut.label()).size(TextSize::Sm))
            .child(
                div()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .bg(theme.background)
                    .border_1()
                    .border_color(theme.border)
                    .child(
                        Text::new(shortcut.display())
                            .size(TextSize::Xs)
                            .color(theme.text_secondary),
                    ),
            )
    }

    fn render_tips_section(cx: &Context<StkoptApp>) -> impl IntoElement {
        let theme = cx.theme();

        div()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .border_t_1()
            .border_color(theme.border)
            .child(Heading::h3("Quick Tips").into_any_element())
            .child(Self::render_tip(
                "🔍",
                "Watch an Account",
                "Enter a Polkadot or Kusama address to monitor staking status.",
            ))
            .child(Self::render_tip(
                "⚡",
                "Optimize Selection",
                "Use the Optimization tab to find the best validators for your stake.",
            ))
            .child(Self::render_tip(
                "📊",
                "Track History",
                "View your staking rewards and APY trends in the History tab.",
            ))
            .child(Self::render_tip(
                "🔐",
                "Secure Signing",
                "Generate QR codes for air-gapped signing with Polkadot Vault.",
            ))
    }

    fn render_tip(icon: &'static str, title: &'static str, description: &'static str) -> impl IntoElement {
        div()
            .flex()
            .gap_3()
            .child(Text::new(icon).size(TextSize::Lg))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(Text::new(title).size(TextSize::Sm))
                    .child(
                        Text::new(description)
                            .size(TextSize::Xs)
                            .color(gpui::rgb(0x888888)),
                    ),
            )
    }

    fn render_footer(cx: &Context<StkoptApp>) -> impl IntoElement {
        let theme = cx.theme();

        div()
            .flex()
            .items_center()
            .justify_center()
            .p_4()
            .border_t_1()
            .border_color(theme.border)
            .child(
                Text::new("Staking Optimizer v0.1.0 • Built with GPUI")
                    .size(TextSize::Xs)
                    .color(theme.text_secondary),
            )
    }
}
