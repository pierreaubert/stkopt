//! Settings section view.

use gpui::prelude::*;
use gpui::*;
use gpui_ui_kit::theme::ThemeExt;
use gpui_ui_kit::*;

use crate::app::StkoptApp;
use crate::persistence::{ConnectionModeConfig, NetworkConfig, ThemeConfig};
use crate::shortcuts::{shortcuts_by_category, Shortcut};

/// Settings section component.
pub struct SettingsSection;

impl SettingsSection {
    pub fn render(app: &mut StkoptApp, cx: &mut Context<StkoptApp>) -> impl IntoElement {
        let _theme = cx.theme();
        let entity = app.entity.clone();

        div()
            .flex()
            .flex_col()
            .gap_6()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(Heading::h1("Settings").into_any_element())
                    .child(
                        Button::new("close-settings", "Close")
                            .variant(ButtonVariant::Ghost)
                            .on_click({
                                let entity = entity.clone();
                                move |_window, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.show_settings = false;
                                        cx.notify();
                                    });
                                }
                            }),
                    ),
            )
            .child(Self::render_general_settings(app, cx))
            .child(Self::render_network_settings(app, cx))
            .child(Self::render_keyboard_shortcuts(cx))
    }

    fn render_general_settings(app: &StkoptApp, cx: &Context<StkoptApp>) -> impl IntoElement {
        let theme = cx.theme();
        let entity = app.entity.clone();
        let entity2 = app.entity.clone();

        div()
            .flex()
            .flex_col()
            .gap_4()
            .p_4()
            .rounded_lg()
            .bg(theme.surface)
            .border_1()
            .border_color(theme.border)
            .child(Heading::h3("General").into_any_element())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(Self::render_setting_row(
                        "Theme",
                        "Choose your preferred color scheme",
                        Self::render_theme_selector(app, cx),
                    ))
                    .child(Self::render_setting_row(
                        "Auto-connect",
                        "Automatically connect to the network on startup",
                        Self::render_toggle(app.settings_auto_connect, "auto-connect").on_click(
                            move |_event, _window, cx| {
                                entity.update(cx, |this, cx| {
                                    this.settings_auto_connect = !this.settings_auto_connect;
                                    this.save_config();
                                    cx.notify();
                                });
                            },
                        ),
                    ))
                    .child(Self::render_setting_row(
                        "Show testnets",
                        "Display testnet networks in the network selector",
                        Self::render_toggle(app.settings_show_testnets, "show-testnets").on_click(
                            move |_event, _window, cx| {
                                entity2.update(cx, |this, cx| {
                                    this.settings_show_testnets = !this.settings_show_testnets;
                                    this.save_config();
                                    cx.notify();
                                });
                            },
                        ),
                    )),
            )
    }

    fn render_network_settings(app: &StkoptApp, cx: &Context<StkoptApp>) -> impl IntoElement {
        let theme = cx.theme();

        div()
            .flex()
            .flex_col()
            .gap_4()
            .p_4()
            .rounded_lg()
            .bg(theme.surface)
            .border_1()
            .border_color(theme.border)
            .child(Heading::h3("Network").into_any_element())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(Self::render_setting_row(
                        "Default Network",
                        "The network to connect to by default",
                        Self::render_network_selector(app, cx),
                    ))
                    .child(Self::render_setting_row(
                        "Connection Mode",
                        "How to connect to the blockchain",
                        Self::render_connection_mode_selector(app, cx),
                    )),
            )
    }

    fn render_keyboard_shortcuts(cx: &Context<StkoptApp>) -> impl IntoElement {
        let theme = cx.theme();
        let grouped = shortcuts_by_category();

        let mut container = div()
            .flex()
            .flex_col()
            .gap_4()
            .p_4()
            .rounded_lg()
            .bg(theme.surface)
            .border_1()
            .border_color(theme.border)
            .child(Heading::h3("Keyboard Shortcuts").into_any_element());

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

            container = container.child(category_div);
        }

        container
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

    fn render_setting_row(
        label: &'static str,
        description: &'static str,
        control: impl IntoElement,
    ) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap_4()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(Text::new(label).size(TextSize::Sm))
                    .child(
                        Text::new(description)
                            .size(TextSize::Xs)
                            .color(gpui::rgb(0x888888)),
                    ),
            )
            .child(control)
    }

    fn render_theme_selector(app: &StkoptApp, cx: &Context<StkoptApp>) -> impl IntoElement {
        let _theme = cx.theme();
        let current = app.settings_theme;
        let entity = app.entity.clone();
        let entity2 = app.entity.clone();
        let entity3 = app.entity.clone();

        div()
            .flex()
            .gap_1()
            .child(
                Self::render_option_button("System", current == ThemeConfig::System, cx)
                    .on_click(move |_event, _window, cx| {
                        entity.update(cx, |this, cx| {
                            this.settings_theme = ThemeConfig::System;
                            this.save_config();
                            cx.notify();
                        });
                    }),
            )
            .child(
                Self::render_option_button("Light", current == ThemeConfig::Light, cx)
                    .on_click(move |_event, _window, cx| {
                        entity2.update(cx, |this, cx| {
                            this.settings_theme = ThemeConfig::Light;
                            this.save_config();
                            cx.notify();
                        });
                    }),
            )
            .child(
                Self::render_option_button("Dark", current == ThemeConfig::Dark, cx)
                    .on_click(move |_event, _window, cx| {
                        entity3.update(cx, |this, cx| {
                            this.settings_theme = ThemeConfig::Dark;
                            this.save_config();
                            cx.notify();
                        });
                    }),
            )
    }

    fn render_network_selector(app: &StkoptApp, cx: &Context<StkoptApp>) -> impl IntoElement {
        let current = app.settings_network;
        let entity = app.entity.clone();
        let entity2 = app.entity.clone();

        div()
            .flex()
            .gap_1()
            .child(
                Self::render_option_button("Polkadot", current == NetworkConfig::Polkadot, cx)
                    .on_click(move |_event, _window, cx| {
                        entity.update(cx, |this, cx| {
                            this.settings_network = NetworkConfig::Polkadot;
                            this.save_config();
                            cx.notify();
                        });
                    }),
            )
            .child(
                Self::render_option_button("Kusama", current == NetworkConfig::Kusama, cx)
                    .on_click(move |_event, _window, cx| {
                        entity2.update(cx, |this, cx| {
                            this.settings_network = NetworkConfig::Kusama;
                            this.save_config();
                            cx.notify();
                        });
                    }),
            )
    }

    fn render_connection_mode_selector(app: &StkoptApp, cx: &Context<StkoptApp>) -> impl IntoElement {
        let current = app.settings_connection_mode;
        let entity = app.entity.clone();
        let entity2 = app.entity.clone();

        div()
            .flex()
            .gap_1()
            .child(
                Self::render_option_button(
                    "Light Client",
                    current == ConnectionModeConfig::LightClient,
                    cx,
                )
                .on_click(move |_event, _window, cx| {
                    entity.update(cx, |this, cx| {
                        this.settings_connection_mode = ConnectionModeConfig::LightClient;
                        this.save_config();
                        cx.notify();
                    });
                }),
            )
            .child(
                Self::render_option_button("RPC", current == ConnectionModeConfig::Rpc, cx)
                    .on_click(move |_event, _window, cx| {
                        entity2.update(cx, |this, cx| {
                            this.settings_connection_mode = ConnectionModeConfig::Rpc;
                            this.save_config();
                            cx.notify();
                        });
                    }),
            )
    }

    fn render_option_button(
        label: &'static str,
        is_selected: bool,
        cx: &Context<StkoptApp>,
    ) -> Stateful<Div> {
        let theme = cx.theme();
        let id = SharedString::from(format!("option-{}", label.to_lowercase()));

        let mut btn = div()
            .id(id)
            .px_3()
            .py_1()
            .rounded_md()
            .text_sm()
            .cursor_pointer()
            .child(label);

        if is_selected {
            btn = btn.bg(theme.accent).text_color(gpui::rgb(0xffffff));
        } else {
            btn = btn
                .bg(theme.background)
                .border_1()
                .border_color(theme.border)
                .text_color(theme.text_secondary);
        }

        btn
    }

    fn render_toggle(enabled: bool, id: impl Into<ElementId>) -> Stateful<Div> {
        let (bg, dot_pos) = if enabled {
            (gpui::rgb(0x22c55e), px(18.0))
        } else {
            (gpui::rgb(0x6b7280), px(2.0))
        };

        div()
            .id(id)
            .w(px(40.0))
            .h(px(22.0))
            .rounded_full()
            .bg(bg)
            .cursor_pointer()
            .child(
                div()
                    .w(px(18.0))
                    .h(px(18.0))
                    .mt(px(2.0))
                    .ml(dot_pos)
                    .rounded_full()
                    .bg(gpui::rgb(0xffffff)),
            )
    }
}
