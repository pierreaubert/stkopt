//! UI rendering.

use crate::app::{App, InputMode, View};
use crate::log_buffer::LogLevel;
use crate::theme::Palette;
use qrcode::QrCode;
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Modifier, Style, Stylize},
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
        render_help_modal(frame, app);
    }
}

/// Render the header with network info and era status.
fn render_header(frame: &mut Frame, app: &App, area: Rect) {
    let p = &app.palette;
    let network_style = Style::default().fg(p.accent).bold();
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
                .border_style(Style::default().fg(p.border))
                .title(" Staking Optimizer "),
        )
        .alignment(Alignment::Left);

    frame.render_widget(header, area);
}

/// Render the tab bar.
fn render_tabs(frame: &mut Frame, app: &App, area: Rect) {
    let p = &app.palette;
    let titles: Vec<Line> = View::all()
        .iter()
        .map(|v| Line::from(v.label()))
        .collect();

    let tabs = Tabs::new(titles)
        .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(p.border)))
        .select(app.current_view.index())
        .style(Style::default().fg(p.tab_inactive))
        .highlight_style(
            Style::default()
                .fg(p.tab_active)
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
        View::History => render_history(frame, app, area),
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
    let p = &app.palette;
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
            Line::from(Span::styled(format!("  {}", loading_text), Style::default().fg(p.fg_dim))),
        ];

        let paragraph = Paragraph::new(text).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(p.border))
                .title(" Validators "),
        );
        frame.render_widget(paragraph, area);
        return;
    }

    // Determine if we have space for full addresses (wide screen)
    let is_wide = area.width >= 120;
    let addr_width: u16 = if is_wide { 48 } else { 15 };

    // Build table rows
    let rows: Vec<Row> = app
        .validators
        .iter()
        .map(|v| {
            let addr_display = if is_wide {
                v.address.clone()
            } else {
                format!("{}...{}", &v.address[..6], &v.address[v.address.len() - 6..])
            };
            let name_display = v.name.as_deref().unwrap_or("-").to_string();
            let name_display = if name_display.len() > 20 {
                format!("{}...", &name_display[..17])
            } else {
                name_display
            };
            let commission_str = format!("{:.1}%", v.commission * 100.0);
            let stake_str = format_balance(v.total_stake, decimals);
            let own_str = format_balance(v.own_stake, decimals);
            let apy_str = format!("{:.2}%", v.apy * 100.0);
            let blocked_str = if v.blocked { "Yes" } else { "No" };

            Row::new(vec![
                Cell::from(name_display),
                Cell::from(addr_display),
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
        Cell::from("Name").style(Style::default().bold()),
        Cell::from("Address").style(Style::default().bold()),
        Cell::from("Comm").style(Style::default().bold()),
        Cell::from("Total Stake").style(Style::default().bold()),
        Cell::from("Own Stake").style(Style::default().bold()),
        Cell::from("Noms").style(Style::default().bold()),
        Cell::from("APY").style(Style::default().bold()),
        Cell::from("Blocked").style(Style::default().bold()),
    ])
    .style(Style::default().fg(p.highlight));

    let widths = [
        Constraint::Length(20),
        Constraint::Length(addr_width),
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
                .border_style(Style::default().fg(p.border))
                .title(format!(" Validators ({}) ", app.validators.len())),
        )
        .row_highlight_style(Style::default().fg(p.selection).add_modifier(Modifier::REVERSED))
        .highlight_symbol(">> ");

    frame.render_stateful_widget(table, area, &mut app.validators_table_state);
}

/// Render the nomination pools view with table.
fn render_pools(frame: &mut Frame, app: &mut App, area: Rect) {
    let pal = &app.palette;
    let decimals = app.network.token_decimals();

    if app.pools.is_empty() {
        let loading_text = if app.connection_status == ConnectionStatus::Connected {
            "Fetching nomination pools...".to_string()
        } else {
            "Waiting for connection...".to_string()
        };

        let text = vec![
            Line::from(""),
            Line::from(Span::styled(format!("  {}", loading_text), Style::default().fg(pal.fg_dim))),
        ];

        let paragraph = Paragraph::new(text).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(pal.border))
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
                PoolState::Open => Style::default().fg(pal.success),
                PoolState::Blocked => Style::default().fg(pal.warning),
                PoolState::Destroying => Style::default().fg(pal.error),
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
    .style(Style::default().fg(pal.highlight));

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
                .border_style(Style::default().fg(pal.border))
                .title(format!(" Nomination Pools ({}) ", app.pools.len())),
        )
        .row_highlight_style(Style::default().fg(pal.selection).add_modifier(Modifier::REVERSED))
        .highlight_symbol(">> ");

    frame.render_stateful_widget(table, area, &mut app.pools_table_state);
}

/// Render the nomination optimizer view.
fn render_nominate(frame: &mut Frame, app: &mut App, area: Rect) {
    let pal = &app.palette;
    let decimals = app.network.token_decimals();

    // Split into info panel and validator table
    let chunks = Layout::vertical([Constraint::Length(8), Constraint::Min(0)]).split(area);

    // Info panel
    let mut info_lines = Vec::new();
    info_lines.push(Line::from(""));

    // Show optimization result or manual selection info
    if let Some(result) = &app.optimization_result {
        info_lines.push(Line::from(vec![
            Span::styled("  Optimized Selection ", Style::default().fg(pal.success).bold()),
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
            Span::styled("  Manual Selection ", Style::default().fg(pal.warning).bold()),
            Span::raw(format!("({}/16 validators)", app.selected_validators.len())),
        ]));
    }

    info_lines.push(Line::from(""));
    info_lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled("o", Style::default().fg(pal.primary).bold()),
        Span::raw(": Optimize  "),
        Span::styled("Space", Style::default().fg(pal.primary).bold()),
        Span::raw(": Toggle  "),
        Span::styled("c", Style::default().fg(pal.primary).bold()),
        Span::raw(": Clear  "),
        Span::styled("g", Style::default().fg(pal.primary).bold()),
        Span::raw(": Generate QR"),
    ]));

    let info_panel = Paragraph::new(info_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(pal.border))
            .title(" Nomination Optimizer "),
    );
    frame.render_widget(info_panel, chunks[0]);

    // Validator table with selection checkboxes
    if app.validators.is_empty() {
        let loading = Paragraph::new("  Loading validators...")
            .style(Style::default().fg(pal.fg_dim))
            .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(pal.border)).title(" Select Validators "));
        frame.render_widget(loading, chunks[1]);
        return;
    }

    // Determine if we have space for full addresses (wide screen)
    let is_wide = chunks[1].width >= 100;
    let addr_width: u16 = if is_wide { 48 } else { 15 };

    let pal_success = pal.success;
    let rows: Vec<Row> = app
        .validators
        .iter()
        .enumerate()
        .map(|(idx, v)| {
            let selected = app.selected_validators.contains(&idx);
            let checkbox = if selected { "[x]" } else { "[ ]" };
            let checkbox_style = if selected {
                Style::default().fg(pal_success).bold()
            } else {
                Style::default()
            };

            let addr_display = if is_wide {
                v.address.clone()
            } else {
                format!("{}...{}", &v.address[..6], &v.address[v.address.len() - 6..])
            };
            let name_display = v.name.as_deref().unwrap_or("-").to_string();
            let name_display = if name_display.len() > 16 {
                format!("{}...", &name_display[..13])
            } else {
                name_display
            };
            let commission_str = format!("{:.1}%", v.commission * 100.0);
            let stake_str = format_balance(v.total_stake, decimals);
            let apy_str = format!("{:.2}%", v.apy * 100.0);
            let blocked_str = if v.blocked { "Yes" } else { "No" };

            Row::new(vec![
                Cell::from(checkbox).style(checkbox_style),
                Cell::from(name_display),
                Cell::from(addr_display),
                Cell::from(commission_str),
                Cell::from(stake_str),
                Cell::from(apy_str),
                Cell::from(blocked_str),
            ])
        })
        .collect();

    let header = Row::new(vec![
        Cell::from("Sel").style(Style::default().bold()),
        Cell::from("Name").style(Style::default().bold()),
        Cell::from("Address").style(Style::default().bold()),
        Cell::from("Comm").style(Style::default().bold()),
        Cell::from("Total Stake").style(Style::default().bold()),
        Cell::from("APY").style(Style::default().bold()),
        Cell::from("Blocked").style(Style::default().bold()),
    ])
    .style(Style::default().fg(pal.highlight));

    let widths = [
        Constraint::Length(5),
        Constraint::Length(16),
        Constraint::Length(addr_width),
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
                .border_style(Style::default().fg(pal.border))
                .title(format!(
                    " Select Validators ({} selected) ",
                    app.selected_validators.len()
                )),
        )
        .row_highlight_style(Style::default().fg(pal.selection).add_modifier(Modifier::REVERSED))
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

    let pal = &app.palette;

    let content_area = if app.input_mode == InputMode::EnteringAccount {
        // Render input box
        let input = Paragraph::new(app.account_input.as_str())
            .style(Style::default().fg(pal.highlight))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(pal.border))
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
                Span::styled("a", Style::default().fg(pal.highlight).bold()),
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
                Style::default().fg(pal.primary).bold(),
            )));
            lines.push(Line::from(vec![
                Span::raw("    Free:     "),
                Span::styled(
                    format!("{} {}", format_balance(status.balance.free, decimals), symbol),
                    Style::default().fg(pal.success),
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
                Style::default().fg(pal.primary).bold(),
            )));
            if let Some(ledger) = &status.staking_ledger {
                lines.push(Line::from(vec![
                    Span::raw("    Bonded: "),
                    Span::styled(
                        format!("{} {}", format_balance(ledger.active, decimals), symbol),
                        Style::default().fg(pal.highlight),
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
                            Style::default().fg(pal.accent),
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
                Style::default().fg(pal.primary).bold(),
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
                Style::default().fg(pal.primary).bold(),
            )));
            if let Some(membership) = &status.pool_membership {
                lines.push(Line::from(vec![
                    Span::raw("    Pool ID: "),
                    Span::styled(
                        membership.pool_id.to_string(),
                        Style::default().fg(pal.highlight),
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
                Span::styled("c", Style::default().fg(pal.highlight).bold()),
                Span::raw(" to clear account, "),
                Span::styled("a", Style::default().fg(pal.highlight).bold()),
                Span::raw(" to change"),
            ]));
        }
    }

    let paragraph = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(pal.border))
            .title(" Account Status "),
    );
    frame.render_widget(paragraph, content_area);
}

/// Render the staking history view with bar chart and table.
fn render_history(frame: &mut Frame, app: &App, area: Rect) {
    let pal = &app.palette;
    let chunks = Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            Constraint::Length(4), // Title/controls
            Constraint::Min(6),    // Main content (split horizontally)
            Constraint::Length(5), // Stats
        ])
        .split(area);

    // Title and controls
    let mut title_lines = Vec::new();

    if app.watched_account.is_none() {
        title_lines.push(Line::from(Span::styled(
            "Set account in Account tab (press 5, then 'a')",
            Style::default().fg(pal.warning),
        )));
    } else if app.loading_history {
        // Loading state
        let progress = app.staking_history.len() as f64 / app.history_total_eras as f64;
        let bar_width = 20;
        let filled = (progress * bar_width as f64) as usize;
        let bar = format!(
            "[{}{}] {}/{}",
            "█".repeat(filled),
            "░".repeat(bar_width - filled),
            app.staking_history.len(),
            app.history_total_eras
        );
        title_lines.push(Line::from(vec![
            Span::raw("Loading: "),
            Span::styled(bar, Style::default().fg(pal.success)),
        ]));
        title_lines.push(Line::from(vec![
            Span::raw("Press "),
            Span::styled("c", Style::default().fg(pal.warning).bold()),
            Span::raw(" to cancel"),
        ]));
    } else if app.staking_history.is_empty() {
        // Not loading, no data
        title_lines.push(Line::from(vec![
            Span::raw("Press "),
            Span::styled("l", Style::default().fg(pal.success).bold()),
            Span::raw(" to load staking history"),
        ]));
    } else {
        // Have data, not loading
        title_lines.push(Line::from(format!(
            "Loaded {} eras",
            app.staking_history.len()
        )));
        title_lines.push(Line::from(vec![
            Span::raw("Press "),
            Span::styled("l", Style::default().fg(pal.success).bold()),
            Span::raw(" to reload"),
        ]));
    }

    let title_para = Paragraph::new(title_lines)
        .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(pal.border)).title(" Staking History "))
        .alignment(Alignment::Center);
    frame.render_widget(title_para, chunks[0]);

    // Main content: split horizontally into bar chart (top) and table (bottom)
    if app.staking_history.is_empty() && !app.loading_history {
        let msg = if app.watched_account.is_none() {
            "No account set"
        } else {
            "Press 'l' to load history"
        };
        let paragraph = Paragraph::new(msg)
            .style(Style::default().fg(pal.muted))
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(pal.border)).title(" Era History "));
        frame.render_widget(paragraph, chunks[1]);
    } else if app.staking_history.is_empty() && app.loading_history {
        let paragraph = Paragraph::new("Loading first data points...")
            .style(Style::default().fg(pal.warning))
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(pal.border)).title(" Era History "));
        frame.render_widget(paragraph, chunks[1]);
    } else {
        // Split content area horizontally: bar chart on top, table below
        let content_chunks = Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([
                Constraint::Percentage(45), // Bar chart
                Constraint::Percentage(55), // Table
            ])
            .split(chunks[1]);

        render_reward_bar_chart(frame, app, content_chunks[0]);
        render_history_table(frame, app, content_chunks[1]);
    }

    // Stats
    render_history_stats(frame, app, chunks[2]);
}

/// Render daily DOT rewards as a bar chart.
fn render_reward_bar_chart(frame: &mut Frame, app: &App, area: Rect) {
    let pal = &app.palette;
    let decimals = app.network.token_decimals();
    let symbol = app.network.token_symbol();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(pal.border))
        .title(format!(" Daily {} Rewards ", symbol));

    let inner_area = block.inner(area);
    frame.render_widget(block, area);

    if app.staking_history.is_empty() || inner_area.height < 2 {
        return;
    }

    let graph_height = inner_area.height.saturating_sub(2) as usize; // Leave room for x-axis label
    let graph_width = inner_area.width.saturating_sub(10) as usize; // Leave room for y-axis labels

    if graph_width < 5 || graph_height < 2 {
        return;
    }

    // Get reward values (in token units for display)
    let divisor = 10u128.pow(decimals as u32);
    let rewards: Vec<f64> = app
        .staking_history
        .iter()
        .map(|p| p.reward as f64 / divisor as f64)
        .collect();

    if rewards.is_empty() {
        return;
    }

    // Y-axis starts from 0
    let max_reward = rewards.iter().cloned().fold(0.0_f64, f64::max);
    let range = max_reward.max(0.01);

    // Calculate how many data points per bar
    let num_bars = graph_width.min(rewards.len());
    let points_per_bar = (rewards.len() as f64 / num_bars as f64).max(1.0);

    // Build bars data
    let mut bar_heights: Vec<(f64, f64)> = Vec::new(); // (normalized height, actual value)
    for i in 0..num_bars {
        let start_idx = (i as f64 * points_per_bar) as usize;
        let end_idx = ((i + 1) as f64 * points_per_bar) as usize;
        let slice = &rewards[start_idx.min(rewards.len())..end_idx.min(rewards.len())];
        if !slice.is_empty() {
            let avg = slice.iter().sum::<f64>() / slice.len() as f64;
            let normalized = avg / range; // Normalize from 0
            bar_heights.push((normalized, avg));
        }
    }

    // Build lines for the chart
    let mut lines: Vec<Line> = Vec::new();

    for row in 0..graph_height {
        let y_pct = 1.0 - (row as f64 / graph_height as f64);
        let y_value = y_pct * range; // Y-axis from 0 to max
        let y_label = if y_value >= 1000.0 {
            format!("{:>6.0}k│", y_value / 1000.0)
        } else if y_value >= 1.0 {
            format!("{:>7.1}│", y_value)
        } else {
            format!("{:>7.3}│", y_value)
        };

        let mut line_spans = vec![Span::styled(y_label, Style::default().fg(pal.muted))];

        for (normalized, _) in &bar_heights {
            let bar_height = (normalized * graph_height as f64).ceil() as usize;
            let row_from_bottom = graph_height - 1 - row;

            let ch = if row_from_bottom < bar_height {
                '█'
            } else {
                ' '
            };

            // Color based on relative height
            let color = if *normalized > 0.7 {
                pal.graph_high
            } else if *normalized > 0.3 {
                pal.graph_mid
            } else {
                pal.graph_low
            };

            line_spans.push(Span::styled(ch.to_string(), Style::default().fg(color)));
        }

        lines.push(Line::from(line_spans));
    }

    // X-axis line
    let x_axis = format!("       └{}", "─".repeat(num_bars));
    lines.push(Line::from(Span::styled(x_axis, Style::default().fg(pal.muted))));

    // Era range label
    if let (Some(first), Some(last)) = (app.staking_history.first(), app.staking_history.last()) {
        let era_label = format!(
            "        Era {:<6} {:>width$}",
            first.era,
            last.era,
            width = num_bars.saturating_sub(10)
        );
        lines.push(Line::from(Span::styled(
            era_label,
            Style::default().fg(pal.muted),
        )));
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner_area);
}

/// Render history data as a table with Era, Date, Reward, and APY columns.
fn render_history_table(frame: &mut Frame, app: &App, area: Rect) {
    let pal = &app.palette;
    let decimals = app.network.token_decimals();
    let symbol = app.network.token_symbol();

    // Build table rows (most recent first)
    let rows: Vec<Row> = app
        .staking_history
        .iter()
        .rev()
        .map(|p| {
            let apy_str = format!("{:.2}%", p.apy * 100.0);
            let reward_str = format!("{} {}", format_balance(p.reward, decimals), symbol);

            // Color APY based on value
            let apy_style = if p.apy * 100.0 >= 15.0 {
                Style::default().fg(pal.graph_high)
            } else if p.apy * 100.0 >= 10.0 {
                Style::default().fg(pal.graph_mid)
            } else {
                Style::default().fg(pal.graph_low)
            };

            Row::new(vec![
                Cell::from(p.era.to_string()),
                Cell::from(p.date.clone()),
                Cell::from(reward_str),
                Cell::from(apy_str).style(apy_style),
            ])
        })
        .collect();

    let header = Row::new(vec![
        Cell::from("Era").style(Style::default().bold()),
        Cell::from("Date").style(Style::default().bold()),
        Cell::from(format!("Reward ({})", symbol)).style(Style::default().bold()),
        Cell::from("APY").style(Style::default().bold()),
    ])
    .style(Style::default().fg(pal.highlight));

    let widths = [
        Constraint::Length(8),
        Constraint::Length(12),
        Constraint::Length(18),
        Constraint::Length(10),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(pal.border))
                .title(format!(" Era History ({} eras) ", app.staking_history.len())),
        )
        .row_highlight_style(Style::default().fg(pal.selection).add_modifier(Modifier::REVERSED));

    frame.render_widget(table, area);
}

/// Render ASCII graph of APY history.
#[allow(dead_code)]
fn render_apy_graph(frame: &mut Frame, app: &App, area: Rect) {
    let pal = &app.palette;
    let inner = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(pal.border))
        .title(" APY % (30 day history) ");

    let inner_area = inner.inner(area);
    frame.render_widget(inner, area);

    if app.staking_history.is_empty() || inner_area.height < 3 {
        return;
    }

    let graph_height = inner_area.height.saturating_sub(1) as usize; // Leave room for x-axis
    let graph_width = inner_area.width.saturating_sub(6) as usize; // Leave room for y-axis labels

    // Get APY values
    let apys: Vec<f64> = app.staking_history.iter().map(|p| p.apy * 100.0).collect();

    if apys.is_empty() {
        return;
    }

    let min_apy = apys.iter().cloned().fold(f64::INFINITY, f64::min);
    let max_apy = apys.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let range = (max_apy - min_apy).max(0.1); // Avoid division by zero

    // Build graph lines
    let mut lines = Vec::new();

    // Y-axis labels and graph bars
    for row in 0..graph_height {
        let y_value = max_apy - (row as f64 / graph_height as f64) * range;
        let y_label = format!("{:>4.1}│", y_value);

        let mut line_spans = vec![Span::styled(y_label, Style::default().fg(pal.muted))];

        // Determine how many data points per column
        let points_per_col = (apys.len() as f64 / graph_width as f64).max(1.0);

        for col in 0..graph_width {
            let start_idx = (col as f64 * points_per_col) as usize;
            let end_idx = ((col + 1) as f64 * points_per_col) as usize;

            // Average APY for this column
            let col_apys: Vec<f64> =
                apys[start_idx.min(apys.len())..end_idx.min(apys.len())].to_vec();

            if col_apys.is_empty() {
                line_spans.push(Span::raw(" "));
                continue;
            }

            let avg_apy = col_apys.iter().sum::<f64>() / col_apys.len() as f64;
            let normalized = (avg_apy - min_apy) / range;
            let bar_height = (normalized * graph_height as f64) as usize;

            // Determine if this row should have a bar
            let row_from_bottom = graph_height - 1 - row;
            let ch = if row_from_bottom < bar_height {
                '█'
            } else if row_from_bottom == bar_height && normalized > 0.0 {
                '▄'
            } else {
                ' '
            };

            let color = if avg_apy >= 15.0 {
                pal.graph_high
            } else if avg_apy >= 10.0 {
                pal.graph_mid
            } else {
                pal.graph_low
            };

            line_spans.push(Span::styled(ch.to_string(), Style::default().fg(color)));
        }

        lines.push(Line::from(line_spans));
    }

    // X-axis
    let x_axis = format!(
        "    └{}",
        "─".repeat(graph_width)
    );
    lines.push(Line::from(Span::styled(x_axis, Style::default().fg(pal.muted))));

    // Era labels
    if let (Some(first), Some(last)) = (app.staking_history.first(), app.staking_history.last()) {
        let era_label = format!("     Era {:<10} {:>width$}", first.era, last.era, width = graph_width - 10);
        lines.push(Line::from(Span::styled(era_label, Style::default().fg(pal.muted))));
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner_area);
}

/// Render history statistics.
fn render_history_stats(frame: &mut Frame, app: &App, area: Rect) {
    let pal = &app.palette;
    let mut lines = Vec::new();

    if app.staking_history.is_empty() {
        lines.push(Line::from("  No history data available"));
    } else {
        let apys: Vec<f64> = app.staking_history.iter().map(|p| p.apy * 100.0).collect();
        let rewards: Vec<u128> = app.staking_history.iter().map(|p| p.reward).collect();

        let avg_apy = apys.iter().sum::<f64>() / apys.len() as f64;
        let min_apy = apys.iter().cloned().fold(f64::INFINITY, f64::min);
        let max_apy = apys.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let total_rewards: u128 = rewards.iter().sum();

        let decimals = app.network.token_decimals();
        let symbol = app.network.token_symbol();

        lines.push(Line::from(vec![
            Span::styled("  APY: ", Style::default().fg(pal.fg_dim)),
            Span::styled(
                format!("{:.2}%", avg_apy),
                Style::default().fg(pal.success).bold(),
            ),
            Span::styled(
                format!(" (min: {:.2}%, max: {:.2}%)", min_apy, max_apy),
                Style::default().fg(pal.muted),
            ),
        ]));

        lines.push(Line::from(vec![
            Span::styled("  Total Rewards: ", Style::default().fg(pal.fg_dim)),
            Span::styled(
                format!("{} {}", format_balance(total_rewards, decimals), symbol),
                Style::default().fg(pal.primary).bold(),
            ),
            Span::styled(
                format!(" over {} eras", app.staking_history.len()),
                Style::default().fg(pal.muted),
            ),
        ]));

        if let Some(last) = app.staking_history.last() {
            lines.push(Line::from(vec![
                Span::styled("  Latest Era: ", Style::default().fg(pal.fg_dim)),
                Span::styled(
                    format!("{}", last.era),
                    Style::default().fg(pal.fg),
                ),
                Span::styled(
                    format!(" | Reward: {} {}", format_balance(last.reward, decimals), symbol),
                    Style::default().fg(pal.muted),
                ),
            ]));
        }
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(pal.border))
        .title(" Statistics ");
    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

/// Render the QR code modal overlay with animated multipart support.
fn render_qr_modal(frame: &mut Frame, app: &App) {
    let pal = &app.palette;
    let area = frame.area();

    let mut lines = Vec::new();
    let mut qr_width: u16 = 0;

    // Available space for QR code (subtract borders, title, footer)
    let max_qr_height = area.height.saturating_sub(12) as usize; // Leave room for text
    let max_qr_width = area.width.saturating_sub(6) as usize;

    match &app.qr_data {
        Some(data) => {
            // Try to generate QR code for full data
            match QrCode::new(data) {
                Ok(qr) => {
                    let qr_lines = render_qr_halfblock(&qr);
                    let qr_h = qr_lines.len();
                    let qr_w = qr_lines.first().map(|l| l.chars().count()).unwrap_or(0);

                    // Check if QR fits in available space
                    if qr_h <= max_qr_height && qr_w <= max_qr_width {
                        // Single QR code fits - show normally
                        qr_width = qr_w as u16;
                        lines.push(Line::from(""));
                        lines.push(Line::from(Span::styled(
                            "Scan with Polkadot Vault",
                            Style::default().fg(pal.primary).bold(),
                        )));
                        lines.push(Line::from(""));
                        for line in qr_lines {
                            lines.push(Line::from(line));
                        }
                        lines.push(Line::from(""));
                        lines.push(Line::from(format!("Payload: {} bytes", data.len())));
                    } else {
                        // QR doesn't fit - use animated multipart
                        render_multipart_qr(
                            &mut lines,
                            &mut qr_width,
                            data,
                            max_qr_height,
                            max_qr_width,
                            app.qr_frame,
                            pal,
                        );
                    }
                }
                Err(e) => {
                    lines.push(Line::from(""));
                    lines.push(Line::from(Span::styled(
                        format!("Error generating QR: {}", e),
                        Style::default().fg(pal.error),
                    )));
                }
            }
        }
        None => {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "No QR data available",
                Style::default().fg(pal.warning),
            )));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::raw("Press "),
        Span::styled("Esc", Style::default().fg(pal.highlight).bold()),
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
                .border_style(Style::default().fg(pal.primary))
                .title(" QR Code "),
        )
        .style(Style::default().bg(pal.bg))
        .alignment(Alignment::Center);

    frame.render_widget(paragraph, modal_area);
}

/// Calculate chunk size that produces a QR code fitting in the given dimensions.
fn calculate_chunk_size(max_qr_height: usize, max_qr_width: usize) -> usize {
    // QR code size increases with data. Estimate based on typical QR dimensions.
    // A QR with ~50 bytes of data typically has ~25 modules (+ quiet zone = ~33).
    // Half-block rendering halves height, so 33 modules = ~17 lines.
    // Quiet zone adds 8 to width, so 25 + 8 = 33 chars wide.

    // Target a QR that fits comfortably
    let target_modules = max_qr_height.min(max_qr_width / 2).saturating_sub(8);

    // Rough mapping: modules ≈ 21 + 4*version, version ≈ ceil((data_bytes - 7) / 14)
    // For version 1 (21 modules): ~17 bytes alphanumeric
    // For version 2 (25 modules): ~32 bytes
    // For version 3 (29 modules): ~53 bytes
    // For version 4 (33 modules): ~78 bytes

    if target_modules >= 33 {
        60 // Comfortable for version 4
    } else if target_modules >= 29 {
        40 // Version 3
    } else if target_modules >= 25 {
        25 // Version 2
    } else {
        15 // Version 1, smallest practical
    }
}

/// Render animated multipart QR codes for data that doesn't fit in a single QR.
fn render_multipart_qr(
    lines: &mut Vec<Line<'static>>,
    qr_width: &mut u16,
    data: &[u8],
    max_qr_height: usize,
    max_qr_width: usize,
    current_frame: usize,
    pal: &Palette,
) {
    use base64::{engine::general_purpose::STANDARD, Engine};

    // Calculate appropriate chunk size
    let chunk_size = calculate_chunk_size(max_qr_height, max_qr_width);

    // Encode data as base64 for QR (QR handles alphanumeric better)
    let b64_data = STANDARD.encode(data);

    // Calculate number of parts needed
    // Each part needs: "p<n>of<total>:" prefix (~10 chars) + data
    let usable_chunk = chunk_size.saturating_sub(12);
    let total_parts = (b64_data.len() + usable_chunk - 1) / usable_chunk.max(1);
    let total_parts = total_parts.max(1);

    // Get current frame (cycle through parts)
    let frame = current_frame % total_parts;

    // Extract chunk for this frame
    let start = frame * usable_chunk;
    let end = (start + usable_chunk).min(b64_data.len());
    let chunk = &b64_data[start..end];

    // Format: "p<part>of<total>:<data>"
    let part_data = format!("p{}of{}:{}", frame + 1, total_parts, chunk);

    // Copy colors to avoid borrow issues with closures
    let primary = pal.primary;
    let warning = pal.warning;
    let success = pal.success;
    let error = pal.error;

    match QrCode::new(&part_data) {
        Ok(qr) => {
            let qr_lines = render_qr_halfblock(&qr);
            *qr_width = qr_lines.first().map(|l| l.chars().count() as u16).unwrap_or(0);

            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Scan with Polkadot Vault",
                Style::default().fg(primary).bold(),
            )));
            lines.push(Line::from(Span::styled(
                format!("(Animated: part {}/{})", frame + 1, total_parts),
                Style::default().fg(warning),
            )));
            lines.push(Line::from(""));

            for line in qr_lines {
                lines.push(Line::from(line));
            }

            lines.push(Line::from(""));
            lines.push(Line::from(format!(
                "Total: {} bytes ({} parts)",
                data.len(),
                total_parts
            )));

            // Progress bar for animation
            let progress_width = 20usize;
            let filled = ((frame + 1) * progress_width) / total_parts;
            let bar: String = "█".repeat(filled) + &"░".repeat(progress_width - filled);
            lines.push(Line::from(Span::styled(
                format!("[{}]", bar),
                Style::default().fg(success),
            )));
        }
        Err(e) => {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("Error generating QR part: {}", e),
                Style::default().fg(error),
            )));
        }
    }
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
fn render_help_modal(frame: &mut Frame, app: &App) {
    let pal = &app.palette;
    let area = frame.area();

    // Calculate centered modal area
    let modal_width = 55.min(area.width.saturating_sub(4));
    let modal_height = 33.min(area.height.saturating_sub(4));
    let modal_x = (area.width.saturating_sub(modal_width)) / 2;
    let modal_y = (area.height.saturating_sub(modal_height)) / 2;
    let modal_area = Rect::new(modal_x, modal_y, modal_width, modal_height);

    // Clear the background
    frame.render_widget(Clear, modal_area);

    let key_style = Style::default().fg(pal.highlight).bold();
    let desc_style = Style::default();

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  Global Keys",
            Style::default().fg(pal.primary).bold(),
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
            Style::default().fg(pal.primary).bold(),
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
            Style::default().fg(pal.primary).bold(),
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
            Style::default().fg(pal.primary).bold(),
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
                .border_style(Style::default().fg(pal.success))
                .title(" Keyboard Shortcuts "),
        )
        .style(Style::default().bg(pal.bg));

    frame.render_widget(paragraph, modal_area);
}

/// Render the log viewer with connection status.
fn render_logs(frame: &mut Frame, app: &App, area: Rect) {
    let p = &app.palette;
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
        ConnectionStatus::Disconnected => Style::default().fg(p.error),
        ConnectionStatus::Connecting => Style::default().fg(p.warning),
        ConnectionStatus::Syncing { .. } => Style::default().fg(p.warning),
        ConnectionStatus::Connected => Style::default().fg(p.success),
        ConnectionStatus::Error(_) => Style::default().fg(p.error),
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
                LogLevel::Trace => Style::default().fg(p.muted),
                LogLevel::Debug => Style::default().fg(p.primary),
                LogLevel::Info => Style::default().fg(p.success),
                LogLevel::Warn => Style::default().fg(p.warning),
                LogLevel::Error => Style::default().fg(p.error),
            };

            // Shorten target if too long
            let target = if log.target.len() > 20 {
                format!("..{}", &log.target[log.target.len() - 18..])
            } else {
                log.target.clone()
            };

            Line::from(vec![
                Span::styled(format!("{:5} ", log.level.as_str()), level_style),
                Span::styled(format!("[{}] ", target), Style::default().fg(p.muted)),
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
        Span::styled(format!("({}) ", log_count), Style::default().fg(p.muted)),
        Span::raw("│ "),
        Span::styled(status_text, status_style),
        Span::styled(scroll_info, Style::default().fg(p.muted)),
        Span::raw(" │ PgUp/PgDn:Scroll  ?:Help  q:Quit "),
    ]);

    let paragraph = Paragraph::new(display_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(p.border))
            .title(title),
    );

    frame.render_widget(paragraph, area);
}
