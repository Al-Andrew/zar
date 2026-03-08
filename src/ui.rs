use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::fs::{EntryKind, FileEntry};
use crate::state::{ActivePane, AppState, InputMode, PaneState};

pub fn render(frame: &mut Frame<'_>, app: &mut AppState) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(3)])
        .split(frame.area());

    render_panes(frame, app, layout[0]);
    render_bottom_bar(frame, app, layout[1]);

    if matches!(app.mode, InputMode::Transfer) {
        render_transfer_dialog(frame, app);
    }
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
        InputMode::Normal | InputMode::Transfer => {
            let slots = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Ratio(1, 10); 10])
                .split(inner);

            for (index, slot) in slots.iter().enumerate() {
                let borders = if index + 1 < slots.len() {
                    Borders::RIGHT
                } else {
                    Borders::NONE
                };
                frame.render_widget(Block::default().borders(borders), *slot);
            }
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

fn render_transfer_dialog(frame: &mut Frame<'_>, app: &AppState) {
    let Some(dialog) = app.transfer.as_ref() else {
        return;
    };

    let area = centered_rect(70, 7, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(dialog.operation.title())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(if dialog.operation.shows_source() {
            [
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ]
        } else {
            [
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ]
        })
        .split(inner);

    let input_row = if dialog.operation.shows_source() {
        frame.render_widget(
            Paragraph::new(format!("From: {}", dialog.source.display())),
            rows[0],
        );
        rows[1]
    } else {
        rows[0]
    };

    frame.render_widget(
        Paragraph::new(format!(
            "{}: {}",
            dialog.operation.destination_label(),
            if dialog.operation.edits_destination() {
                dialog.destination.clone()
            } else {
                dialog.source.display().to_string()
            }
        )),
        input_row,
    );
    frame.render_widget(
        Paragraph::new("Enter confirm | Esc cancel"),
        if dialog.operation.shows_source() {
            rows[2]
        } else {
            rows[1]
        },
    );

    if dialog.operation.edits_destination() {
        let cursor_x = input_row
            .x
            .saturating_add(dialog.operation.destination_label().len() as u16 + 2)
            .saturating_add(dialog.destination[..dialog.cursor].chars().count() as u16);
        frame.set_cursor_position((cursor_x, input_row.y));
    }
}

fn centered_rect(width_percent: u16, height: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Fill(1),
            Constraint::Length(height),
            Constraint::Fill(1),
        ])
        .split(area);
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - width_percent) / 2),
            Constraint::Percentage(width_percent),
            Constraint::Percentage((100 - width_percent) / 2),
        ])
        .split(vertical[1]);

    horizontal[1]
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
    use crate::state::{ActivePane, AppState, InputMode, TransferDialogState, TransferOperation};

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

    #[test]
    fn normal_mode_bottom_bar_renders_empty_ten_slot_layout() {
        let temp = TempDir::new().expect("temp dir");
        let app = AppState::new(Config::default(), temp.path().to_path_buf()).expect("app");

        let backend = TestBackend::new(22, 3);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| super::render_bottom_bar(frame, &app, frame.area()))
            .expect("draw");

        let buffer = terminal.backend().buffer();
        let separator_columns = [2_u16, 4, 6, 8, 10, 12, 14, 16, 18];

        for x in 1..21 {
            let cell = &buffer[(x, 1)];
            if separator_columns.contains(&x) {
                assert_eq!(cell.symbol(), "│");
            } else {
                assert_eq!(cell.symbol(), " ");
            }
        }
    }

    #[test]
    fn transfer_dialog_renders_title_and_destination() {
        let temp = TempDir::new().expect("temp dir");
        let mut app = AppState::new(Config::default(), temp.path().to_path_buf()).expect("app");
        app.mode = InputMode::Transfer;
        app.transfer = Some(TransferDialogState::new(
            TransferOperation::Copy,
            temp.path().join("source.txt"),
            "/tmp/dest".to_string(),
        ));

        let backend = TestBackend::new(60, 12);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal.draw(|frame| super::render(frame, &mut app)).expect("draw");

        let buffer = terminal.backend().buffer();
        let rendered: String = (0..buffer.area.height)
            .flat_map(|y| {
                (0..buffer.area.width)
                    .map(move |x| buffer[(x, y)].symbol().to_string())
                    .chain(std::iter::once("\n".to_string()))
            })
            .collect();

        assert!(rendered.contains("Copy File"));
        assert!(rendered.contains("To: /tmp/dest"));
    }

    #[test]
    fn create_directory_dialog_renders_path_prompt() {
        let temp = TempDir::new().expect("temp dir");
        let mut app = AppState::new(Config::default(), temp.path().to_path_buf()).expect("app");
        app.mode = InputMode::Transfer;
        app.transfer = Some(TransferDialogState::new(
            TransferOperation::CreateDirectory,
            temp.path().to_path_buf(),
            "/tmp/new-dir".to_string(),
        ));

        let backend = TestBackend::new(60, 12);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal.draw(|frame| super::render(frame, &mut app)).expect("draw");

        let buffer = terminal.backend().buffer();
        let rendered: String = (0..buffer.area.height)
            .flat_map(|y| {
                (0..buffer.area.width)
                    .map(move |x| buffer[(x, y)].symbol().to_string())
                    .chain(std::iter::once("\n".to_string()))
            })
            .collect();

        assert!(rendered.contains("Create Directory"));
        assert!(rendered.contains("Path: /tmp/new-dir"));
    }

    #[test]
    fn delete_dialog_renders_target_prompt() {
        let temp = TempDir::new().expect("temp dir");
        let mut app = AppState::new(Config::default(), temp.path().to_path_buf()).expect("app");
        app.mode = InputMode::Transfer;
        app.transfer = Some(TransferDialogState::new(
            TransferOperation::Delete,
            temp.path().join("victim.txt"),
            String::new(),
        ));

        let backend = TestBackend::new(60, 12);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal.draw(|frame| super::render(frame, &mut app)).expect("draw");

        let buffer = terminal.backend().buffer();
        let rendered: String = (0..buffer.area.height)
            .flat_map(|y| {
                (0..buffer.area.width)
                    .map(move |x| buffer[(x, y)].symbol().to_string())
                    .chain(std::iter::once("\n".to_string()))
            })
            .collect();

        assert!(rendered.contains("Delete"));
        assert!(rendered.contains("Target:"));
        assert!(rendered.contains("victim.txt"));
    }
}
