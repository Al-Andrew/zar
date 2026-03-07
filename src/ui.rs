use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::fs::{EntryKind, FileEntry};
use crate::state::{ActivePane, AppState, InputMode, PaneState, StatusKind};

pub fn render(frame: &mut Frame<'_>, app: &mut AppState) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(3),
        ])
        .split(frame.area());

    render_header(frame, app, layout[0]);
    render_panes(frame, app, layout[1]);
    render_bottom_bar(frame, app, layout[2]);
}

fn render_header(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
    let active = match app.active_pane {
        ActivePane::Left => "left",
        ActivePane::Right => "right",
    };
    let mode = match app.mode {
        InputMode::Normal => "normal",
        InputMode::Command => "command",
    };
    let text = Line::from(vec![
        Span::styled(" zar ", Style::default().fg(Color::Black).bg(Color::Cyan)),
        Span::raw(format!(" active: {active} ")),
        Span::raw(format!(" mode: {mode} ")),
    ]);

    frame.render_widget(Paragraph::new(text), area);
}

fn render_panes(frame: &mut Frame<'_>, app: &mut AppState, area: Rect) {
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let visible_height = panes[0].height.saturating_sub(2) as usize;
    app.left.ensure_visible(visible_height);
    app.right.ensure_visible(visible_height);

    render_pane(
        frame,
        &app.left,
        panes[0],
        matches!(app.active_pane, ActivePane::Left),
    );
    render_pane(
        frame,
        &app.right,
        panes[1],
        matches!(app.active_pane, ActivePane::Right),
    );
}

fn render_pane(frame: &mut Frame<'_>, pane: &PaneState, area: Rect, active: bool) {
    let border_style = if active {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let block = Block::default()
        .title(pane.cwd.display().to_string())
        .borders(Borders::ALL)
        .border_style(border_style);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let height = inner.height as usize;
    let start = pane.scroll.min(pane.entries.len());
    let end = (start + height).min(pane.entries.len());

    let lines: Vec<Line<'static>> = if pane.entries.is_empty() {
        vec![Line::from(Span::styled(
            "<empty>",
            Style::default().fg(Color::DarkGray),
        ))]
    } else {
        pane.entries[start..end]
            .iter()
            .enumerate()
            .map(|(index, entry)| render_entry(entry, start + index == pane.selected))
            .collect()
    };

    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_entry(entry: &FileEntry, selected: bool) -> Line<'static> {
    let mut style = match entry.kind {
        EntryKind::Directory => Style::default().fg(Color::Cyan),
        EntryKind::File => Style::default(),
        EntryKind::Symlink => Style::default().fg(Color::Magenta),
        EntryKind::Other => Style::default().fg(Color::Gray),
    };

    if selected {
        style = style
            .bg(Color::Blue)
            .fg(Color::White)
            .add_modifier(Modifier::BOLD);
    }

    let prefix = if entry.kind.is_directory() {
        "[D]"
    } else {
        "   "
    };
    Line::from(Span::styled(
        format!("{prefix} {}", entry.display_name()),
        style,
    ))
}

fn render_bottom_bar(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
    let block = Block::default().borders(Borders::ALL);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    match app.mode {
        InputMode::Normal => {
            let help = format!(
                "{} | {} command | Tab switch pane | Enter open | Backspace up | q quit",
                app.status.text,
                app.command.trigger_key.label()
            );
            let style = match app.status.kind {
                StatusKind::Info => Style::default(),
                StatusKind::Error => Style::default().fg(Color::Red),
            };
            frame.render_widget(Paragraph::new(help).style(style), inner);
        }
        InputMode::Command => {
            let prompt = format!("cmd> {}", app.command.buffer);
            frame.render_widget(Paragraph::new(prompt), inner);

            let cursor_x = inner
                .x
                .saturating_add(5)
                .saturating_add(app.command.buffer[..app.command.cursor].chars().count() as u16);
            frame.set_cursor_position((cursor_x, inner.y));
        }
    }
}
