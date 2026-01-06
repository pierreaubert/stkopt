//! UI rendering.

use crate::app::{App, InputMode, View};
use crate::log_buffer::LogLevel;
use qrcode::QrCode;
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, Tabs},
    Frame,
};
use stkopt_chain::PoolState;
use stkopt_core::ConnectionStatus;

/// Render the entire UI.
pub fn render(frame: &mut Frame, app: &mut App) {
    let chunks = Layout::vertical([
        Constraint::Length(3), // Header
        Constraint::Length(3), // Tabs
        Constraint::Min(0),    // Content
        Constraint::Length(5), // Log viewer (3 lines + border)
    ])
    .split(frame.area());

    render_header(frame, app, chunks[0]);
    render_tabs(frame, app, chunks[1]);
    render_content(frame, app, chunks[2]);
    render_logs(frame, app, chunks[3]);

    // Render QR modal overlay if showing
    if app.showing_qr {
        render_qr_modal(frame, app);
    }

    // Render help overlay if showing
    if app.showing_help {
        render_help_modal(frame);
    }
}

/// Render the header with network info and era status.
fn render_header(frame: &mut Frame, app: &App, area: Rect) {
    let network_style = Style::default().fg(Color::Magenta).bold();
    let symbol = app.network.token_symbol();

    let era_info = match app.current_era {
        Some(era) => format!("Era {} ({:.0}%)", era, app.era_pct_complete * 100.0),
        None => "Era --".to_string(),
    };

    let header_text = Line::from(vec![
        Span::styled(format!("[{}] ", symbol), network_style),
        Span::raw(app.network.to_string()),
        Span::raw("  │  "),
        Span::raw(era_info),
    ]);

    let header = Paragraph::new(header_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Staking Optimizer "),
        )
        .alignment(Alignment::Left);

    frame.render_widget(header, area);
}

/// Render the tab bar.
fn render_tabs(frame: &mut Frame, app: &App, area: Rect) {
    let titles: Vec<Line> = View::all()
        .iter()
        .map(|v| Line::from(v.label()))
        .collect();

    let tabs = Tabs::new(titles)
        .block(Block::default().borders(Borders::ALL))
        .select(app.current_view.index())
        .style(Style::default().fg(Color::White))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );

    frame.render_widget(tabs, area);
}

/// Render the main content area based on current view.
fn render_content(frame: &mut Frame, app: &mut App, area: Rect) {
    match app.current_view {
        View::Validators => render_validators(frame, app, area),
        View::Pools => render_pools(frame, app, area),
        View::Nominate => render_nominate(frame, app, area),
        View::Account => render_account(frame, app, area),
    };
}

/// Format balance with proper decimals.
fn format_balance(balance: u128, decimals: u8) -> String {
    let divisor = 10u128.pow(decimals as u32);
    let whole = balance / divisor;
    let frac = (balance % divisor) / 10u128.pow(decimals.saturating_sub(2) as u32);
    if whole >= 1_000_000 {
        format!("{:.2}M", whole as f64 / 1_000_000.0)
    } else if whole >= 1_000 {
        format!("{:.2}K", whole as f64 / 1_000.0)
    } else {
        format!("{}.{:02}", whole, frac)
    }
}

/// Render the validators view with table.
fn render_validators(frame: &mut Frame, app: &mut App, area: Rect) {
    let decimals = app.network.token_decimals();

    if app.validators.is_empty() {
        let loading_text = if app.loading_validators {
            format!(
                "Loading validators... {:.0}%",
                app.loading_progress * 100.0
            )
        } else if app.connection_status == ConnectionStatus::Connected {
            "Fetching validators...".to_string()
        } else {
            "Waiting for connection...".to_string()
        };

        let text = vec![
            Line::from(""),
            Line::from(format!("  {}", loading_text)),
        ];

        let paragraph = Paragraph::new(text).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Validators "),
        );
        frame.render_widget(paragraph, area);
        return;
    }

    // Build table rows
    let rows: Vec<Row> = app
        .validators
        .iter()
        .map(|v| {
            let addr_short = format!("{}...{}", &v.address[..6], &v.address[v.address.len() - 6..]);
            let commission_str = format!("{:.1}%", v.commission * 100.0);
            let stake_str = format_balance(v.total_stake, decimals);
            let own_str = format_balance(v.own_stake, decimals);
            let apy_str = format!("{:.2}%", v.apy * 100.0);
            let blocked_str = if v.blocked { "Yes" } else { "No" };

            Row::new(vec![
                Cell::from(addr_short),
                Cell::from(commission_str),
                Cell::from(stake_str),
                Cell::from(own_str),
                Cell::from(v.nominator_count.to_string()),
                Cell::from(apy_str),
                Cell::from(blocked_str),
            ])
        })
        .collect();

    let header = Row::new(vec![
        Cell::from("Address").style(Style::default().bold()),
        Cell::from("Comm").style(Style::default().bold()),
        Cell::from("Total Stake").style(Style::default().bold()),
        Cell::from("Own Stake").style(Style::default().bold()),
        Cell::from("Noms").style(Style::default().bold()),
        Cell::from("APY").style(Style::default().bold()),
        Cell::from("Blocked").style(Style::default().bold()),
    ])
    .style(Style::default().fg(Color::Yellow));

    let widths = [
        Constraint::Length(15),
        Constraint::Length(7),
        Constraint::Length(12),
        Constraint::Length(12),
        Constraint::Length(6),
        Constraint::Length(8),
        Constraint::Length(8),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" Validators ({}) ", app.validators.len())),
        )
        .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol(">> ");

    frame.render_stateful_widget(table, area, &mut app.validators_table_state);
}

/// Render the nomination pools view with table.
fn render_pools(frame: &mut Frame, app: &mut App, area: Rect) {
    let decimals = app.network.token_decimals();

    if app.pools.is_empty() {
        let loading_text = if app.connection_status == ConnectionStatus::Connected {
            "Fetching nomination pools...".to_string()
        } else {
            "Waiting for connection...".to_string()
        };

        let text = vec![
            Line::from(""),
            Line::from(format!("  {}", loading_text)),
        ];

        let paragraph = Paragraph::new(text).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Nomination Pools "),
        );
        frame.render_widget(paragraph, area);
        return;
    }

    // Build table rows
    let rows: Vec<Row> = app
        .pools
        .iter()
        .map(|p| {
            let state_str = match p.state {
                PoolState::Open => "Open",
                PoolState::Blocked => "Blocked",
                PoolState::Destroying => "Destroying",
            };
            let state_style = match p.state {
                PoolState::Open => Style::default().fg(Color::Green),
                PoolState::Blocked => Style::default().fg(Color::Yellow),
                PoolState::Destroying => Style::default().fg(Color::Red),
            };
            let points_str = format_balance(p.points, decimals);
            let name_display = if p.name.is_empty() {
                format!("Pool #{}", p.id)
            } else if p.name.len() > 30 {
                format!("{}...", &p.name[..27])
            } else {
                p.name.clone()
            };
            let apy_str = match p.apy {
                Some(apy) => format!("{:.2}%", apy * 100.0),
                None => "-".to_string(),
            };

            Row::new(vec![
                Cell::from(p.id.to_string()),
                Cell::from(name_display),
                Cell::from(state_str).style(state_style),
                Cell::from(p.member_count.to_string()),
                Cell::from(points_str),
                Cell::from(apy_str),
            ])
        })
        .collect();

    let header = Row::new(vec![
        Cell::from("ID").style(Style::default().bold()),
        Cell::from("Name").style(Style::default().bold()),
        Cell::from("State").style(Style::default().bold()),
        Cell::from("Members").style(Style::default().bold()),
        Cell::from("Points").style(Style::default().bold()),
        Cell::from("APY").style(Style::default().bold()),
    ])
    .style(Style::default().fg(Color::Yellow));

    let widths = [
        Constraint::Length(6),
        Constraint::Min(25),
        Constraint::Length(12),
        Constraint::Length(10),
        Constraint::Length(14),
        Constraint::Length(8),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" Nomination Pools ({}) ", app.pools.len())),
        )
        .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol(">> ");

    frame.render_stateful_widget(table, area, &mut app.pools_table_state);
}

/// Render the nomination optimizer view.
fn render_nominate(frame: &mut Frame, app: &mut App, area: Rect) {
    let decimals = app.network.token_decimals();

    // Split into info panel and validator table
    let chunks = Layout::vertical([Constraint::Length(8), Constraint::Min(0)]).split(area);

    // Info panel
    let mut info_lines = Vec::new();
    info_lines.push(Line::from(""));

    // Show optimization result or manual selection info
    if let Some(result) = &app.optimization_result {
        info_lines.push(Line::from(vec![
            Span::styled("  Optimized Selection ", Style::default().fg(Color::Green).bold()),
            Span::raw(format!("({} validators)", result.selected.len())),
        ]));
        info_lines.push(Line::from(format!(
            "  Est. APY: {:.2}% - {:.2}% (avg {:.2}%)",
            result.estimated_apy_min * 100.0,
            result.estimated_apy_max * 100.0,
            result.estimated_apy_avg * 100.0
        )));
    } else {
        info_lines.push(Line::from(vec![
            Span::styled("  Manual Selection ", Style::default().fg(Color::Yellow).bold()),
            Span::raw(format!("({}/16 validators)", app.selected_validators.len())),
        ]));
    }

    info_lines.push(Line::from(""));
    info_lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled("o", Style::default().fg(Color::Cyan).bold()),
        Span::raw(": Optimize  "),
        Span::styled("Space", Style::default().fg(Color::Cyan).bold()),
        Span::raw(": Toggle  "),
        Span::styled("c", Style::default().fg(Color::Cyan).bold()),
        Span::raw(": Clear  "),
        Span::styled("g", Style::default().fg(Color::Cyan).bold()),
        Span::raw(": Generate QR"),
    ]));

    let info_panel = Paragraph::new(info_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Nomination Optimizer "),
    );
    frame.render_widget(info_panel, chunks[0]);

    // Validator table with selection checkboxes
    if app.validators.is_empty() {
        let loading = Paragraph::new("  Loading validators...")
            .block(Block::default().borders(Borders::ALL).title(" Select Validators "));
        frame.render_widget(loading, chunks[1]);
        return;
    }

    let rows: Vec<Row> = app
        .validators
        .iter()
        .enumerate()
        .map(|(idx, v)| {
            let selected = app.selected_validators.contains(&idx);
            let checkbox = if selected { "[x]" } else { "[ ]" };
            let checkbox_style = if selected {
                Style::default().fg(Color::Green).bold()
            } else {
                Style::default()
            };

            let addr_short = format!("{}...{}", &v.address[..6], &v.address[v.address.len() - 6..]);
            let commission_str = format!("{:.1}%", v.commission * 100.0);
            let stake_str = format_balance(v.total_stake, decimals);
            let apy_str = format!("{:.2}%", v.apy * 100.0);
            let blocked_str = if v.blocked { "Yes" } else { "No" };

            Row::new(vec![
                Cell::from(checkbox).style(checkbox_style),
                Cell::from(addr_short),
                Cell::from(commission_str),
                Cell::from(stake_str),
                Cell::from(apy_str),
                Cell::from(blocked_str),
            ])
        })
        .collect();

    let header = Row::new(vec![
        Cell::from("Sel").style(Style::default().bold()),
        Cell::from("Address").style(Style::default().bold()),
        Cell::from("Comm").style(Style::default().bold()),
        Cell::from("Total Stake").style(Style::default().bold()),
        Cell::from("APY").style(Style::default().bold()),
        Cell::from("Blocked").style(Style::default().bold()),
    ])
    .style(Style::default().fg(Color::Yellow));

    let widths = [
        Constraint::Length(5),
        Constraint::Length(15),
        Constraint::Length(7),
        Constraint::Length(12),
        Constraint::Length(8),
        Constraint::Length(8),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(
                    " Select Validators ({} selected) ",
                    app.selected_validators.len()
                )),
        )
        .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol(">> ");

    frame.render_stateful_widget(table, chunks[1], &mut app.nominate_table_state);
}

/// Render the account status view.
fn render_account(frame: &mut Frame, app: &App, area: Rect) {
    let decimals = app.network.token_decimals();
    let symbol = app.network.token_symbol();

    // Split area for input (if in input mode) and content
    let chunks = if app.input_mode == InputMode::EnteringAccount {
        Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).split(area)
    } else {
        Layout::vertical([Constraint::Min(0)]).split(area)
    };

    let content_area = if app.input_mode == InputMode::EnteringAccount {
        // Render input box
        let input = Paragraph::new(app.account_input.as_str())
            .style(Style::default().fg(Color::Yellow))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Enter SS58 Address (Enter to confirm, Esc to cancel) "),
            );
        frame.render_widget(input, chunks[0]);
        chunks[1]
    } else {
        chunks[0]
    };

    let mut lines = Vec::new();
    lines.push(Line::from(""));

    match (&app.watched_account, &app.account_status) {
        (None, _) => {
            lines.push(Line::from("  No account loaded"));
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::raw("  Press "),
                Span::styled("a", Style::default().fg(Color::Yellow).bold()),
                Span::raw(" to enter an account address"),
            ]));
        }
        (Some(account), None) => {
            lines.push(Line::from(vec![
                Span::styled("  Address: ", Style::default().bold()),
                Span::raw(account.to_string()),
            ]));
            lines.push(Line::from(""));
            lines.push(Line::from("  Loading account data..."));
        }
        (Some(_), Some(status)) => {
            // Address
            lines.push(Line::from(vec![
                Span::styled("  Address: ", Style::default().bold()),
                Span::raw(status.address.to_string()),
            ]));
            lines.push(Line::from(""));

            // Balances section
            lines.push(Line::from(Span::styled(
                "  Balances",
                Style::default().fg(Color::Cyan).bold(),
            )));
            lines.push(Line::from(vec![
                Span::raw("    Free:     "),
                Span::styled(
                    format!("{} {}", format_balance(status.balance.free, decimals), symbol),
                    Style::default().fg(Color::Green),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::raw("    Reserved: "),
                Span::raw(format!(
                    "{} {}",
                    format_balance(status.balance.reserved, decimals),
                    symbol
                )),
            ]));
            lines.push(Line::from(vec![
                Span::raw("    Frozen:   "),
                Span::raw(format!(
                    "{} {}",
                    format_balance(status.balance.frozen, decimals),
                    symbol
                )),
            ]));
            lines.push(Line::from(""));

            // Staking section
            lines.push(Line::from(Span::styled(
                "  Staking",
                Style::default().fg(Color::Cyan).bold(),
            )));
            if let Some(ledger) = &status.staking_ledger {
                lines.push(Line::from(vec![
                    Span::raw("    Bonded: "),
                    Span::styled(
                        format!("{} {}", format_balance(ledger.active, decimals), symbol),
                        Style::default().fg(Color::Yellow),
                    ),
                ]));
                if ledger.total > ledger.active {
                    lines.push(Line::from(vec![
                        Span::raw("    Total:  "),
                        Span::raw(format!(
                            "{} {}",
                            format_balance(ledger.total, decimals),
                            symbol
                        )),
                    ]));
                }
                if !ledger.unlocking.is_empty() {
                    lines.push(Line::from(vec![
                        Span::raw("    Unlocking: "),
                        Span::styled(
                            format!("{} chunks", ledger.unlocking.len()),
                            Style::default().fg(Color::Magenta),
                        ),
                    ]));
                    for chunk in &ledger.unlocking {
                        lines.push(Line::from(format!(
                            "      - {} {} (era {})",
                            format_balance(chunk.value, decimals),
                            symbol,
                            chunk.era
                        )));
                    }
                }
            } else {
                lines.push(Line::from("    Not staking directly"));
            }
            lines.push(Line::from(""));

            // Nominations section
            lines.push(Line::from(Span::styled(
                "  Nominations",
                Style::default().fg(Color::Cyan).bold(),
            )));
            if let Some(nominations) = &status.nominations {
                lines.push(Line::from(format!(
                    "    {} validators nominated (era {})",
                    nominations.targets.len(),
                    nominations.submitted_in
                )));
                for (i, target) in nominations.targets.iter().take(5).enumerate() {
                    let addr_short = format!(
                        "{}...{}",
                        &target.to_string()[..8],
                        &target.to_string()[target.to_string().len() - 8..]
                    );
                    lines.push(Line::from(format!("    {}. {}", i + 1, addr_short)));
                }
                if nominations.targets.len() > 5 {
                    lines.push(Line::from(format!(
                        "    ... and {} more",
                        nominations.targets.len() - 5
                    )));
                }
            } else {
                lines.push(Line::from("    No nominations"));
            }
            lines.push(Line::from(""));

            // Pool membership section
            lines.push(Line::from(Span::styled(
                "  Nomination Pool",
                Style::default().fg(Color::Cyan).bold(),
            )));
            if let Some(membership) = &status.pool_membership {
                lines.push(Line::from(vec![
                    Span::raw("    Pool ID: "),
                    Span::styled(
                        membership.pool_id.to_string(),
                        Style::default().fg(Color::Yellow),
                    ),
                ]));
                lines.push(Line::from(format!(
                    "    Points: {}",
                    format_balance(membership.points, decimals)
                )));
                if !membership.unbonding_eras.is_empty() {
                    lines.push(Line::from(format!(
                        "    Unbonding: {} eras",
                        membership.unbonding_eras.len()
                    )));
                }
            } else {
                lines.push(Line::from("    Not a pool member"));
            }

            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::raw("  Press "),
                Span::styled("c", Style::default().fg(Color::Yellow).bold()),
                Span::raw(" to clear account, "),
                Span::styled("a", Style::default().fg(Color::Yellow).bold()),
                Span::raw(" to change"),
            ]));
        }
    }

    let paragraph = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Account Status "),
    );
    frame.render_widget(paragraph, content_area);
}

/// Render the QR code modal overlay.
fn render_qr_modal(frame: &mut Frame, app: &App) {
    let area = frame.area();

    let mut lines = Vec::new();
    let mut qr_width: u16 = 0;

    match &app.qr_data {
        Some(data) => {
            match QrCode::new(data) {
                Ok(qr) => {
                    // Use half-block rendering for compact display
                    // ▀ = top half, ▄ = bottom half, █ = both, ' ' = neither
                    let qr_lines = render_qr_halfblock(&qr);
                    qr_width = qr_lines.first().map(|l| l.len() as u16).unwrap_or(0);

                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled(
                        "Scan with Polkadot Vault",
                        Style::default().fg(Color::Cyan).bold(),
                    )));
                    lines.push(Line::from(""));

                    for line in qr_lines {
                        lines.push(Line::from(line));
                    }

                    lines.push(Line::from(""));
                    lines.push(Line::from(format!("Payload: {} bytes", data.len())));
                }
                Err(e) => {
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled(
                        format!("Error generating QR: {}", e),
                        Style::default().fg(Color::Red),
                    )));
                }
            }
        }
        None => {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "No QR data available",
                Style::default().fg(Color::Yellow),
            )));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::raw("Press "),
        Span::styled("Esc", Style::default().fg(Color::Yellow).bold()),
        Span::raw(" to close"),
    ]));

    // Dynamic modal sizing based on QR code size
    let content_height = lines.len() as u16;
    let content_width = qr_width.max(30);

    // Add padding for borders and margins
    let modal_width = (content_width + 4).min(area.width.saturating_sub(2));
    let modal_height = (content_height + 2).min(area.height.saturating_sub(2));

    let modal_x = (area.width.saturating_sub(modal_width)) / 2;
    let modal_y = (area.height.saturating_sub(modal_height)) / 2;
    let modal_area = Rect::new(modal_x, modal_y, modal_width, modal_height);

    // Clear the background
    frame.render_widget(Clear, modal_area);

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(" QR Code "),
        )
        .style(Style::default().bg(Color::Black))
        .alignment(Alignment::Center);

    frame.render_widget(paragraph, modal_area);
}

/// Render QR code using half-block characters for compact display.
/// Uses ▀ (top), ▄ (bottom), █ (both), ' ' (neither) to fit 2 rows per line.
fn render_qr_halfblock(qr: &QrCode) -> Vec<String> {
    let colors = qr.to_colors();
    let width = qr.width();

    // Add quiet zone (4 modules on each side per QR spec)
    let quiet = 4;
    let total_width = width + 2 * quiet;

    let mut result = Vec::new();

    // Process 2 rows at a time
    let total_height = width + 2 * quiet;
    let mut y = 0;

    while y < total_height {
        let mut line = String::new();

        for x in 0..total_width {
            let top_dark = if y < quiet || y >= quiet + width || x < quiet || x >= quiet + width {
                false // quiet zone is light
            } else {
                let idx = (y - quiet) * width + (x - quiet);
                colors.get(idx).map(|c| *c == qrcode::Color::Dark).unwrap_or(false)
            };

            let bottom_dark = if y + 1 < quiet || y + 1 >= quiet + width || x < quiet || x >= quiet + width {
                false
            } else if y + 1 < total_height {
                let idx = (y + 1 - quiet) * width + (x - quiet);
                colors.get(idx).map(|c| *c == qrcode::Color::Dark).unwrap_or(false)
            } else {
                false
            };

            let ch = match (top_dark, bottom_dark) {
                (true, true) => '█',
                (true, false) => '▀',
                (false, true) => '▄',
                (false, false) => ' ',
            };
            line.push(ch);
        }

        result.push(line);
        y += 2;
    }

    result
}

/// Render the help modal overlay.
fn render_help_modal(frame: &mut Frame) {
    let area = frame.area();

    // Calculate centered modal area
    let modal_width = 55.min(area.width.saturating_sub(4));
    let modal_height = 33.min(area.height.saturating_sub(4));
    let modal_x = (area.width.saturating_sub(modal_width)) / 2;
    let modal_y = (area.height.saturating_sub(modal_height)) / 2;
    let modal_area = Rect::new(modal_x, modal_y, modal_width, modal_height);

    // Clear the background
    frame.render_widget(Clear, modal_area);

    let key_style = Style::default().fg(Color::Yellow).bold();
    let desc_style = Style::default();

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  Global Keys",
            Style::default().fg(Color::Cyan).bold(),
        )),
        Line::from(vec![
            Span::styled("  q         ", key_style),
            Span::styled("Quit application", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  Tab       ", key_style),
            Span::styled("Next tab", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  Shift+Tab ", key_style),
            Span::styled("Previous tab", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  1-4       ", key_style),
            Span::styled("Jump to tab", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  ↑/k ↓/j   ", key_style),
            Span::styled("Navigate list", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  ?         ", key_style),
            Span::styled("Toggle this help", desc_style),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  Account Tab",
            Style::default().fg(Color::Cyan).bold(),
        )),
        Line::from(vec![
            Span::styled("  a         ", key_style),
            Span::styled("Enter account address", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  c         ", key_style),
            Span::styled("Clear account", desc_style),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  Nominate Tab",
            Style::default().fg(Color::Cyan).bold(),
        )),
        Line::from(vec![
            Span::styled("  o         ", key_style),
            Span::styled("Run optimization", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  Space     ", key_style),
            Span::styled("Toggle validator selection", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  c         ", key_style),
            Span::styled("Clear nominations", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  g         ", key_style),
            Span::styled("Generate QR code", desc_style),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  Log Viewer",
            Style::default().fg(Color::Cyan).bold(),
        )),
        Line::from(vec![
            Span::styled("  PgUp/PgDn ", key_style),
            Span::styled("Scroll logs up/down", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  End       ", key_style),
            Span::styled("Jump to latest logs", desc_style),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::raw("  Press "),
            Span::styled("Esc", key_style),
            Span::raw(" or "),
            Span::styled("?", key_style),
            Span::raw(" to close"),
        ]),
    ];

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Green))
                .title(" Keyboard Shortcuts "),
        )
        .style(Style::default().bg(Color::Black));

    frame.render_widget(paragraph, modal_area);
}

/// Render the log viewer with connection status.
fn render_logs(frame: &mut Frame, app: &App, area: Rect) {
    let logs = app.log_buffer.get_lines();
    let log_count = logs.len();

    // Build title with connection status and scroll info
    let status_text = match &app.connection_status {
        ConnectionStatus::Disconnected => "Disconnected".to_string(),
        ConnectionStatus::Connecting => format!("Connecting{}", ".".repeat((app.tick_count() % 4) as usize)),
        ConnectionStatus::Syncing { progress } => format!("Syncing {:.0}%", progress * 100.0),
        ConnectionStatus::Connected => "Connected".to_string(),
        ConnectionStatus::Error(e) => format!("Error: {}", e),
    };

    let status_style = match &app.connection_status {
        ConnectionStatus::Disconnected => Style::default().fg(Color::Red),
        ConnectionStatus::Connecting => Style::default().fg(Color::Yellow),
        ConnectionStatus::Syncing { .. } => Style::default().fg(Color::Yellow),
        ConnectionStatus::Connected => Style::default().fg(Color::Green),
        ConnectionStatus::Error(_) => Style::default().fg(Color::Red),
    };

    let scroll_info = if app.log_scroll > 0 {
        format!(" [↑{}]", app.log_scroll)
    } else {
        String::new()
    };

    // Get visible lines (3 lines, accounting for scroll)
    let visible_lines = 3;
    let start_idx = if log_count > visible_lines {
        log_count - visible_lines - app.log_scroll.min(log_count.saturating_sub(visible_lines))
    } else {
        0
    };
    let end_idx = (start_idx + visible_lines).min(log_count);

    let lines: Vec<Line> = logs[start_idx..end_idx]
        .iter()
        .map(|log| {
            let level_style = match log.level {
                LogLevel::Trace => Style::default().fg(Color::DarkGray),
                LogLevel::Debug => Style::default().fg(Color::Cyan),
                LogLevel::Info => Style::default().fg(Color::Green),
                LogLevel::Warn => Style::default().fg(Color::Yellow),
                LogLevel::Error => Style::default().fg(Color::Red),
            };

            // Shorten target if too long
            let target = if log.target.len() > 20 {
                format!("..{}", &log.target[log.target.len() - 18..])
            } else {
                log.target.clone()
            };

            Line::from(vec![
                Span::styled(format!("{:5} ", log.level.as_str()), level_style),
                Span::styled(format!("[{}] ", target), Style::default().fg(Color::DarkGray)),
                Span::raw(&log.message),
            ])
        })
        .collect();

    // Pad with empty lines if we have fewer than 3 log lines
    let mut display_lines = lines;
    while display_lines.len() < visible_lines {
        display_lines.insert(0, Line::from(""));
    }

    let title = Line::from(vec![
        Span::raw(" Logs "),
        Span::styled(format!("({}) ", log_count), Style::default().fg(Color::DarkGray)),
        Span::raw("│ "),
        Span::styled(status_text, status_style),
        Span::styled(scroll_info, Style::default().fg(Color::DarkGray)),
        Span::raw(" │ PgUp/PgDn:Scroll  ?:Help  q:Quit "),
    ]);

    let paragraph = Paragraph::new(display_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(title),
    );

    frame.render_widget(paragraph, area);
}
