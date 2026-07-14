use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Sparkline, Table, TableState, Wrap},
};

use crate::app::{App, AppMode, SortColumn, SortMode};
use crate::port_info::{
    Exposure, NO_CWD_PLACEHOLDER, NO_PARENT_PLACEHOLDER, Severity, format_age, format_memory,
};

// --- Layout constants ---
const TITLE_BAR_HEIGHT: u16 = 1;
const FILTER_BAR_HEIGHT: u16 = 3;
const HELP_BAR_HEIGHT: u16 = 1;
const MIN_CONTENT_HEIGHT: u16 = 5;
const DETAIL_PANEL_PERCENT: u16 = 30;
const TABLE_PANEL_PERCENT: u16 = 70;
/// Fixed height of the bottom charts row (2 rows of bars + top/bottom border).
const CHARTS_ROW_HEIGHT: u16 = 7;
/// The charts row splits into two equal halves (CPU | Memory).
const CHART_HALF_PERCENT: u16 = 50;
const CHART_MAX_VALUE: u64 = 100;

// --- Table layout constants (shared by rendering and mouse hit-testing) ---
const TABLE_BORDER_WIDTH: u16 = 1;
const HIGHLIGHT_SYMBOL: &str = "\u{25ba} ";
const HIGHLIGHT_SYMBOL_WIDTH: u16 = 2;
const TABLE_COLUMN_SPACING: u16 = 1;
const DESCRIPTION_MIN_WIDTH: u16 = 20;
/// Header occupies one row plus a one-row bottom margin (see `render_table`).
const HEADER_ROW_HEIGHT: u16 = 1;
const HEADER_BOTTOM_MARGIN: u16 = 1;
const TABLE_COLUMNS: &[(SortColumn, u16)] = &[
    (SortColumn::Port, 7),
    (SortColumn::Command, 16),
    (SortColumn::Pid, 7),
    (SortColumn::Cpu, 7),
    (SortColumn::Mem, 9),
    (SortColumn::Severity, 10),
    (SortColumn::Exposure, 8),
];

// --- Colors (named to avoid repeating raw `Color::X` across widgets) ---
const COLOR_ACCENT: Color = Color::Cyan;
const COLOR_WARN: Color = Color::Yellow;
const COLOR_MUTED: Color = Color::DarkGray;
const COLOR_DANGER: Color = Color::Red;
const COLOR_SUCCESS: Color = Color::Green;
const COLOR_CRITICAL: Color = Color::Magenta;
const COLOR_TEXT: Color = Color::White;
const COLOR_ROW_HIGHLIGHT_BG: Color = Color::Rgb(40, 40, 60);
/// High-contrast style for the app title (the cyan `COLOR_ACCENT` was hard to
/// read on the dark-gray title bar). Kept separate so block titles etc. that
/// intentionally use `COLOR_ACCENT` don't change.
const COLOR_TITLE: Color = Color::White;

// --- Labels ---
const TITLE_TEXT: &str = " Port Killer ";
const HELP_NORMAL: &str = " q: Quit │ ↑↓/jk: Navigate │ Enter: Kill │ K: Force Kill │ /: Filter │ r: Refresh │ s: Sort │ click header: Sort";
const HELP_INPUT: &str = " Esc: Cancel │ Enter: Apply │ Type to filter by port or command";
const HELP_CONFIRM: &str = " y/Enter: Confirm │ n/Esc: Cancel";
const SORT_ASCENDING_MARK: &str = " \u{25b2}";
const SORT_DESCENDING_MARK: &str = " \u{25bc}";
/// Appended to the port count when not running as root, since `lsof`/`ss` then
/// only report the current user's sockets.
const SUDO_HINT: &str = " · run with sudo to see all";

// --- Charts labels ---
const CHART_CPU_LABEL: &str = "CPU";
const CHART_MEM_LABEL: &str = "Mem";

// --- Confirm modal labels ---
const CONFIRM_TITLE_NORMAL: &str = " Confirm Kill ";
const CONFIRM_TITLE_FORCE: &str = " Confirm FORCE KILL ";
const CONFIRM_PROMPT_NORMAL: &str = "Kill";
const CONFIRM_PROMPT_FORCE: &str = "FORCE KILL (SIGKILL)";

// --- Detail panel labels ---
const DETAIL_LABEL_PORT: &str = "Port:    ";
const DETAIL_LABEL_CMD: &str = "Cmd:     ";
const DETAIL_LABEL_PID: &str = "PID:     ";
const DETAIL_LABEL_SEV: &str = "Sev:     ";
const DETAIL_LABEL_DESC: &str = "Desc:    ";
const DETAIL_LABEL_CPU: &str = "CPU:     ";
const DETAIL_LABEL_MEM: &str = "Mem:     ";
const DETAIL_LABEL_BIND: &str = "Bind:    ";
const DETAIL_LABEL_EXPOSURE: &str = "Exposure:";
const DETAIL_LABEL_CMDLINE: &str = "Cmdline: ";
const DETAIL_LABEL_CWD: &str = "Cwd:     ";
const DETAIL_LABEL_AGE: &str = "Age:     ";
const DETAIL_LABEL_PARENT: &str = "Parent:  ";

/// The rects each region occupies for a given terminal size and mode.
/// Pure and shared between rendering and mouse hit-testing so they can never
/// drift apart.
pub struct AppLayout {
    pub title: Rect,
    pub filter: Rect,
    pub detail: Rect,
    pub table: Rect,
    pub cpu_chart: Rect,
    pub mem_chart: Rect,
    pub help: Rect,
}

pub fn compute_layout(area: Rect, mode: AppMode) -> AppLayout {
    let filter_height = if mode == AppMode::Input {
        FILTER_BAR_HEIGHT
    } else {
        0
    };

    let chunks = Layout::vertical([
        Constraint::Length(TITLE_BAR_HEIGHT),
        Constraint::Length(filter_height),
        Constraint::Min(MIN_CONTENT_HEIGHT),
        Constraint::Length(CHARTS_ROW_HEIGHT),
        Constraint::Length(HELP_BAR_HEIGHT),
    ])
    .split(area);

    let content = Layout::horizontal([
        Constraint::Percentage(DETAIL_PANEL_PERCENT),
        Constraint::Percentage(TABLE_PANEL_PERCENT),
    ])
    .split(chunks[2]);

    let charts = Layout::horizontal([
        Constraint::Percentage(CHART_HALF_PERCENT),
        Constraint::Percentage(CHART_HALF_PERCENT),
    ])
    .split(chunks[3]);

    AppLayout {
        title: chunks[0],
        filter: chunks[1],
        detail: content[0],
        table: content[1],
        cpu_chart: charts[0],
        mem_chart: charts[1],
        help: chunks[4],
    }
}

/// Maps a mouse click position to the sort column whose header was clicked,
/// or `None` if the click missed the header row / fell outside the columns.
pub fn column_at(table_area: Rect, mouse_x: u16, mouse_y: u16) -> Option<SortColumn> {
    let header_row_y = table_area.y + TABLE_BORDER_WIDTH;
    if mouse_y != header_row_y {
        return None;
    }

    let mut x = table_area.x + TABLE_BORDER_WIDTH + HIGHLIGHT_SYMBOL_WIDTH;
    for &(column, width) in TABLE_COLUMNS {
        if mouse_x >= x && mouse_x < x + width {
            return Some(column);
        }
        x += width + TABLE_COLUMN_SPACING;
    }
    None
}

/// Maps a mouse click inside the table body to the 0-based VISIBLE row offset
/// (before the scroll offset is applied), or `None` if the click landed on a
/// border, the header, or the header's bottom margin. Geometry mirrors
/// `render_table` exactly so it can't drift.
pub fn row_at(table_area: Rect, mouse_y: u16) -> Option<usize> {
    let first_data_row_y =
        table_area.y + TABLE_BORDER_WIDTH + HEADER_ROW_HEIGHT + HEADER_BOTTOM_MARGIN;
    // Bottom border row is `table_area.bottom() - TABLE_BORDER_WIDTH`.
    let bottom_border_y = table_area.y + table_area.height.saturating_sub(TABLE_BORDER_WIDTH);
    if mouse_y < first_data_row_y || mouse_y >= bottom_border_y {
        return None;
    }
    Some((mouse_y - first_data_row_y) as usize)
}

fn severity_style(severity: Severity) -> Style {
    match severity {
        Severity::Low => Style::default().fg(COLOR_SUCCESS),
        Severity::Medium => Style::default().fg(COLOR_WARN),
        Severity::High => Style::default().fg(COLOR_DANGER),
        Severity::Critical => Style::default()
            .fg(COLOR_CRITICAL)
            .add_modifier(Modifier::BOLD),
    }
}

fn exposure_style(exposure: Exposure) -> Style {
    match exposure {
        Exposure::Loopback => Style::default().fg(COLOR_SUCCESS),
        Exposure::Specific => Style::default().fg(COLOR_TEXT),
        Exposure::AllInterfaces => Style::default().fg(COLOR_WARN).add_modifier(Modifier::BOLD),
    }
}

pub fn render(frame: &mut Frame, app: &mut App) {
    let layout = compute_layout(frame.area(), app.mode);

    render_title_bar(frame, layout.title, app);

    if app.mode == AppMode::Input {
        render_input(frame, layout.filter, app);
    }

    render_detail_panel(frame, layout.detail, app);
    render_table(frame, layout.table, app);
    render_charts(frame, layout.cpu_chart, layout.mem_chart, app);
    render_help_bar(frame, layout.help, app);

    if app.mode == AppMode::Confirm {
        render_confirm_modal(frame, app);
    }
}

fn render_charts(frame: &mut Frame, cpu_area: Rect, mem_area: Rect, app: &App) {
    render_sparkline(
        frame,
        cpu_area,
        CHART_CPU_LABEL,
        &app.cpu_history,
        COLOR_ACCENT,
    );
    render_sparkline(
        frame,
        mem_area,
        CHART_MEM_LABEL,
        &app.mem_history,
        COLOR_WARN,
    );
}

fn render_sparkline(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    history: &std::collections::VecDeque<u64>,
    color: Color,
) {
    let current = history.back().copied().unwrap_or(0);
    let sparkline = Sparkline::default()
        .data(history.iter().copied())
        .max(CHART_MAX_VALUE)
        .style(Style::default().fg(color))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(COLOR_MUTED))
                .title(format!(" {label} {current}% "))
                .title_style(Style::default().fg(COLOR_ACCENT)),
        );
    frame.render_widget(sparkline, area);
}

fn render_title_bar(frame: &mut Frame, area: Rect, app: &App) {
    let count = app.filtered_indices.len();
    let total = app.entries.len();

    let mut spans = vec![Span::styled(
        TITLE_TEXT,
        Style::default()
            .fg(COLOR_TITLE)
            .add_modifier(Modifier::BOLD),
    )];

    if let Some(ref msg) = app.status_message {
        spans.push(Span::styled(
            format!("  {msg}"),
            Style::default().fg(COLOR_WARN),
        ));
    }

    let mut filter_info = if !app.input_buffer.is_empty() {
        format!("  {count}/{total} ports (filtered)")
    } else {
        format!("  {total} ports")
    };
    // Without root, lsof/ss only see this user's sockets — hint at the rest.
    if !app.elevated {
        filter_info.push_str(SUDO_HINT);
    }

    // Push spacer to right-align the count
    let used_width: usize = spans.iter().map(|span| span.content.len()).sum();
    let remaining = (area.width as usize).saturating_sub(used_width + filter_info.len());
    spans.push(Span::raw(" ".repeat(remaining)));
    spans.push(Span::styled(filter_info, Style::default().fg(COLOR_MUTED)));

    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(COLOR_MUTED).fg(COLOR_TEXT)),
        area,
    );
}

fn render_detail_panel(frame: &mut Frame, area: Rect, app: &App) {
    let mut lines = Vec::new();

    if let Some(entry) = app.selected_entry() {
        lines.push(Line::from(vec![
            Span::styled(DETAIL_LABEL_PORT, Style::default().fg(COLOR_MUTED)),
            Span::raw(entry.port.to_string()),
        ]));
        lines.push(Line::from(vec![
            Span::styled(DETAIL_LABEL_CMD, Style::default().fg(COLOR_MUTED)),
            Span::raw(entry.command.clone()),
        ]));
        lines.push(Line::from(vec![
            Span::styled(DETAIL_LABEL_PID, Style::default().fg(COLOR_MUTED)),
            Span::raw(entry.pid.to_string()),
        ]));
        lines.push(Line::from(vec![
            Span::styled(DETAIL_LABEL_SEV, Style::default().fg(COLOR_MUTED)),
            Span::styled(entry.severity.to_string(), severity_style(entry.severity)),
        ]));
        lines.push(Line::from(vec![
            Span::styled(DETAIL_LABEL_DESC, Style::default().fg(COLOR_MUTED)),
            Span::raw(entry.description.clone()),
        ]));
        lines.push(Line::from(vec![
            Span::styled(DETAIL_LABEL_CPU, Style::default().fg(COLOR_MUTED)),
            Span::raw(format!("{:.1}%", entry.cpu_percent)),
        ]));
        lines.push(Line::from(vec![
            Span::styled(DETAIL_LABEL_MEM, Style::default().fg(COLOR_MUTED)),
            Span::raw(format_memory(entry.memory_bytes)),
        ]));
        lines.push(Line::from(vec![
            Span::styled(DETAIL_LABEL_BIND, Style::default().fg(COLOR_MUTED)),
            Span::raw(entry.bind_addr.clone()),
        ]));
        lines.push(Line::from(vec![
            Span::styled(DETAIL_LABEL_EXPOSURE, Style::default().fg(COLOR_MUTED)),
            Span::styled(entry.exposure.to_string(), exposure_style(entry.exposure)),
        ]));
        lines.push(Line::from(vec![
            Span::styled(DETAIL_LABEL_AGE, Style::default().fg(COLOR_MUTED)),
            Span::raw(format_age(entry.age_seconds)),
        ]));
        lines.push(Line::from(vec![
            Span::styled(DETAIL_LABEL_PARENT, Style::default().fg(COLOR_MUTED)),
            Span::raw(match &entry.parent {
                Some((pid, name)) => format!("{name} ({pid})"),
                None => NO_PARENT_PLACEHOLDER.to_string(),
            }),
        ]));
        lines.push(Line::from(vec![
            Span::styled(DETAIL_LABEL_CWD, Style::default().fg(COLOR_MUTED)),
            Span::raw(
                entry
                    .cwd
                    .clone()
                    .unwrap_or_else(|| NO_CWD_PLACEHOLDER.to_string()),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled(DETAIL_LABEL_CMDLINE, Style::default().fg(COLOR_MUTED)),
            Span::raw(entry.cmdline.clone()),
        ]));
    } else {
        lines.push(Line::from(Span::styled(
            "No port selected",
            Style::default().fg(COLOR_MUTED),
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Severity legend",
        Style::default()
            .fg(COLOR_MUTED)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(Span::styled(
        Severity::Low.to_string(),
        severity_style(Severity::Low),
    )));
    lines.push(Line::from(Span::styled(
        Severity::Medium.to_string(),
        severity_style(Severity::Medium),
    )));
    lines.push(Line::from(Span::styled(
        Severity::High.to_string(),
        severity_style(Severity::High),
    )));
    lines.push(Line::from(Span::styled(
        Severity::Critical.to_string(),
        severity_style(Severity::Critical),
    )));

    // ponytail: ratatui's Wrap already reflows to the panel's actual width on
    // every resize, so cmdline/cwd wrap cleanly without a hand-rolled
    // truncation-width constant.
    let panel = Paragraph::new(lines).wrap(Wrap { trim: false }).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(COLOR_MUTED))
            .title(" Details ")
            .title_style(Style::default().fg(COLOR_ACCENT)),
    );

    frame.render_widget(panel, area);
}

fn render_table(frame: &mut Frame, area: Rect, app: &mut App) {
    let header_cells: Vec<Cell> = TABLE_COLUMNS
        .iter()
        .map(|&(column, _)| {
            let mark = if app.sort_mode == SortMode::Column(column) {
                if app.sort_ascending {
                    SORT_ASCENDING_MARK
                } else {
                    SORT_DESCENDING_MARK
                }
            } else {
                ""
            };
            Cell::from(format!("{}{}", column.label(), mark))
        })
        .chain(std::iter::once(Cell::from("Description")))
        .collect();

    let header = Row::new(header_cells)
        .style(Style::default().fg(COLOR_WARN).add_modifier(Modifier::BOLD))
        .bottom_margin(1);

    let rows: Vec<Row> = app
        .filtered_indices
        .iter()
        .map(|&idx| {
            let entry = &app.entries[idx];
            Row::new(vec![
                Cell::from(entry.port.to_string()),
                Cell::from(entry.command.clone()),
                Cell::from(entry.pid.to_string()),
                Cell::from(format!("{:.1}", entry.cpu_percent)),
                Cell::from(format_memory(entry.memory_bytes)),
                Cell::from(entry.severity.to_string()).style(severity_style(entry.severity)),
                Cell::from(entry.exposure.to_string()).style(exposure_style(entry.exposure)),
                Cell::from(entry.description.clone()),
            ])
        })
        .collect();

    let widths: Vec<Constraint> = TABLE_COLUMNS
        .iter()
        .map(|&(_, width)| Constraint::Length(width))
        .chain(std::iter::once(Constraint::Min(DESCRIPTION_MIN_WIDTH)))
        .collect();

    let table = Table::new(rows, widths)
        .header(header)
        .column_spacing(TABLE_COLUMN_SPACING)
        .row_highlight_style(
            Style::default()
                .bg(COLOR_ROW_HIGHLIGHT_BG)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(HIGHLIGHT_SYMBOL)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(COLOR_MUTED))
                .title(" Listening Ports ")
                .title_style(Style::default().fg(COLOR_ACCENT)),
        );

    // Seed with the recorded offset; ratatui adjusts it to keep the selection
    // visible, then we read it back so mouse hit-testing knows what's on screen.
    let mut table_state = TableState::default()
        .with_selected(Some(app.selected_index))
        .with_offset(app.table_offset);
    frame.render_stateful_widget(table, area, &mut table_state);
    app.record_table_offset(table_state.offset());
}

fn render_input(frame: &mut Frame, area: Rect, app: &App) {
    let input = Paragraph::new(Line::from(vec![
        Span::styled("Filter: ", Style::default().fg(COLOR_WARN)),
        Span::raw(&app.input_buffer),
        Span::styled("\u{2588}", Style::default().fg(COLOR_ACCENT)),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(COLOR_WARN))
            .title(" Search ")
            .title_style(Style::default().fg(COLOR_WARN)),
    );

    frame.render_widget(input, area);
}

fn render_help_bar(frame: &mut Frame, area: Rect, app: &App) {
    let help_text = match app.mode {
        AppMode::Normal => HELP_NORMAL,
        AppMode::Input => HELP_INPUT,
        AppMode::Confirm => HELP_CONFIRM,
    };

    frame.render_widget(
        Paragraph::new(help_text).style(Style::default().fg(COLOR_MUTED)),
        area,
    );
}

/// Wide enough for the longest prompt ("FORCE KILL (SIGKILL) ...").
const CONFIRM_MODAL_WIDTH: u16 = 60;
const CONFIRM_MODAL_HEIGHT: u16 = 7;

fn render_confirm_modal(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let modal_width = CONFIRM_MODAL_WIDTH.min(area.width.saturating_sub(4));
    let modal_height = CONFIRM_MODAL_HEIGHT;
    let modal_area = Rect {
        x: (area.width.saturating_sub(modal_width)) / 2,
        y: (area.height.saturating_sub(modal_height)) / 2,
        width: modal_width,
        height: modal_height,
    };

    frame.render_widget(Clear, modal_area);

    if let Some(entry) = app.selected_entry() {
        let prompt = if app.confirm_force {
            CONFIRM_PROMPT_FORCE
        } else {
            CONFIRM_PROMPT_NORMAL
        };
        let title = if app.confirm_force {
            CONFIRM_TITLE_FORCE
        } else {
            CONFIRM_TITLE_NORMAL
        };

        let text = vec![
            Line::from(""),
            Line::from(vec![Span::styled(
                format!(
                    "{prompt} \"{}\" (PID {}) on port {}?",
                    entry.command, entry.pid, entry.port
                ),
                if app.confirm_force {
                    Style::default()
                        .fg(COLOR_DANGER)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                },
            )]),
            Line::from(""),
            Line::from(vec![
                Span::styled(
                    "[y]",
                    Style::default()
                        .fg(COLOR_SUCCESS)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" Confirm    "),
                Span::styled(
                    "[n]",
                    Style::default()
                        .fg(COLOR_DANGER)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" Cancel"),
            ]),
        ];

        let modal = Paragraph::new(text).alignment(Alignment::Center).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(COLOR_DANGER))
                .title(title)
                .title_style(
                    Style::default()
                        .fg(COLOR_DANGER)
                        .add_modifier(Modifier::BOLD),
                ),
        );

        frame.render_widget(modal, modal_area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_layout_hides_filter_bar_outside_input_mode() {
        let area = Rect::new(0, 0, 100, 40);
        let layout = compute_layout(area, AppMode::Normal);
        assert_eq!(layout.filter.height, 0);

        let layout = compute_layout(area, AppMode::Input);
        assert_eq!(layout.filter.height, FILTER_BAR_HEIGHT);
    }

    #[test]
    fn compute_layout_splits_detail_and_table_panels() {
        let area = Rect::new(0, 0, 100, 40);
        let layout = compute_layout(area, AppMode::Normal);
        assert!(layout.detail.width < layout.table.width);
        assert_eq!(layout.detail.x, 0);
        assert_eq!(layout.table.x, layout.detail.width);
    }

    #[test]
    fn column_at_maps_header_clicks_to_columns() {
        let table_area = Rect::new(10, 5, 90, 20);
        let header_y = table_area.y + TABLE_BORDER_WIDTH;
        let port_x = table_area.x + TABLE_BORDER_WIDTH + HIGHLIGHT_SYMBOL_WIDTH;

        assert_eq!(
            column_at(table_area, port_x, header_y),
            Some(SortColumn::Port)
        );
        assert_eq!(column_at(table_area, port_x, header_y + 1), None);
    }

    #[test]
    fn column_at_maps_second_column() {
        let table_area = Rect::new(0, 0, 90, 20);
        let header_y = table_area.y + TABLE_BORDER_WIDTH;
        let command_x = table_area.x
            + TABLE_BORDER_WIDTH
            + HIGHLIGHT_SYMBOL_WIDTH
            + TABLE_COLUMNS[0].1
            + TABLE_COLUMN_SPACING;

        assert_eq!(
            column_at(table_area, command_x, header_y),
            Some(SortColumn::Command)
        );
    }

    #[test]
    fn row_at_maps_body_clicks_to_visible_offsets() {
        let table_area = Rect::new(10, 5, 90, 20);
        // Border (+1), header row (+1), header bottom margin (+1) → first data
        // row is 3 below the top of the table area.
        let first_data_row_y = table_area.y + 3;

        // Header row and its margin are not body rows.
        assert_eq!(row_at(table_area, table_area.y), None); // top border
        assert_eq!(row_at(table_area, table_area.y + 1), None); // header
        assert_eq!(row_at(table_area, table_area.y + 2), None); // header margin

        assert_eq!(row_at(table_area, first_data_row_y), Some(0));
        assert_eq!(row_at(table_area, first_data_row_y + 4), Some(4));

        // Bottom border and anything past it are ignored.
        let bottom_border_y = table_area.y + table_area.height - 1;
        assert_eq!(row_at(table_area, bottom_border_y), None);
    }
}
