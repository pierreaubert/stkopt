//! Dashboard section view - overview of staking status.

use gpui::prelude::*;
use gpui::*;
use gpui_ui_kit::theme::ThemeExt;
use gpui_ui_kit::*;

use crate::app::StkoptApp;

pub struct DashboardSection;

impl DashboardSection {
    pub fn render(app: &StkoptApp, cx: &Context<StkoptApp>) -> impl IntoElement {
        let theme = cx.theme();
        let symbol = app.network.symbol();

        let (total_balance, bonded, unbonding, rewards) = if let Some(ref info) = app.staking_info {
            (
                format_balance(info.total_balance, symbol),
                format_balance(info.bonded, symbol),
                format_balance(info.unbonding, symbol),
                format_balance(info.rewards_pending, symbol),
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
            .flex()
            .flex_col()
            .gap_6()
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
                            .gap_3()
                            .child(Button::new("btn-bond", "Bond").variant(ButtonVariant::Primary))
                            .child(Button::new("btn-unbond", "Unbond").variant(ButtonVariant::Secondary))
                            .child(Button::new("btn-claim", "Claim Rewards").variant(ButtonVariant::Secondary))
                            .child(Button::new("btn-nominate", "Nominate").variant(ButtonVariant::Secondary)),
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
            .child(
                Text::new(value)
                    .size(TextSize::Xl)
                    .weight(TextWeight::Bold),
            ),
    )
}

fn format_balance(amount: u128, symbol: &str) -> String {
    let decimals = 10u128.pow(10);
    let whole = amount / decimals;
    let frac = (amount % decimals) / 10u128.pow(6);
    format!("{}.{:04} {}", whole, frac, symbol)
}
