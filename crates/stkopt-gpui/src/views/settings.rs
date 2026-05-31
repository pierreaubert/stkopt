//! Settings section view.

use gpui::prelude::*;
use gpui::*;
use gpui_ui_kit::theme::ThemeExt;
use gpui_ui_kit::*;

use crate::app::{ConnectionMode, StkoptApp};
use crate::persistence::{ConnectionModeConfig, NetworkConfig};
use crate::shortcuts::{Shortcut, shortcuts_by_category};

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
                        &theme,
                    ))
                    .child(Self::render_setting_row(
                        "Auto-connect",
                        "Automatically connect to the network on startup",
                        Toggle::new("auto-connect")
                            .checked(app.settings_auto_connect)
                            .size(ToggleSize::Md)
                            .on_change(move |enabled, _window, cx| {
                                entity.update(cx, |this, cx| {
                                    this.settings_auto_connect = enabled;
                                    this.save_config();
                                    cx.notify();
                                });
                            }),
                        &theme,
                    ))
                    .child(Self::render_setting_row(
                        "Show testnets",
                        "Display testnet networks in the network selector",
                        Toggle::new("show-testnets")
                            .checked(app.settings_show_testnets)
                            .size(ToggleSize::Md)
                            .on_change(move |enabled, _window, cx| {
                                entity2.update(cx, |this, cx| {
                                    this.settings_show_testnets = enabled;
                                    if !enabled && this.settings_network.is_testnet() {
                                        this.settings_network = NetworkConfig::Polkadot;
                                    }
                                    this.save_config();
                                    cx.notify();
                                });
                            }),
                        &theme,
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
                        &theme,
                    ))
                    .child(Self::render_setting_row(
                        "Connection Mode",
                        "How to connect to the blockchain",
                        Self::render_connection_mode_selector(app, cx),
                        &theme,
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
            let mut category_div = div().flex().flex_col().gap_2().child(
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
        theme: &gpui_ui_kit::theme::Theme,
    ) -> impl IntoElement {
        div()
            .flex()
            .flex_wrap()
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
                            .color(theme.text_secondary),
                    ),
            )
            .child(control)
    }

    fn render_theme_selector(app: &StkoptApp, cx: &Context<StkoptApp>) -> impl IntoElement {
        let _theme = cx.theme();
        let current = crate::theme::theme_config_value(app.settings_theme);
        let entity = app.entity.clone();

        ButtonSet::new("theme-selector")
            .options(vec![
                ButtonSetOption::new("system", "System"),
                ButtonSetOption::new("light", "Light"),
                ButtonSetOption::new("dark", "Dark"),
                ButtonSetOption::new("midnight", "Midnight"),
                ButtonSetOption::new("forest", "Forest"),
                ButtonSetOption::new("black-and-white", "B&W"),
            ])
            .selected(current)
            .size(ButtonSetSize::Sm)
            .on_change(move |value, _window, cx| {
                let value = value.to_string();
                if let Some(theme) = crate::theme::theme_config_from_value(&value) {
                    entity.update(cx, |this, cx| {
                        this.settings_theme = theme;
                        this.save_config();
                        cx.notify();
                    });
                    crate::theme::apply_theme_config(theme, cx);
                }
            })
    }

    fn render_network_selector(app: &StkoptApp, _cx: &Context<StkoptApp>) -> impl IntoElement {
        let current = app.settings_network;
        let entity = app.entity.clone();

        let mut options = vec![
            ButtonSetOption::new("polkadot", "Polkadot"),
            ButtonSetOption::new("kusama", "Kusama"),
        ];
        if app.settings_show_testnets || current == NetworkConfig::Westend {
            options.push(ButtonSetOption::new("westend", "Westend"));
        }

        ButtonSet::new("network-selector")
            .options(options)
            .selected(network_config_value(current))
            .size(ButtonSetSize::Sm)
            .on_change(move |value, _window, cx| {
                let value = value.to_string();
                if let Some(network) = network_config_from_value(&value) {
                    entity.update(cx, |this, cx| {
                        this.settings_network = network;
                        this.save_config();
                        cx.notify();
                    });
                }
            })
    }

    fn render_connection_mode_selector(
        app: &StkoptApp,
        _cx: &Context<StkoptApp>,
    ) -> impl IntoElement {
        let current = app.settings_connection_mode;
        let entity = app.entity.clone();

        ButtonSet::new("connection-mode-selector")
            .options(vec![
                ButtonSetOption::new("light-client", "Light Client"),
                ButtonSetOption::new("rpc", "RPC"),
            ])
            .selected(connection_mode_config_value(current))
            .size(ButtonSetSize::Sm)
            .on_change(move |value, _window, cx| {
                let value = value.to_string();
                if let Some(mode) = connection_mode_config_from_value(&value) {
                    entity.update(cx, |this, cx| {
                        this.set_connection_mode(ConnectionMode::from_config(mode), cx);
                    });
                }
            })
    }
}

fn network_config_value(network: NetworkConfig) -> &'static str {
    match network {
        NetworkConfig::Polkadot => "polkadot",
        NetworkConfig::Kusama => "kusama",
        NetworkConfig::Westend => "westend",
        NetworkConfig::Paseo => "paseo",
        NetworkConfig::Custom => "custom",
    }
}

fn network_config_from_value(value: &str) -> Option<NetworkConfig> {
    match value {
        "polkadot" => Some(NetworkConfig::Polkadot),
        "kusama" => Some(NetworkConfig::Kusama),
        "westend" => Some(NetworkConfig::Westend),
        _ => None,
    }
}

fn connection_mode_config_value(mode: ConnectionModeConfig) -> &'static str {
    match mode {
        ConnectionModeConfig::LightClient => "light-client",
        ConnectionModeConfig::Rpc => "rpc",
    }
}

fn connection_mode_config_from_value(value: &str) -> Option<ConnectionModeConfig> {
    match value {
        "light-client" => Some(ConnectionModeConfig::LightClient),
        "rpc" => Some(ConnectionModeConfig::Rpc),
        _ => None,
    }
}
