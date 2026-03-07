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
        .constraints([Constraint::Min(3), Constraint::Length(3)])
        .split(frame.area());

    render_panes(frame, app, layout[0]);
    render_bottom_bar(frame, app, layout[1]);
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
            .map(|(index, entry)| render_entry(entry, active && start + index == pane.selected))
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

    Line::from(Span::styled(entry.display_name(), style))
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

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;
    use std::path::PathBuf;

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::{Color, Modifier, Stylize};
    use tempfile::TempDir;

    use super::render_entry;
    use crate::config::Config;
    use crate::fs::{EntryKind, FileEntry};
    use crate::state::{ActivePane, AppState};

    fn test_entry(kind: EntryKind, name: &str) -> FileEntry {
        FileEntry {
            name: OsString::from(name),
            path: PathBuf::from(name),
            kind,
            is_hidden: false,
        }
    }

    #[test]
    fn directory_entries_render_without_prefix_and_keep_directory_color() {
        let line = render_entry(&test_entry(EntryKind::Directory, "src"), false);

        assert_eq!(line.spans.len(), 1);
        assert_eq!(line.spans[0].content, "src");
        assert!(!line.spans[0].content.contains("[D]"));
        assert_eq!(line.spans[0].style, "src".cyan().style);
    }

    #[test]
    fn selected_entries_use_selected_row_style() {
        let line = render_entry(&test_entry(EntryKind::Directory, "src"), true);

        assert_eq!(line.spans.len(), 1);
        assert_eq!(line.spans[0].content, "src");
        assert_eq!(line.spans[0].style, "src".white().on_blue().bold().style);
    }

    #[test]
    fn inactive_pane_keeps_selection_state_without_drawing_highlight() {
        let temp = TempDir::new().expect("temp dir");
        let left_dir = temp.path().join("left");
        let right_dir = temp.path().join("right");
        fs::create_dir(&left_dir).expect("left dir");
        fs::create_dir(&right_dir).expect("right dir");
        fs::write(left_dir.join("alpha.txt"), b"a").expect("left file");
        fs::write(right_dir.join("zeta.txt"), b"z").expect("right file");

        let mut app = AppState::new(Config::default(), temp.path().to_path_buf()).expect("app");
        app.left.set_cwd(left_dir).expect("left cwd");
        app.right.set_cwd(right_dir).expect("right cwd");
        app.left.selected = 0;
        app.right.selected = 0;
        app.active_pane = ActivePane::Right;

        let backend = TestBackend::new(60, 6);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal.draw(|frame| super::render(frame, &mut app)).expect("draw");

        let buffer = terminal.backend().buffer();
        let left_cell = &buffer[(1, 1)];
        let right_cell = &buffer[(31, 1)];

        assert_eq!(left_cell.symbol(), "a");
        assert_eq!(left_cell.bg, Color::Reset);
        assert!(!left_cell.modifier.contains(Modifier::BOLD));

        assert_eq!(right_cell.symbol(), "z");
        assert_eq!(right_cell.fg, Color::White);
        assert_eq!(right_cell.bg, Color::Blue);
        assert!(right_cell.modifier.contains(Modifier::BOLD));
    }
}
