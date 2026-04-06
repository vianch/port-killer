use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState},
    Frame,
};

use crate::app::{App, AppMode};
use crate::port_info::Severity;

pub fn render(frame: &mut Frame, app: &App) {
    let input_height = if app.mode == AppMode::Input { 3 } else { 0 };

    let chunks = Layout::vertical([
        Constraint::Length(1),       // Title bar
        Constraint::Min(5),          // Table
        Constraint::Length(input_height), // Input (conditional)
        Constraint::Length(1),       // Help bar
    ])
    .split(frame.area());

    render_title_bar(frame, chunks[0], app);
    render_table(frame, chunks[1], app);

    if app.mode == AppMode::Input {
        render_input(frame, chunks[2], app);
    }

    render_help_bar(frame, chunks[3], app);

    if app.mode == AppMode::Confirm {
        render_confirm_modal(frame, app);
    }
}

fn render_title_bar(frame: &mut Frame, area: Rect, app: &App) {
    let count = app.filtered_indices.len();
    let total = app.entries.len();

    let mut spans = vec![
        Span::styled(
            " Port Killer ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    ];

    if let Some(ref msg) = app.status_message {
        spans.push(Span::styled(
            format!("  {msg}"),
            Style::default().fg(Color::Yellow),
        ));
    }

    let filter_info = if !app.input_buffer.is_empty() {
        format!("  {count}/{total} ports (filtered)")
    } else {
        format!("  {total} ports")
    };

    // Push spacer to right-align the count
    let used_width: usize = spans.iter().map(|s| s.content.len()).sum();
    let remaining = (area.width as usize).saturating_sub(used_width + filter_info.len());
    spans.push(Span::raw(" ".repeat(remaining)));
    spans.push(Span::styled(
        filter_info,
        Style::default().fg(Color::DarkGray),
    ));

    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(Color::DarkGray).fg(Color::White)),
        area,
    );
}

fn render_table(frame: &mut Frame, area: Rect, app: &App) {
    let header = Row::new(vec!["Port", "Command", "PID", "Severity", "Description"])
        .style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .bottom_margin(1);

    let rows: Vec<Row> = app
        .filtered_indices
        .iter()
        .map(|&idx| {
            let entry = &app.entries[idx];
            let severity_style = match entry.severity {
                Severity::Low => Style::default().fg(Color::Green),
                Severity::Medium => Style::default().fg(Color::Yellow),
                Severity::High => Style::default().fg(Color::Red),
                Severity::Critical => Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            };
            Row::new(vec![
                Cell::from(entry.port.to_string()),
                Cell::from(entry.command.clone()),
                Cell::from(entry.pid.to_string()),
                Cell::from(entry.severity.to_string()).style(severity_style),
                Cell::from(entry.description.clone()),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(7),
        Constraint::Min(15),
        Constraint::Length(8),
        Constraint::Length(10),
        Constraint::Min(20),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .row_highlight_style(
            Style::default()
                .bg(Color::Rgb(40, 40, 60))
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("► ")
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(" Listening Ports ")
                .title_style(Style::default().fg(Color::Cyan)),
        );

    let mut table_state = TableState::default().with_selected(Some(app.selected_index));
    frame.render_stateful_widget(table, area, &mut table_state);
}

fn render_input(frame: &mut Frame, area: Rect, app: &App) {
    let input = Paragraph::new(Line::from(vec![
        Span::styled("Filter: ", Style::default().fg(Color::Yellow)),
        Span::raw(&app.input_buffer),
        Span::styled("█", Style::default().fg(Color::Cyan)),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(" Search ")
            .title_style(Style::default().fg(Color::Yellow)),
    );

    frame.render_widget(input, area);
}

fn render_help_bar(frame: &mut Frame, area: Rect, app: &App) {
    let help_text = match app.mode {
        AppMode::Normal => {
            " q: Quit │ ↑↓/jk: Navigate │ Enter: Kill │ /: Filter │ r: Refresh"
        }
        AppMode::Input => " Esc: Cancel │ Enter: Apply │ Type to filter by port or command",
        AppMode::Confirm => " y/Enter: Confirm Kill │ n/Esc: Cancel",
    };

    frame.render_widget(
        Paragraph::new(help_text).style(Style::default().fg(Color::DarkGray)),
        area,
    );
}

fn render_confirm_modal(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let modal_width = 50u16.min(area.width.saturating_sub(4));
    let modal_height = 7u16;
    let modal_area = Rect {
        x: (area.width.saturating_sub(modal_width)) / 2,
        y: (area.height.saturating_sub(modal_height)) / 2,
        width: modal_width,
        height: modal_height,
    };

    frame.render_widget(Clear, modal_area);

    if let Some(entry) = app.selected_entry() {
        let text = vec![
            Line::from(""),
            Line::from(format!(
                "Kill \"{}\" (PID {}) on port {}?",
                entry.command, entry.pid, entry.port
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled(
                    "[y]",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" Confirm    "),
                Span::styled(
                    "[n]",
                    Style::default()
                        .fg(Color::Red)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" Cancel"),
            ]),
        ];

        let modal = Paragraph::new(text)
            .alignment(ratatui::layout::Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Red))
                    .title(" Confirm Kill ")
                    .title_style(
                        Style::default()
                            .fg(Color::Red)
                            .add_modifier(Modifier::BOLD),
                    ),
            );

        frame.render_widget(modal, modal_area);
    }
}
