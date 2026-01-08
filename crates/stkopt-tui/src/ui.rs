//! UI rendering.

use crate::app::{App, InputMode, PoolSortField, ValidatorSortField, View};
use crate::log_buffer::LogLevel;
use crate::theme::Palette;
use qrcode::{EcLevel, QrCode, Version};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, Tabs},
};
use stkopt_chain::PoolState;
use stkopt_core::ConnectionStatus;

/// Safely truncate a string to a maximum number of characters (not bytes).
/// Handles multi-byte Unicode characters correctly.
fn truncate_str(s: &str, max_chars: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_chars {
        s.to_string()
    } else if max_chars <= 3 {
        s.chars().take(max_chars).collect()
    } else {
        let truncated: String = s.chars().take(max_chars - 3).collect();
        format!("{}...", truncated)
    }
}

/// Render the entire UI.
pub fn render(frame: &mut Frame, app: &mut App) {
    let chunks = Layout::vertical([
        Constraint::Length(3),  // Header
        Constraint::Length(3),  // Tabs
        Constraint::Min(0),     // Content
        Constraint::Length(12), // Log viewer (10 lines + border)
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

    // Render sort menu overlay
    if app.input_mode == InputMode::SortMenu {
        render_sort_menu(frame, app);
    }

    // Render strategy menu overlay
    if app.input_mode == InputMode::StrategyMenu {
        render_strategy_menu(frame, app);
    }

    // Render account prompt popup if showing
    if app.show_account_prompt {
        render_account_prompt(frame, app);
    }

    // Render loading spinner overlay if chain is connecting and no cached data
    if app.loading_chain && app.validators.is_empty() {
        render_loading_spinner(frame, app);
    }
}

/// Render the loading spinner overlay with progress bar and ETA.
fn render_loading_spinner(frame: &mut Frame, app: &App) {
    let p = &app.palette;
    let area = frame.area();

    // Center a box
    let spinner_width = 50;
    let spinner_height = 7;
    let x = (area.width.saturating_sub(spinner_width)) / 2;
    let y = (area.height.saturating_sub(spinner_height)) / 2;
    let spinner_area = Rect::new(x, y, spinner_width, spinner_height);

    // Clear background
    frame.render_widget(Clear, spinner_area);

    // Spinner message
    let spinner = app.spinner_char();
    let message = format!("{} Connecting to {}...", spinner, app.network);

    // Build progress bar
    let progress_width = 30usize;
    let filled = (app.loading_progress * progress_width as f32) as usize;
    let bar: String = "█".repeat(filled) + &"░".repeat(progress_width - filled);

    // ETA or bandwidth info
    let eta_text = if let Some(eta) = app.format_eta() {
        format!("ETA: {}", eta)
    } else if let Some(bw) = app.estimated_bandwidth {
        format!("Speed: {:.1} KB/s", bw / 1024.0)
    } else {
        "Light client syncing, please wait".to_string()
    };

    let text = vec![
        Line::from(message).style(Style::default().fg(p.accent).bold()),
        Line::from(""),
        Line::from(Span::styled(
            format!("[{}] {:.0}%", bar, app.loading_progress * 100.0),
            Style::default().fg(p.success),
        )),
        Line::from(""),
        Line::from(eta_text).style(Style::default().fg(p.muted)),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(p.border))
        .style(Style::default().bg(p.bg));

    let paragraph = Paragraph::new(text)
        .block(block)
        .alignment(ratatui::layout::Alignment::Center);

    frame.render_widget(paragraph, spinner_area);
}

/// Render the account prompt popup.
fn render_account_prompt(frame: &mut Frame, app: &App) {
    let p = &app.palette;
    let area = frame.area();

    // Center a box
    let prompt_width = 60.min(area.width.saturating_sub(4));
    let prompt_height = 9;
    let x = (area.width.saturating_sub(prompt_width)) / 2;
    let y = (area.height.saturating_sub(prompt_height)) / 2;
    let prompt_area = Rect::new(x, y, prompt_width, prompt_height);

    // Clear background
    frame.render_widget(Clear, prompt_area);

    let text = vec![
        Line::from("No account configured").style(Style::default().fg(p.accent).bold()),
        Line::from(""),
        Line::from("Press 'a' to enter your stash account address"),
        Line::from("or scan a QR code from Polkadot Vault."),
        Line::from(""),
        Line::from("Press 'q' to quit").style(Style::default().fg(p.muted)),
    ];

    let block = Block::default()
        .title(" Welcome to Staking Optimizer ")
        .title_style(Style::default().fg(p.accent).bold())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(p.border))
        .style(Style::default().bg(p.bg));

    let paragraph = Paragraph::new(text)
        .block(block)
        .alignment(ratatui::layout::Alignment::Center);

    frame.render_widget(paragraph, prompt_area);
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

    // Build chain info display
    let chain_display = if let Some(info) = &app.chain_info {
        let style = if info.validated {
            Style::default().fg(p.success)
        } else {
            Style::default().fg(p.warning)
        };
        vec![
            Span::raw("  │  "),
            Span::styled(format!("{} v{}", info.spec_name, info.spec_version), style),
        ]
    } else {
        vec![]
    };

    let mut spans = vec![
        Span::styled(format!("[{}] ", symbol), network_style),
        Span::raw(app.network.to_string()),
        Span::raw("  │  "),
        Span::raw(era_info),
    ];
    spans.extend(chain_display);

    let header_text = Line::from(spans);

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
    let titles: Vec<Line> = View::all().iter().map(|v| Line::from(v.label())).collect();

    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(p.border)),
        )
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
    }
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

    // Split area if searching
    let (search_area, table_area) = if app.input_mode == InputMode::Searching {
        let chunks = Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).split(area);
        (Some(chunks[0]), chunks[1])
    } else {
        (None, area)
    };

    // Render search bar if in search mode
    if let Some(search_area) = search_area {
        let search_text = format!("/{}", app.search_query);
        let search = Paragraph::new(search_text)
            .style(Style::default().fg(p.highlight))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(p.primary))
                    .title(" Search (Enter to confirm, Esc to cancel) "),
            );
        frame.render_widget(search, search_area);
    }

    if app.validators.is_empty() {
        let loading_text = if app.loading_validators {
            format!("Loading validators... {:.0}%", app.loading_progress * 100.0)
        } else if app.connection_status == ConnectionStatus::Connected {
            "Fetching validators...".to_string()
        } else {
            "Waiting for connection...".to_string()
        };

        let text = vec![
            Line::from(""),
            Line::from(Span::styled(
                format!("  {}", loading_text),
                Style::default().fg(p.fg_dim),
            )),
        ];

        let paragraph = Paragraph::new(text).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(p.border))
                .title(" Validators | /:Search  s:Sort  b:Blocked  ?:Help "),
        );
        frame.render_widget(paragraph, table_area);
        return;
    }

    // Get filtered and sorted validators
    let filtered = app.filtered_validators();
    let filtered_count = filtered.len();

    // Determine if we have space for full addresses (wide screen)
    let is_wide = table_area.width >= 120;
    let addr_width: u16 = if is_wide { 48 } else { 15 };

    // Build table rows from filtered validators
    let rows: Vec<Row> = filtered
        .iter()
        .map(|v| {
            let addr_display = if is_wide {
                v.address.clone()
            } else {
                format!(
                    "{}...{}",
                    &v.address[..6],
                    &v.address[v.address.len() - 6..]
                )
            };
            let name_display = truncate_str(v.name.as_deref().unwrap_or("-"), 20);
            let commission_str = format!("{:.1}%", v.commission * 100.0);
            let stake_str = format_balance(v.total_stake, decimals);
            let own_str = format_balance(v.own_stake, decimals);
            let points_str = v.points.to_string();
            let apy_str = format!("{:.2}%", v.apy * 100.0);
            let blocked_str = if v.blocked { "Yes" } else { "No" };

            Row::new(vec![
                Cell::from(name_display),
                Cell::from(addr_display),
                Cell::from(commission_str),
                Cell::from(stake_str),
                Cell::from(own_str),
                Cell::from(points_str),
                Cell::from(v.nominator_count.to_string()),
                Cell::from(apy_str),
                Cell::from(blocked_str),
            ])
        })
        .collect();

    // Build header with sort indicator
    let sort_indicator = |field: ValidatorSortField| {
        if app.validator_sort == field {
            if app.validator_sort_asc {
                " ▲"
            } else {
                " ▼"
            }
        } else {
            ""
        }
    };

    let header = Row::new(vec![
        Cell::from(format!("Name{}", sort_indicator(ValidatorSortField::Name)))
            .style(Style::default().bold()),
        Cell::from(format!(
            "Address{}",
            sort_indicator(ValidatorSortField::Address)
        ))
        .style(Style::default().bold()),
        Cell::from(format!(
            "Comm{}",
            sort_indicator(ValidatorSortField::Commission)
        ))
        .style(Style::default().bold()),
        Cell::from(format!(
            "Total Stake{}",
            sort_indicator(ValidatorSortField::TotalStake)
        ))
        .style(Style::default().bold()),
        Cell::from(format!(
            "Own Stake{}",
            sort_indicator(ValidatorSortField::OwnStake)
        ))
        .style(Style::default().bold()),
        Cell::from(format!(
            "Points{}",
            sort_indicator(ValidatorSortField::Points)
        ))
        .style(Style::default().bold()),
        Cell::from(format!(
            "Noms{}",
            sort_indicator(ValidatorSortField::Nominators)
        ))
        .style(Style::default().bold()),
        Cell::from(format!("APY{}", sort_indicator(ValidatorSortField::Apy)))
            .style(Style::default().bold()),
        Cell::from(format!(
            "Blocked{}",
            sort_indicator(ValidatorSortField::Blocked)
        ))
        .style(Style::default().bold()),
    ])
    .style(Style::default().fg(p.highlight));

    let widths = [
        Constraint::Length(22),
        Constraint::Length(addr_width),
        Constraint::Length(7),
        Constraint::Length(14),
        Constraint::Length(12),
        Constraint::Length(10),
        Constraint::Length(8),
        Constraint::Length(10),
        Constraint::Length(10),
    ];

    // Build title with filter info
    let mut title_parts = vec![format!(" Validators ({}", filtered_count)];
    if filtered_count != app.validators.len() {
        title_parts.push(format!("/{}", app.validators.len()));
    }
    title_parts.push(") ".to_string());
    if !app.search_query.is_empty() {
        title_parts.push(format!("[filter: {}] ", app.search_query));
    }
    if !app.show_blocked {
        title_parts.push("[hiding blocked] ".to_string());
    }
    title_parts.push("| /:Search  s:Sort  b:Blocked  S:Reverse  ?:Help ".to_string());
    let title = title_parts.join("");

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(p.border))
                .title(title),
        )
        .row_highlight_style(
            Style::default()
                .fg(p.selection)
                .add_modifier(Modifier::REVERSED),
        )
        .highlight_symbol(">> ");

    frame.render_stateful_widget(table, table_area, &mut app.validators_table_state);
}

/// Render the nomination pools view with table.
fn render_pools(frame: &mut Frame, app: &mut App, area: Rect) {
    let pal = &app.palette;
    let decimals = app.network.token_decimals();

    // Split area if searching
    let (search_area, table_area) = if app.input_mode == InputMode::Searching {
        let chunks = Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).split(area);
        (Some(chunks[0]), chunks[1])
    } else {
        (None, area)
    };

    // Render search bar if in search mode
    if let Some(search_area) = search_area {
        let search_text = format!("/{}", app.search_query);
        let search = Paragraph::new(search_text)
            .style(Style::default().fg(pal.highlight))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(pal.primary))
                    .title(" Search (Enter to confirm, Esc to cancel) "),
            );
        frame.render_widget(search, search_area);
    }

    if app.pools.is_empty() {
        let loading_text = if app.connection_status == ConnectionStatus::Connected {
            "Fetching nomination pools...".to_string()
        } else {
            "Waiting for connection...".to_string()
        };

        let text = vec![
            Line::from(""),
            Line::from(Span::styled(
                format!("  {}", loading_text),
                Style::default().fg(pal.fg_dim),
            )),
        ];

        let paragraph = Paragraph::new(text).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(pal.border))
                .title(" Nomination Pools | /:Search  s:Sort  ?:Help "),
        );
        frame.render_widget(paragraph, table_area);
        return;
    }

    // Get filtered and sorted pools
    let filtered = app.filtered_pools();
    let filtered_count = filtered.len();

    // Build table rows from filtered pools
    let rows: Vec<Row> = filtered
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
            } else {
                truncate_str(&p.name, 30)
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

    // Build header with sort indicator
    let sort_indicator = |field: PoolSortField| {
        if app.pool_sort == field {
            if app.pool_sort_asc { " ▲" } else { " ▼" }
        } else {
            ""
        }
    };

    let header = Row::new(vec![
        Cell::from(format!("ID{}", sort_indicator(PoolSortField::Id)))
            .style(Style::default().bold()),
        Cell::from(format!("Name{}", sort_indicator(PoolSortField::Name)))
            .style(Style::default().bold()),
        Cell::from(format!("State{}", sort_indicator(PoolSortField::State)))
            .style(Style::default().bold()),
        Cell::from(format!("Members{}", sort_indicator(PoolSortField::Members)))
            .style(Style::default().bold()),
        Cell::from(format!("Points{}", sort_indicator(PoolSortField::Points)))
            .style(Style::default().bold()),
        Cell::from(format!("APY{}", sort_indicator(PoolSortField::Apy)))
            .style(Style::default().bold()),
    ])
    .style(Style::default().fg(pal.highlight));

    let widths = [
        Constraint::Length(8),
        Constraint::Min(25),
        Constraint::Length(14),
        Constraint::Length(12),
        Constraint::Length(16),
        Constraint::Length(10),
    ];

    // Build title with filter info
    let mut title_parts = vec![format!(" Nomination Pools ({}", filtered_count)];
    if filtered_count != app.pools.len() {
        title_parts.push(format!("/{}", app.pools.len()));
    }
    title_parts.push(") ".to_string());
    if !app.search_query.is_empty() {
        title_parts.push(format!("[filter: {}] ", app.search_query));
    }
    title_parts.push("| /:Search  s:Sort  S:Reverse  ?:Help ".to_string());
    let title = title_parts.join("");

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(pal.border))
                .title(title),
        )
        .row_highlight_style(
            Style::default()
                .fg(pal.selection)
                .add_modifier(Modifier::REVERSED),
        )
        .highlight_symbol(">> ");

    frame.render_stateful_widget(table, table_area, &mut app.pools_table_state);
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
            Span::styled(
                "  Optimized Selection ",
                Style::default().fg(pal.success).bold(),
            ),
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
            Span::styled(
                "  Manual Selection ",
                Style::default().fg(pal.warning).bold(),
            ),
            Span::raw(format!("({}/16 validators)", app.selected_validators.len())),
        ]));
    }

    info_lines.push(Line::from(""));
    info_lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled("o", Style::default().fg(pal.primary).bold()),
        Span::raw(": Optimize  "),
        Span::styled("t", Style::default().fg(pal.primary).bold()),
        Span::raw(": Strategy  "),
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
            .title(" Nomination Optimizer | Space:Toggle  o:Optimize  t:Strategy  g:QR  c:Clear "),
    );
    frame.render_widget(info_panel, chunks[0]);

    // Validator table with selection checkboxes
    if app.validators.is_empty() {
        let loading = Paragraph::new("  Loading validators...")
            .style(Style::default().fg(pal.fg_dim))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(pal.border))
                    .title(" Select Validators "),
            );
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
                format!(
                    "{}...{}",
                    &v.address[..6],
                    &v.address[v.address.len() - 6..]
                )
            };
            let name_display = truncate_str(v.name.as_deref().unwrap_or("-"), 16);
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
        .row_highlight_style(
            Style::default()
                .fg(pal.selection)
                .add_modifier(Modifier::REVERSED),
        )
        .highlight_symbol(">> ");

    frame.render_stateful_widget(table, chunks[1], &mut app.nominate_table_state);
}

/// Render the account status view.
fn render_account(frame: &mut Frame, app: &mut App, area: Rect) {
    let decimals = app.network.token_decimals();
    let symbol = app.network.token_symbol();
    let pal = &app.palette;

    // Layout:
    // Top: Input (if entering)
    // Main: Split horizontally (Left: Info, Right: Address Book)

    let main_area = if app.input_mode == InputMode::EnteringAccount {
        let input_height = if app.validation_error.is_some() { 5 } else { 3 };
        let chunks =
            Layout::vertical([Constraint::Length(input_height), Constraint::Min(0)]).split(area);

        // Render input box
        let input = Paragraph::new(app.account_input.as_str())
            .style(Style::default().fg(pal.highlight))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(pal.primary))
                    .title(" Enter SS58 Address (Enter to confirm, Esc to cancel) "),
            );
        frame.render_widget(input, chunks[0]);

        // Render validation error if present
        if let Some(ref error) = app.validation_error {
            let error_lines = vec![Line::from(vec![
                Span::styled("✗ ", Style::default().fg(pal.error)),
                Span::styled(error, Style::default().fg(pal.error)),
            ])];
            let error_para = Paragraph::new(error_lines)
                .style(Style::default().fg(pal.error))
                .block(
                    Block::default()
                        .borders(Borders::NONE)
                        .style(Style::default().bg(pal.bg)),
                );
            frame.render_widget(error_para, chunks[1]);
            chunks[2]
        } else {
            chunks[1]
        }
    } else {
        area
    };

    let chunks =
        Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).split(main_area);

    let left_area = chunks[0];
    let right_area = chunks[1];

    // Determine focus colors
    let left_border = if app.account_panel_focus == 0 {
        pal.primary
    } else {
        pal.border
    };
    let right_border = if app.account_panel_focus == 1 {
        pal.primary
    } else {
        pal.border
    };

    // --- Left Panel: Account Status ---
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
            lines.push(Line::from(Span::styled(
                "  Loading account data...",
                Style::default().fg(pal.warning),
            )));
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

            // Calculate detailed split
            let free = status.balance.free;
            let reserved = status.balance.reserved;
            let frozen = status.balance.frozen;
            let total = free + reserved;
            // Transferable is usually free - frozen (simplified)
            let transferable = free.saturating_sub(frozen);
            let bonded = if let Some(l) = &status.staking_ledger {
                l.active
            } else {
                0
            };

            lines.push(Line::from(vec![
                Span::raw("    Total:        "),
                Span::styled(
                    format!("{} {}", format_balance(total, decimals), symbol),
                    Style::default().bold(),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::raw("    Transferable: "),
                Span::styled(
                    format!("{} {}", format_balance(transferable, decimals), symbol),
                    Style::default().fg(pal.success),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::raw("    Locked:       "),
                Span::raw(format!(
                    "{} {}",
                    format_balance(frozen.max(reserved), decimals),
                    symbol
                )),
            ]));
            lines.push(Line::from(vec![
                Span::raw("    Bonded:       "),
                Span::styled(
                    format!("{} {}", format_balance(bonded, decimals), symbol),
                    Style::default().fg(pal.accent),
                ),
            ]));
            lines.push(Line::from(""));

            // Staking section
            lines.push(Line::from(Span::styled(
                "  Staking",
                Style::default().fg(pal.primary).bold(),
            )));
            if let Some(ledger) = &status.staking_ledger {
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
                } else {
                    lines.push(Line::from("    No unlocking chunks"));
                }
            } else {
                lines.push(Line::from("    Not staking directly"));
            }
            lines.push(Line::from(""));

            // Nominations
            lines.push(Line::from(Span::styled(
                "  Nominations",
                Style::default().fg(pal.primary).bold(),
            )));
            if let Some(nominations) = &status.nominations {
                lines.push(Line::from(format!(
                    "    {} validators (era {})",
                    nominations.targets.len(),
                    nominations.submitted_in
                )));
            } else {
                lines.push(Line::from("    No nominations"));
            }

            // Pool
            lines.push(Line::from(Span::styled(
                "  Nomination Pool",
                Style::default().fg(pal.primary).bold(),
            )));
            if let Some(membership) = &status.pool_membership {
                // Look up pool name from pools list
                let pool_name = app
                    .pools
                    .iter()
                    .find(|p| p.id == membership.pool_id)
                    .map(|p| p.name.as_str());
                let pool_display = match pool_name {
                    Some(name) if !name.is_empty() => format!(
                        "    Member of Pool {} ({})",
                        membership.pool_id, name
                    ),
                    _ => format!("    Member of Pool {}", membership.pool_id),
                };
                lines.push(Line::from(pool_display));
                lines.push(Line::from(format!(
                    "    {}: {}",
                    app.network.token_symbol(),
                    format_balance(membership.points, decimals)
                )));
            } else {
                lines.push(Line::from("    Not a pool member"));
            }

            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::raw("  Press "),
                Span::styled("c", Style::default().fg(pal.highlight).bold()),
                Span::raw(" to clear, "),
                Span::styled("a", Style::default().fg(pal.highlight).bold()),
                Span::raw(" to change"),
            ]));
        }
    }

    // Build title with focus hint
    let status_title = if app.account_panel_focus == 0 {
        " Account Status [←→:Switch Panel] | a:Change  c:Clear "
    } else {
        " Account Status | a:Change  c:Clear "
    };

    let status_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(left_border))
        .title(status_title);

    let status_para = Paragraph::new(lines).block(status_block);
    frame.render_widget(status_para, left_area);

    // --- Right Panel: Address Book ---
    // Hardcoded list for now
    let known_addresses: Vec<(&str, &str)> = vec![
        (
            "Polkadot Treasury",
            "13UVJyLnbVp9RBZYFwCNuGnK87JYJ2nb7jMwaVe4vQ2UNCzN",
        ),
        (
            "Polkadot Fellowship",
            "16SpacegeUTft9v3ts27CEC3tJaxgvE4uZeCctThFH3Vb24p",
        ),
        (
            "Snowbridge",
            "13cKp89Nt7t1hZVWnqhKW9LY7Udhxk2BmLwKi3snVgUAjZGE",
        ),
    ];

    let mut address_rows = Vec::new();
    // Add user account
    if let Some(account) = &app.watched_account {
        address_rows.push(Row::new(vec![
            Cell::from("My Account").style(Style::default().bold().fg(pal.success)),
            Cell::from(account.to_string()),
        ]));
    }

    // Add known addresses
    for (name, addr) in &known_addresses {
        address_rows.push(Row::new(vec![Cell::from(*name), Cell::from(*addr)]));
    }

    // Build title with focus hint
    let address_title = if app.account_panel_focus == 1 {
        " Addresses [←→:Switch Panel  Enter:Select] | ↑↓:Navigate "
    } else {
        " Addresses "
    };

    let address_table = Table::new(
        address_rows,
        [Constraint::Percentage(30), Constraint::Percentage(70)],
    )
    .header(Row::new(vec!["Name", "Address"]).style(Style::default().fg(pal.muted)))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(right_border))
            .title(address_title),
    )
    .row_highlight_style(
        Style::default()
            .fg(pal.selection)
            .add_modifier(Modifier::REVERSED),
    )
    .highlight_symbol(">> ");

    frame.render_stateful_widget(address_table, right_area, &mut app.address_book_state);
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
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(pal.border))
                .title(" Staking History | l:Load  c:Cancel  q:Quit "),
        )
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
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(pal.border))
                    .title(" Era History "),
            );
        frame.render_widget(paragraph, chunks[1]);
    } else if app.staking_history.is_empty() && app.loading_history {
        let paragraph = Paragraph::new("Loading first data points...")
            .style(Style::default().fg(pal.warning))
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(pal.border))
                    .title(" Era History "),
            );
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

/// Calculate nice Y-axis tick values.
fn calculate_y_ticks(max_value: f64, num_ticks: usize) -> Vec<f64> {
    if max_value <= 0.0 || num_ticks == 0 {
        return vec![0.0];
    }

    // Find a "nice" step size
    let raw_step = max_value / (num_ticks as f64);
    let magnitude = 10f64.powf(raw_step.log10().floor());
    let normalized = raw_step / magnitude;

    let nice_step = if normalized <= 1.0 {
        1.0 * magnitude
    } else if normalized <= 2.0 {
        2.0 * magnitude
    } else if normalized <= 5.0 {
        5.0 * magnitude
    } else {
        10.0 * magnitude
    };

    let nice_max = (max_value / nice_step).ceil() * nice_step;
    let actual_ticks = (nice_max / nice_step) as usize + 1;

    (0..actual_ticks).map(|i| i as f64 * nice_step).collect()
}

/// Calculate 7-day moving average for trend line.
fn calculate_trend(values: &[f64], window: usize) -> Vec<f64> {
    if values.is_empty() || window == 0 {
        return vec![];
    }

    let mut result = Vec::with_capacity(values.len());
    for i in 0..values.len() {
        let start = i.saturating_sub(window / 2);
        let end = (i + window / 2 + 1).min(values.len());
        let slice = &values[start..end];
        let avg = slice.iter().sum::<f64>() / slice.len() as f64;
        result.push(avg);
    }
    result
}

/// Render daily DOT rewards as a bar chart with trend line.
fn render_reward_bar_chart(frame: &mut Frame, app: &App, area: Rect) {
    let pal = &app.palette;
    let decimals = app.network.token_decimals();
    let symbol = app.network.token_symbol();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(pal.border))
        .title(format!(" Daily {} Rewards (━ 7-era trend) ", symbol));

    let inner_area = block.inner(area);
    frame.render_widget(block, area);

    if app.staking_history.is_empty() || inner_area.height < 4 {
        return;
    }

    // Calculate dimensions with 1:2 ratio (height = width / 2)
    let y_axis_width = 8usize; // Width for Y-axis labels
    let available_width = inner_area.width.saturating_sub(y_axis_width as u16) as usize;
    let max_graph_height = inner_area.height.saturating_sub(3) as usize; // Room for x-axis
    let target_height = (available_width / 2).min(max_graph_height);
    let graph_height = target_height.max(4);
    let graph_width = available_width;

    if graph_width < 10 || graph_height < 3 {
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

    // Calculate trend line (7-era moving average)
    let trend = calculate_trend(&rewards, 7);

    // Y-axis: find nice ticks
    let max_reward = rewards.iter().cloned().fold(0.0_f64, f64::max);
    let y_ticks = calculate_y_ticks(max_reward, 5);
    let y_max = *y_ticks.last().unwrap_or(&max_reward.max(0.01));

    // Calculate bar width with spacing (2 chars per bar: 1 bar + 1 space)
    let bar_spacing = 2usize;
    let num_bars = (graph_width / bar_spacing).min(rewards.len());
    let points_per_bar = (rewards.len() as f64 / num_bars as f64).max(1.0);

    // Build bars data and trend points
    let mut bar_data: Vec<(f64, f64)> = Vec::new(); // (normalized height, actual value)
    let mut trend_data: Vec<f64> = Vec::new();

    for i in 0..num_bars {
        let start_idx = (i as f64 * points_per_bar) as usize;
        let end_idx = ((i + 1) as f64 * points_per_bar) as usize;
        let slice = &rewards[start_idx.min(rewards.len())..end_idx.min(rewards.len())];
        let trend_slice = &trend[start_idx.min(trend.len())..end_idx.min(trend.len())];

        if !slice.is_empty() {
            let avg = slice.iter().sum::<f64>() / slice.len() as f64;
            let normalized = (avg / y_max).min(1.0);
            bar_data.push((normalized, avg));

            if !trend_slice.is_empty() {
                let trend_avg = trend_slice.iter().sum::<f64>() / trend_slice.len() as f64;
                trend_data.push((trend_avg / y_max).min(1.0));
            } else {
                trend_data.push(normalized);
            }
        }
    }

    // Build lines for the chart
    let mut lines: Vec<Line> = Vec::new();

    // Only show y-axis labels at tick positions
    let tick_rows: Vec<usize> = y_ticks
        .iter()
        .map(|&v| ((1.0 - v / y_max) * graph_height as f64) as usize)
        .collect();

    for row in 0..graph_height {
        // Y-axis label (only at tick positions)
        let y_pct = 1.0 - (row as f64 / graph_height as f64);
        let y_value = y_pct * y_max;

        let y_label = if tick_rows.contains(&row) {
            if y_value >= 1000.0 {
                format!("{:>5.0}k │", y_value / 1000.0)
            } else if y_value >= 1.0 {
                format!("{:>6.1} │", y_value)
            } else {
                format!("{:>6.2} │", y_value)
            }
        } else {
            "       │".to_string()
        };

        let mut line_spans = vec![Span::styled(y_label, Style::default().fg(pal.muted))];

        for (bar_idx, (normalized, _)) in bar_data.iter().enumerate() {
            let bar_height = (normalized * graph_height as f64).ceil() as usize;
            let row_from_bottom = graph_height - 1 - row;

            // Check if trend line should be drawn at this position
            let trend_height = if bar_idx < trend_data.len() {
                (trend_data[bar_idx] * graph_height as f64).round() as usize
            } else {
                0
            };
            let is_trend_row = row_from_bottom == trend_height
                || (row_from_bottom == trend_height.saturating_sub(1) && trend_height > 0);

            // Bar character
            let bar_ch = if row_from_bottom < bar_height {
                '▓'
            } else {
                ' '
            };

            // Color based on relative height
            let bar_color = if *normalized > 0.7 {
                pal.graph_high
            } else if *normalized > 0.3 {
                pal.graph_mid
            } else {
                pal.graph_low
            };

            // Draw bar or trend line
            if is_trend_row && row_from_bottom >= bar_height {
                // Trend line (when above bar)
                line_spans.push(Span::styled("━", Style::default().fg(pal.warning)));
            } else {
                line_spans.push(Span::styled(
                    bar_ch.to_string(),
                    Style::default().fg(bar_color),
                ));
            }

            // Add spacing between bars
            if bar_idx < bar_data.len() - 1 {
                // Check if trend line continues in spacing
                let next_trend = if bar_idx + 1 < trend_data.len() {
                    (trend_data[bar_idx + 1] * graph_height as f64).round() as usize
                } else {
                    trend_height
                };
                let is_trend_space = row_from_bottom == trend_height
                    || row_from_bottom == next_trend
                    || (trend_height != next_trend
                        && row_from_bottom > trend_height.min(next_trend)
                        && row_from_bottom < trend_height.max(next_trend));

                if is_trend_space {
                    line_spans.push(Span::styled("━", Style::default().fg(pal.warning)));
                } else {
                    line_spans.push(Span::raw(" "));
                }
            }
        }

        lines.push(Line::from(line_spans));
    }

    // X-axis line
    let x_axis_len = num_bars * bar_spacing;
    let x_axis = format!("       └{}", "─".repeat(x_axis_len));
    lines.push(Line::from(Span::styled(
        x_axis,
        Style::default().fg(pal.muted),
    )));

    // Era range label with dates
    if let (Some(first), Some(last)) = (app.staking_history.first(), app.staking_history.last()) {
        let era_label = format!(
            "        Era {} ({})  ───  Era {} ({})",
            first.era, first.date, last.era, last.date
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
                .title(format!(
                    " Era History ({} eras) ",
                    app.staking_history.len()
                )),
        )
        .row_highlight_style(
            Style::default()
                .fg(pal.selection)
                .add_modifier(Modifier::REVERSED),
        );

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
    let x_axis = format!("    └{}", "─".repeat(graph_width));
    lines.push(Line::from(Span::styled(
        x_axis,
        Style::default().fg(pal.muted),
    )));

    // Era labels
    if let (Some(first), Some(last)) = (app.staking_history.first(), app.staking_history.last()) {
        let era_label = format!(
            "     Era {:<10} {:>width$}",
            first.era,
            last.era,
            width = graph_width - 10
        );
        lines.push(Line::from(Span::styled(
            era_label,
            Style::default().fg(pal.muted),
        )));
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
                Span::styled(format!("{}", last.era), Style::default().fg(pal.fg)),
                Span::styled(
                    format!(
                        " | Reward: {} {}",
                        format_balance(last.reward, decimals),
                        symbol
                    ),
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

/// Render the QR code modal overlay with tabs.
fn render_qr_modal(frame: &mut Frame, app: &App) {
    let pal = &app.palette;
    let area = frame.area();

    // Use up to 90% of screen for modal
    // QR version 10 = 57 + 8 quiet = 65 chars wide, 33 rows (half-blocks)
    let max_modal_width = 90.min(area.width * 9 / 10);
    let max_modal_height = 45.min(area.height * 9 / 10);
    let modal_width = max_modal_width.max(55); // Minimum 55 chars
    let modal_height = max_modal_height.max(30); // Minimum 30 lines
    let modal_x = (area.width.saturating_sub(modal_width)) / 2;
    let modal_y = (area.height.saturating_sub(modal_height)) / 2;
    let modal_area = Rect::new(modal_x, modal_y, modal_width, modal_height);

    frame.render_widget(Clear, modal_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(pal.primary))
        .title(" Nomination QR ");

    frame.render_widget(block.clone(), modal_area);

    // Inner layout for tabs and content
    let inner_area = block.inner(modal_area);
    let chunks = Layout::vertical([
        Constraint::Length(1), // Tabs
        Constraint::Min(0),    // Content
        Constraint::Length(1), // Footer hint
    ])
    .split(inner_area);

    // Tabs - show Scan tab if we have pending unsigned tx, Submit tab if we have signed tx
    let titles = if app.pending_tx.is_some() {
        vec![
            Line::from(" QR Code "),
            Line::from(" Extrinsic "),
            Line::from(" Scan "),
            Line::from(" Submit "),
        ]
    } else if app.pending_unsigned_tx.is_some() {
        vec![
            Line::from(" QR Code "),
            Line::from(" Extrinsic "),
            Line::from(" Scan "),
        ]
    } else {
        vec![Line::from(" QR Code "), Line::from(" Extrinsic ")]
    };
    let tabs = Tabs::new(titles)
        .select(app.qr_modal_tab)
        .style(Style::default().fg(pal.muted))
        .highlight_style(Style::default().fg(pal.highlight).bold());
    frame.render_widget(tabs, chunks[0]);

    match app.qr_modal_tab {
        0 => render_qr_content(frame, app, chunks[1]),
        1 => render_qr_details(frame, app, chunks[1]),
        2 => render_scan_camera(frame, app, chunks[1]),
        3 => render_submit_tab(frame, app, chunks[1]),
        _ => render_qr_content(frame, app, chunks[1]),
    }

    // Footer
    let footer = if app.pending_tx.is_some() {
        "Tab:View  Enter:Submit  Esc:Close"
    } else if app.pending_unsigned_tx.is_some() {
        "Tab:View  s:Scan  Esc:Close"
    } else {
        "Tab:View  Esc:Close"
    };
    frame.render_widget(
        Paragraph::new(footer).alignment(Alignment::Center),
        chunks[2],
    );
}

fn render_qr_content(frame: &mut Frame, app: &App, area: Rect) {
    let pal = &app.palette;
    let mut lines = Vec::new();
    let mut qr_width: u16 = 0;

    let max_qr_height = area.height.saturating_sub(8) as usize; // Leave room for text
    let max_qr_width = area.width.saturating_sub(4) as usize;

    // Minimum size check for smallest usable QR (version 4)
    // Version 4 = 33 modules + 8 quiet zone = 41 chars wide
    // Height = 41/2 = ~21 rows (half-blocks)
    let min_width_needed = 50; // 41 + margin
    let min_height_needed = 25; // ~21 rows for QR + text

    if max_qr_width < min_width_needed || max_qr_height < min_height_needed {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "⚠ Terminal too small for QR code",
            Style::default().fg(pal.warning).bold(),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(format!(
            "Current: {}x{} chars",
            area.width, area.height
        )));
        lines.push(Line::from(format!(
            "Minimum: {}x{} chars",
            min_width_needed + 4,
            min_height_needed + 8
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Please resize your terminal window",
            Style::default().fg(pal.muted),
        )));
    } else {
        match &app.qr_data {
            Some(data) => {
                // Always use multipart format (UOS headers) to ensure Vault recognizes it as binary
                // and not text (which causes "invalid utf-8" errors starting with 'S' 0x53).
                let dark_theme = app.theme == crate::theme::Theme::Dark;
                render_multipart_qr(
                    &mut lines,
                    &mut qr_width,
                    data,
                    max_qr_height,
                    max_qr_width,
                    app.qr_frame,
                    pal,
                    dark_theme,
                );
            }
            None => lines.push(Line::from("No Data")),
        }
    }

    let p = Paragraph::new(lines).alignment(Alignment::Center);
    frame.render_widget(p, area);
}

fn render_qr_details(frame: &mut Frame, app: &App, area: Rect) {
    let pal = &app.palette;
    let mut lines = Vec::new();

    // Header info
    lines.push(Line::from(vec![Span::styled(
        "Transaction Details",
        Style::default().fg(pal.primary).bold(),
    )]));
    lines.push(Line::from(""));

    if let Some(tx_info) = &app.qr_tx_info {
        // Call info
        lines.push(Line::from(vec![
            Span::styled("Call: ", Style::default().fg(pal.fg_dim)),
            Span::styled(&tx_info.call, Style::default().fg(pal.success).bold()),
        ]));
        lines.push(Line::from(""));

        // Signer
        lines.push(Line::from(vec![Span::styled(
            "Signer: ",
            Style::default().fg(pal.fg_dim),
        )]));
        // Truncate signer for display
        let signer_display = if tx_info.signer.len() > 48 {
            format!(
                "{}...{}",
                &tx_info.signer[..24],
                &tx_info.signer[tx_info.signer.len() - 20..]
            )
        } else {
            tx_info.signer.clone()
        };
        lines.push(Line::from(format!("  {}", signer_display)));
        lines.push(Line::from(""));

        // Targets (validators being nominated)
        lines.push(Line::from(vec![Span::styled(
            format!("Targets ({} validators):", tx_info.targets.len()),
            Style::default().fg(pal.fg_dim),
        )]));

        // Show validators (truncate addresses)
        let max_display = 8; // Show up to 8 validators
        for (i, target) in tx_info.targets.iter().take(max_display).enumerate() {
            let addr_display = if target.len() > 20 {
                format!("{}...{}", &target[..10], &target[target.len() - 10..])
            } else {
                target.clone()
            };
            lines.push(Line::from(format!("  {}. {}", i + 1, addr_display)));
        }
        if tx_info.targets.len() > max_display {
            lines.push(Line::from(format!(
                "  ... and {} more",
                tx_info.targets.len() - max_display
            )));
        }
        lines.push(Line::from(""));

        // Technical details
        lines.push(Line::from(vec![Span::styled(
            "Technical:",
            Style::default().fg(pal.fg_dim),
        )]));
        lines.push(Line::from(format!("  Nonce: {}", tx_info.nonce)));
        lines.push(Line::from(format!(
            "  Spec Version: {}",
            tx_info.spec_version
        )));
        lines.push(Line::from(format!("  Tx Version: {}", tx_info.tx_version)));
        lines.push(Line::from(format!(
            "  Call Data: {} bytes",
            tx_info.call_data_size
        )));
        lines.push(Line::from(format!(
            "  Metadata Hash: {}",
            if tx_info.include_metadata_hash {
                "Enabled"
            } else {
                "Disabled"
            }
        )));

        if tx_info.include_metadata_hash {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "  Note: If scanning fails, update your Vault metadata.",
                Style::default().fg(pal.warning),
            )));
        }

        if let Some(data) = &app.qr_data {
            lines.push(Line::from(format!("  QR Payload: {} bytes", data.len())));
        }
    } else if let Some(data) = &app.qr_data {
        // Fallback if no tx_info
        lines.push(Line::from(format!("Payload Size: {} bytes", data.len())));
        if let Some(account) = &app.watched_account {
            lines.push(Line::from(format!("Signer: {}", account)));
        }
        lines.push(Line::from("Action: Nominate validators"));
        lines.push(Line::from(format!(
            "Validators: {}",
            app.selected_validators.len()
        )));
    } else {
        lines.push(Line::from("No transaction data available"));
    }

    let p = Paragraph::new(lines).block(Block::default().borders(Borders::NONE));
    frame.render_widget(p, area);
}

/// Convert grayscale pixels to braille characters.
///
/// Each braille character represents a 2x4 pixel block.
/// Braille dot positions:
/// ```
/// 1  4
/// 2  5
/// 3  6
/// 7  8
/// ```
fn grayscale_to_braille(
    pixels: &[u8],
    width: usize,
    height: usize,
    threshold: u8,
) -> Vec<String> {
    // Braille patterns: U+2800 to U+28FF
    // Bit mapping: dot 1 = bit 0, dot 2 = bit 1, dot 3 = bit 2, dot 4 = bit 3,
    //              dot 5 = bit 4, dot 6 = bit 5, dot 7 = bit 6, dot 8 = bit 7

    let mut lines = Vec::new();

    // Process 4 rows at a time (each braille char is 4 rows tall)
    for row in (0..height).step_by(4) {
        let mut line = String::new();

        // Process 2 columns at a time (each braille char is 2 cols wide)
        for col in (0..width).step_by(2) {
            let mut braille: u32 = 0x2800; // Base braille character

            // Sample 8 pixels and map to braille dots
            // Left column (dots 1, 2, 3, 7)
            if row < height && col < width {
                let idx = row * width + col;
                if pixels.get(idx).copied().unwrap_or(255) < threshold {
                    braille |= 0x01; // dot 1
                }
            }
            if row + 1 < height && col < width {
                let idx = (row + 1) * width + col;
                if pixels.get(idx).copied().unwrap_or(255) < threshold {
                    braille |= 0x02; // dot 2
                }
            }
            if row + 2 < height && col < width {
                let idx = (row + 2) * width + col;
                if pixels.get(idx).copied().unwrap_or(255) < threshold {
                    braille |= 0x04; // dot 3
                }
            }
            if row + 3 < height && col < width {
                let idx = (row + 3) * width + col;
                if pixels.get(idx).copied().unwrap_or(255) < threshold {
                    braille |= 0x40; // dot 7
                }
            }

            // Right column (dots 4, 5, 6, 8)
            if row < height && col + 1 < width {
                let idx = row * width + col + 1;
                if pixels.get(idx).copied().unwrap_or(255) < threshold {
                    braille |= 0x08; // dot 4
                }
            }
            if row + 1 < height && col + 1 < width {
                let idx = (row + 1) * width + col + 1;
                if pixels.get(idx).copied().unwrap_or(255) < threshold {
                    braille |= 0x10; // dot 5
                }
            }
            if row + 2 < height && col + 1 < width {
                let idx = (row + 2) * width + col + 1;
                if pixels.get(idx).copied().unwrap_or(255) < threshold {
                    braille |= 0x20; // dot 6
                }
            }
            if row + 3 < height && col + 1 < width {
                let idx = (row + 3) * width + col + 1;
                if pixels.get(idx).copied().unwrap_or(255) < threshold {
                    braille |= 0x80; // dot 8
                }
            }

            line.push(char::from_u32(braille).unwrap_or(' '));
        }

        lines.push(line);
    }

    lines
}

/// Render the camera scan tab with visual feedback.
fn render_scan_camera(frame: &mut Frame, app: &App, area: Rect) {
    use crate::app::CameraScanStatus;

    let pal = &app.palette;
    let mut lines = Vec::new();

    // Header
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Scan Signed Transaction from Vault",
        Style::default().fg(pal.primary).bold(),
    )));
    lines.push(Line::from(""));

    // Determine status and colors based on camera_scan_status
    let frames = app.camera_frames_captured;
    let (status_text, status_style) = match app.camera_scan_status {
        None | Some(CameraScanStatus::Initializing) => (
            "Initializing camera...".to_string(),
            Style::default().fg(pal.muted),
        ),
        Some(CameraScanStatus::Scanning) => (
            format!("Scanning... [{} frames]", frames),
            Style::default().fg(pal.warning),
        ),
        Some(CameraScanStatus::Detected) => (
            format!("QR DETECTED! [{} frames] Hold steady...", frames),
            Style::default().fg(pal.success).bold(),
        ),
        Some(CameraScanStatus::Success) => (
            "Successfully scanned!".to_string(),
            Style::default().fg(pal.success),
        ),
        Some(CameraScanStatus::Error) => (
            "Camera error - check permissions".to_string(),
            Style::default().fg(pal.error),
        ),
    };

    let border_color = match app.camera_scan_status {
        None | Some(CameraScanStatus::Initializing) | Some(CameraScanStatus::Scanning) => pal.border,
        Some(CameraScanStatus::Detected) => pal.success,
        Some(CameraScanStatus::Success) => pal.success,
        Some(CameraScanStatus::Error) => pal.error,
    };

    // Render camera preview or placeholder
    if let Some(ref pixels) = app.camera_preview {
        let (width, height) = app.camera_preview_size;
        if width > 0 && height > 0 {
            // Convert grayscale to braille
            let braille_lines = grayscale_to_braille(pixels, width, height, 128);
            let preview_width = width / 2; // braille chars are 2 pixels wide

            // Draw top border
            let top_border = format!("┌{}┐", "─".repeat(preview_width));
            lines.push(Line::from(Span::styled(
                top_border,
                Style::default().fg(border_color),
            )));

            // Draw braille preview with side borders
            for braille_line in &braille_lines {
                lines.push(Line::from(vec![
                    Span::styled("│", Style::default().fg(border_color)),
                    Span::styled(braille_line.clone(), Style::default().fg(pal.fg)),
                    Span::styled("│", Style::default().fg(border_color)),
                ]));
            }

            // Draw bottom border
            let bottom_border = format!("└{}┘", "─".repeat(preview_width));
            lines.push(Line::from(Span::styled(
                bottom_border,
                Style::default().fg(border_color),
            )));

            // Show QR bounds indicator if detected
            if app.qr_bounds.is_some() {
                lines.push(Line::from(Span::styled(
                    "▶ QR code in view ◀",
                    Style::default().fg(pal.success).bold(),
                )));
            }
        }
    } else {
        // Fallback: simple placeholder when no preview
        let frame_width = 40;
        let frame_height = 12;
        let top_border = format!("┌{}┐", "─".repeat(frame_width));
        let bottom_border = format!("└{}┘", "─".repeat(frame_width));
        let empty_line = format!("│{}│", " ".repeat(frame_width));

        lines.push(Line::from(Span::styled(
            top_border,
            Style::default().fg(border_color),
        )));

        for i in 0..frame_height {
            if i == frame_height / 2 {
                let indicator = match app.camera_scan_status {
                    Some(CameraScanStatus::Initializing) | None => app.spinner_char(),
                    Some(CameraScanStatus::Scanning) => '◯',
                    Some(CameraScanStatus::Detected) => '◉',
                    Some(CameraScanStatus::Success) => '✓',
                    Some(CameraScanStatus::Error) => '✗',
                };
                let center = format!("{:^width$}", indicator, width = frame_width);
                lines.push(Line::from(vec![
                    Span::styled("│", Style::default().fg(border_color)),
                    Span::styled(center, status_style),
                    Span::styled("│", Style::default().fg(border_color)),
                ]));
            } else {
                lines.push(Line::from(Span::styled(
                    empty_line.clone(),
                    Style::default().fg(border_color),
                )));
            }
        }

        lines.push(Line::from(Span::styled(
            bottom_border,
            Style::default().fg(border_color),
        )));
    }

    lines.push(Line::from(""));

    // Status text
    lines.push(Line::from(Span::styled(status_text, status_style)));
    lines.push(Line::from(""));

    // Help text
    if matches!(
        app.camera_scan_status,
        Some(CameraScanStatus::Scanning) | Some(CameraScanStatus::Detected)
    ) {
        lines.push(Line::from(Span::styled(
            "Hold Vault QR code in front of camera",
            Style::default().fg(pal.muted),
        )));
    } else if matches!(app.camera_scan_status, Some(CameraScanStatus::Error)) {
        lines.push(Line::from(Span::styled(
            "Grant camera access in System Preferences > Privacy",
            Style::default().fg(pal.muted),
        )));
    }

    let p = Paragraph::new(lines)
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::NONE));
    frame.render_widget(p, area);
}

/// Render the Submit tab for broadcasting signed transaction.
fn render_submit_tab(frame: &mut Frame, app: &App, area: Rect) {
    use crate::action::TxSubmissionStatus;

    let pal = &app.palette;
    let mut lines = Vec::new();

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Submit Signed Transaction",
        Style::default().fg(pal.primary).bold(),
    )));
    lines.push(Line::from(""));

    if let Some(ref tx) = app.pending_tx {
        // Show transaction hash
        lines.push(Line::from(vec![
            Span::styled("Tx Hash: ", Style::default().fg(pal.muted)),
            Span::styled(
                format!("0x{}", hex::encode(&tx.tx_hash[..8])),
                Style::default().fg(pal.accent),
            ),
            Span::styled("...", Style::default().fg(pal.muted)),
        ]));

        // Show extrinsic size
        lines.push(Line::from(vec![
            Span::styled("Size: ", Style::default().fg(pal.muted)),
            Span::styled(
                format!("{} bytes", tx.signed_extrinsic.len()),
                Style::default().fg(pal.fg),
            ),
        ]));

        lines.push(Line::from(""));

        // Show status with appropriate styling
        let (status_text, status_style) = match &tx.status {
            TxSubmissionStatus::WaitingForSignature => (
                "⏳ Waiting for signature...".to_string(),
                Style::default().fg(pal.warning),
            ),
            TxSubmissionStatus::ReadyToSubmit => (
                "✓ Ready to submit".to_string(),
                Style::default().fg(pal.success).bold(),
            ),
            TxSubmissionStatus::Submitting => (
                format!("⏳ Submitting{}", ".".repeat((app.tick_count() % 4) as usize)),
                Style::default().fg(pal.warning),
            ),
            TxSubmissionStatus::InBlock { block_hash } => (
                format!("📦 In block 0x{}...", hex::encode(&block_hash[..4])),
                Style::default().fg(pal.success),
            ),
            TxSubmissionStatus::Finalized { block_hash } => (
                format!("✓ Finalized in 0x{}...", hex::encode(&block_hash[..4])),
                Style::default().fg(pal.success).bold(),
            ),
            TxSubmissionStatus::Failed(err) => (
                format!("✗ Failed: {}", truncate_str(err, 40)),
                Style::default().fg(pal.error),
            ),
        };

        lines.push(Line::from(Span::styled(status_text, status_style)));
        lines.push(Line::from(""));

        // Show action hint based on status
        match &tx.status {
            TxSubmissionStatus::ReadyToSubmit => {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "Press Enter or 's' to broadcast to network",
                    Style::default().fg(pal.highlight).bold(),
                )));
            }
            TxSubmissionStatus::Finalized { .. } => {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "Transaction confirmed! Press Esc to close.",
                    Style::default().fg(pal.success),
                )));
            }
            TxSubmissionStatus::Failed(_) => {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "Press Esc to close and try again.",
                    Style::default().fg(pal.muted),
                )));
            }
            _ => {}
        }
    } else {
        lines.push(Line::from(Span::styled(
            "No signed transaction pending",
            Style::default().fg(pal.muted),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Scan a signature QR from Vault first",
            Style::default().fg(pal.muted),
        )));
    }

    let p = Paragraph::new(lines)
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::NONE));
    frame.render_widget(p, area);
}

/// Calculate chunk size that produces scannable QR codes.
/// Balances chunk size vs number of frames for optimal scanning from terminal displays.
fn calculate_chunk_size(max_qr_height: usize, max_qr_width: usize) -> usize {
    // QR codes displayed on terminals and scanned by phone cameras need to be
    // not too dense. Target QR version 6-10 for good scannability.
    //
    // QR version capacity (binary mode, L error correction):
    // Version 6 (41 modules): ~136 bytes
    // Version 8 (49 modules): ~192 bytes
    // Version 10 (57 modules): ~271 bytes
    //
    // Half-block rendering: modules/2 = terminal lines needed
    // Single-width: modules + 8 (quiet zone) = chars needed

    let max_modules_from_height = max_qr_height * 2;
    let max_modules_from_width = max_qr_width.saturating_sub(8);
    let target_modules = max_modules_from_height.min(max_modules_from_width);

    // Choose chunk size for scannable QR codes (not too dense)
    if target_modules >= 65 {
        250 // Version ~10, good balance
    } else if target_modules >= 55 {
        180 // Version ~8
    } else if target_modules >= 45 {
        120 // Version ~6
    } else {
        80 // Small terminal, version ~4
    }
}

/// Render animated multipart QR codes for data that doesn't fit in a single QR.
///
/// Uses UOS multipart format for Polkadot Vault:
/// - Each frame: `[0x00][total_frames:2 BE][frame_index:2 BE][frame_data]`
/// - Data is raw binary (not hex-encoded)
fn render_multipart_qr(
    lines: &mut Vec<Line<'static>>,
    qr_width: &mut u16,
    data: &[u8],
    max_qr_height: usize,
    max_qr_width: usize,
    current_frame: usize,
    pal: &Palette,
    dark_theme: bool,
) {
    // Input is raw binary UOS data
    let raw_bytes = data;

    // Calculate appropriate chunk size
    // QR code byte capacity depends on version and error correction
    // Version 10 with L correction can hold ~271 bytes
    // Each frame adds 5 bytes header: [0x00][total:2][index:2]
    let target_qr_bytes = calculate_chunk_size(max_qr_height, max_qr_width);
    let raw_chunk_size = target_qr_bytes.saturating_sub(5);
    let raw_chunk_size = raw_chunk_size.max(50); // Minimum 50 bytes per chunk

    // Calculate number of parts needed
    let total_parts = raw_bytes.len().div_ceil(raw_chunk_size);
    let total_parts = total_parts.clamp(1, 65535) as u16; // Max 2 bytes

    // Get current frame (cycle through parts)
    let frame_idx = (current_frame % (total_parts as usize)) as u16;

    // Extract chunk for this frame
    let start = (frame_idx as usize) * raw_chunk_size;
    let end = (start + raw_chunk_size).min(raw_bytes.len());
    let chunk = &raw_bytes[start..end];

    // Build frame with UOS multipart header:
    // [0x00][total_frames:2 BE][frame_index:2 BE][frame_data]
    let mut frame_bytes = Vec::with_capacity(5 + chunk.len());
    frame_bytes.push(0x00); // Multipart marker
    frame_bytes.extend_from_slice(&total_parts.to_be_bytes());
    frame_bytes.extend_from_slice(&frame_idx.to_be_bytes());
    frame_bytes.extend_from_slice(chunk);

    // Determine QR version based on target chunk size to ensure consistent dimensions
    // across all frames, even if the last frame has less data.
    // Version capacity (EcLevel::L): v6=106, v7=122, v8=152, v9=180, v10=213, v11=251, v12=287
    let full_frame_size = 5 + raw_chunk_size;
    let qr_version = if full_frame_size <= 106 {
        6
    } else if full_frame_size <= 122 {
        7
    } else if full_frame_size <= 152 {
        8
    } else if full_frame_size <= 180 {
        9
    } else if full_frame_size <= 213 {
        10
    } else if full_frame_size <= 251 {
        11
    } else {
        12
    };

    // Copy colors to avoid borrow issues with closures
    let primary = pal.primary;
    let warning = pal.warning;
    let success = pal.success;
    let error = pal.error;

    // Use fixed QR version for consistent frame dimensions
    // EcLevel::L provides maximum data capacity
    match QrCode::with_version(&frame_bytes, Version::Normal(qr_version), EcLevel::L) {
        Ok(qr) => {
            let qr_lines = render_qr_halfblock(&qr, dark_theme);
            // Width in chars (each module is 2 chars wide now)
            *qr_width = qr_lines
                .first()
                .map(|l| l.width() as u16)
                .unwrap_or(0);

            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::raw("Ensure Vault network is "),
                Span::styled("Polkadot Asset Hub", Style::default().fg(pal.accent).bold()),
            ]));
            lines.push(Line::from(Span::styled(
                "Scan with Polkadot Vault",
                Style::default().fg(primary).bold(),
            )));
            lines.push(Line::from(Span::styled(
                format!("(Animated: part {}/{})", frame_idx + 1, total_parts),
                Style::default().fg(warning),
            )));
            lines.push(Line::from(""));

            for line in qr_lines {
                lines.push(line);
            }

            lines.push(Line::from(""));
            lines.push(Line::from(format!(
                "Total: {} bytes ({} parts)",
                raw_bytes.len(),
                total_parts
            )));

            // Progress bar for animation
            let progress_width = 20usize;
            let filled = ((frame_idx as usize + 1) * progress_width) / (total_parts as usize);
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

/// Render QR code using half-block characters for compact, square display.
/// Terminal chars are ~2:1 aspect ratio (taller than wide), so half-blocks
/// (2 modules per char height) + single-width (1 char per module) ≈ square.
///
/// `dark_theme`: if true, inverts colors for dark terminal backgrounds
fn render_qr_halfblock(qr: &QrCode, dark_theme: bool) -> Vec<Line<'static>> {
    use ratatui::style::Color;

    let qr_colors = qr.to_colors();
    let width = qr.width();

    // Add quiet zone (4 modules on each side per QR spec)
    let quiet = 4;
    let total_width = width + 2 * quiet;

    // For dark themes: QR dark modules = white, light modules = black (inverted)
    // For light themes: QR dark modules = black, light modules = white (standard)
    let (dark_fg, light_bg) = if dark_theme {
        (Color::White, Color::Black)
    } else {
        (Color::Black, Color::White)
    };

    let mut result = Vec::new();

    // Process 2 rows at a time (half-block compression)
    let total_height = width + 2 * quiet;
    let mut y = 0;

    while y < total_height {
        let mut spans = Vec::new();

        for x in 0..total_width {
            let top_dark = if y < quiet || y >= quiet + width || x < quiet || x >= quiet + width {
                false // quiet zone is light
            } else {
                let idx = (y - quiet) * width + (x - quiet);
                qr_colors
                    .get(idx)
                    .map(|c| *c == qrcode::Color::Dark)
                    .unwrap_or(false)
            };

            let bottom_dark =
                if y + 1 < quiet || y + 1 >= quiet + width || x < quiet || x >= quiet + width {
                    false
                } else if y + 1 < total_height {
                    let idx = (y + 1 - quiet) * width + (x - quiet);
                    qr_colors
                        .get(idx)
                        .map(|c| *c == qrcode::Color::Dark)
                        .unwrap_or(false)
                } else {
                    false
                };

            // Half-block chars: ▀ = top half, ▄ = bottom half, █ = full, ' ' = empty
            // Use explicit fg/bg colors for cross-terminal compatibility
            let (ch, fg, bg) = match (top_dark, bottom_dark) {
                (true, true) => ("█", dark_fg, dark_fg),
                (true, false) => ("▀", dark_fg, light_bg),
                (false, true) => ("▄", dark_fg, light_bg),
                (false, false) => (" ", light_bg, light_bg),
            };
            spans.push(Span::styled(ch.to_string(), Style::default().fg(fg).bg(bg)));
        }

        result.push(Line::from(spans));
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
        ConnectionStatus::Connecting => {
            format!("Connecting{}", ".".repeat((app.tick_count() % 4) as usize))
        }
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

    // Get visible lines (10 lines, accounting for scroll)
    let visible_lines = 10;
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

    // Pad with empty lines if we have fewer log lines than visible area
    let mut display_lines = lines;
    while display_lines.len() < visible_lines {
        display_lines.insert(0, Line::from(""));
    }

    // Format bandwidth if available
    let bandwidth_text = if let Some(bw) = app.estimated_bandwidth {
        if bw >= 1_000_000.0 {
            format!(" │ ↓ {:.1} MB/s", bw / 1_000_000.0)
        } else {
            format!(" │ ↓ {:.0} KB/s", bw / 1024.0)
        }
    } else {
        String::new()
    };

    let title = Line::from(vec![
        Span::raw(" Logs "),
        Span::styled(format!("({}) ", log_count), Style::default().fg(p.muted)),
        Span::raw("│ "),
        Span::styled(status_text, status_style),
        Span::styled(bandwidth_text, Style::default().fg(p.primary)),
        Span::styled(scroll_info, Style::default().fg(p.muted)),
        Span::raw(" │ PgUp/PgDn  ?:Help  q:Quit "),
    ]);

    let paragraph = Paragraph::new(display_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(p.border))
            .title(title),
    );

    frame.render_widget(paragraph, area);
}

/// Render the sort menu overlay.
fn render_sort_menu(frame: &mut Frame, app: &App) {
    let pal = &app.palette;
    let area = frame.area();

    // Calculate centered modal area
    let modal_width = 35.min(area.width.saturating_sub(4));
    let modal_height = 16.min(area.height.saturating_sub(4));
    let modal_x = (area.width.saturating_sub(modal_width)) / 2;
    let modal_y = (area.height.saturating_sub(modal_height)) / 2;
    let modal_area = Rect::new(modal_x, modal_y, modal_width, modal_height);

    // Clear the background
    frame.render_widget(Clear, modal_area);

    let key_style = Style::default().fg(pal.highlight).bold();

    let lines = match app.current_view {
        View::Validators => {
            let mut lines = vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  Sort by:",
                    Style::default().fg(pal.primary).bold(),
                )),
                Line::from(""),
            ];
            for field in ValidatorSortField::all() {
                let marker = if *field == app.validator_sort {
                    "▶ "
                } else {
                    "  "
                };
                lines.push(Line::from(vec![
                    Span::raw(marker),
                    Span::styled(format!("{}", field.key()), key_style),
                    Span::raw(format!(" - {}", field.label())),
                ]));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::raw("  Press key or "),
                Span::styled("Esc", key_style),
                Span::raw(" to close"),
            ]));
            lines
        }
        View::Pools => {
            let mut lines = vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  Sort by:",
                    Style::default().fg(pal.primary).bold(),
                )),
                Line::from(""),
            ];
            for field in PoolSortField::all() {
                let marker = if *field == app.pool_sort {
                    "▶ "
                } else {
                    "  "
                };
                lines.push(Line::from(vec![
                    Span::raw(marker),
                    Span::styled(format!("{}", field.key()), key_style),
                    Span::raw(format!(" - {}", field.label())),
                ]));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::raw("  Press key or "),
                Span::styled("Esc", key_style),
                Span::raw(" to close"),
            ]));
            lines
        }
        _ => vec![Line::from("Invalid view for sort menu")],
    };

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(pal.primary))
                .title(" Sort Menu "),
        )
        .style(Style::default().bg(pal.bg));

    frame.render_widget(paragraph, modal_area);
}

/// Render the strategy menu overlay.
fn render_strategy_menu(frame: &mut Frame, app: &App) {
    let pal = &app.palette;
    let area = frame.area();

    // Calculate centered modal area
    let modal_width = 50.min(area.width.saturating_sub(4));
    let modal_height = 14.min(area.height.saturating_sub(4));
    let modal_x = (area.width.saturating_sub(modal_width)) / 2;
    let modal_y = (area.height.saturating_sub(modal_height)) / 2;
    let modal_area = Rect::new(modal_x, modal_y, modal_width, modal_height);

    // Clear the background
    frame.render_widget(Clear, modal_area);

    let key_style = Style::default().fg(pal.highlight).bold();
    let selected_style = Style::default().fg(pal.success).bold();

    let strategies = [
        ("Top APY", "Select validators with highest APY"),
        ("Random from Top", "Random selection from top performers"),
        ("Diversify by Stake", "Spread across different stake sizes"),
    ];

    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  Optimization Strategy:",
            Style::default().fg(pal.primary).bold(),
        )),
        Line::from(""),
    ];

    for (i, (name, desc)) in strategies.iter().enumerate() {
        let is_selected = i == app.strategy_index;
        let marker = if is_selected { "▶ " } else { "  " };
        let style = if is_selected {
            selected_style
        } else {
            Style::default()
        };

        lines.push(Line::from(vec![
            Span::raw(marker),
            Span::styled(format!("{}", i + 1), key_style),
            Span::raw(". "),
            Span::styled(*name, style),
        ]));
        lines.push(Line::from(format!("     {}", desc)));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled("↑/↓", key_style),
        Span::raw(": Navigate  "),
        Span::styled("Enter", key_style),
        Span::raw(": Select  "),
        Span::styled("Esc", key_style),
        Span::raw(": Cancel"),
    ]));

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(pal.primary))
                .title(" Strategy Selection "),
        )
        .style(Style::default().bg(pal.bg));

    frame.render_widget(paragraph, modal_area);
}
