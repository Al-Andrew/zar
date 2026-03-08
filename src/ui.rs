use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::fs::{EntryKind, FileEntry};
use crate::state::{
    ActivePane, AppState, InputMode, PaneState, TransferControl, TransferDialogState,
};

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

pub fn transfer_dialog_hit_target(
    app: &AppState,
    frame_area: Rect,
    x: u16,
    y: u16,
) -> Option<TransferControl> {
    let dialog = app.transfer.as_ref()?;
    let layout = transfer_dialog_layout(dialog, frame_area);

    if layout
        .source_field
        .is_some_and(|source_field| rect_contains(source_field, x, y))
    {
        Some(TransferControl::SourceField)
    } else if rect_contains(layout.destination_field, x, y) {
        Some(TransferControl::DestinationField)
    } else if rect_contains(layout.confirm_button, x, y) {
        Some(TransferControl::ConfirmButton)
    } else if rect_contains(layout.cancel_button, x, y) {
        Some(TransferControl::CancelButton)
    } else {
        None
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

    let layout = transfer_dialog_layout(dialog, frame.area());
    let area = layout.area;
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
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
            ]
        } else {
            [Constraint::Length(3), Constraint::Length(3), Constraint::Min(0)]
        })
        .split(inner);

    let input_area = if dialog.operation.shows_source() {
        render_transfer_field(
            frame,
            rows[0],
            "From",
            &dialog.source.display().to_string(),
            transfer_control_highlighted(dialog, TransferControl::SourceField),
        );
        rows[1]
    } else {
        rows[0]
    };

    let input_value = if dialog.operation.edits_destination() {
        dialog.destination.clone()
    } else {
        dialog.source.display().to_string()
    };
    render_transfer_field(
        frame,
        input_area,
        dialog.operation.destination_label(),
        &input_value,
        transfer_control_highlighted(dialog, TransferControl::DestinationField),
    );

    render_dialog_button(
        frame,
        layout.confirm_button,
        "Confirm (Enter)",
        transfer_control_highlighted(dialog, TransferControl::ConfirmButton),
    );
    render_dialog_button(
        frame,
        layout.cancel_button,
        "Cancel (Esc)",
        transfer_control_highlighted(dialog, TransferControl::CancelButton),
    );

    if dialog.operation.edits_destination() {
        let input_inner = padded_inner_rect(input_area, 1);
        let cursor_x = input_inner
            .x
            .saturating_add(dialog.destination[..dialog.cursor].chars().count() as u16);
        frame.set_cursor_position((cursor_x, input_inner.y));
    }
}

fn render_dialog_button(frame: &mut Frame<'_>, area: Rect, label: &str, highlighted: bool) {
    let border_style = if highlighted {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let text_style = if highlighted {
        Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(
        Paragraph::new(label).style(text_style).alignment(Alignment::Center),
        inner,
    );
}

fn render_transfer_field(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    value: &str,
    active: bool,
) {
    let border_style = if active {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(border_style);
    let inner = padded_inner_rect(area, 1);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(value), inner);
}

fn transfer_control_highlighted(dialog: &TransferDialogState, control: TransferControl) -> bool {
    dialog.focus == control || dialog.hovered == Some(control)
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

struct TransferDialogLayout {
    area: Rect,
    source_field: Option<Rect>,
    destination_field: Rect,
    confirm_button: Rect,
    cancel_button: Rect,
}

fn transfer_dialog_layout(dialog: &TransferDialogState, frame_area: Rect) -> TransferDialogLayout {
    let area = centered_rect(
        70,
        if dialog.operation.shows_source() { 11 } else { 9 },
        frame_area,
    );
    let inner = Block::default().borders(Borders::ALL).inner(area);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(if dialog.operation.shows_source() {
            [
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
            ]
        } else {
            [Constraint::Length(3), Constraint::Length(3), Constraint::Min(0)]
        })
        .split(inner);

    let button_row = if dialog.operation.shows_source() {
        rows[2]
    } else {
        rows[1]
    };
    let buttons = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Fill(1),
            Constraint::Length(18),
            Constraint::Length(2),
            Constraint::Length(16),
            Constraint::Fill(1),
        ])
        .split(button_row);

    TransferDialogLayout {
        area,
        source_field: dialog.operation.shows_source().then_some(rows[0]),
        destination_field: if dialog.operation.shows_source() {
            rows[1]
        } else {
            rows[0]
        },
        confirm_button: buttons[1],
        cancel_button: buttons[3],
    }
}

fn padded_inner_rect(area: Rect, horizontal_padding: u16) -> Rect {
    let inner = Block::default().borders(Borders::ALL).inner(area);
    Rect {
        x: inner.x.saturating_add(horizontal_padding),
        y: inner.y,
        width: inner.width.saturating_sub(horizontal_padding),
        height: inner.height,
    }
}

fn rect_contains(area: Rect, x: u16, y: u16) -> bool {
    x >= area.x
        && x < area.x.saturating_add(area.width)
        && y >= area.y
        && y < area.y.saturating_add(area.height)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;
    use std::path::PathBuf;

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::layout::Rect;
    use ratatui::style::{Color, Modifier, Stylize};
    use tempfile::TempDir;

    use super::{render_entry, transfer_dialog_hit_target, transfer_dialog_layout};
    use crate::config::Config;
    use crate::fs::{EntryKind, FileEntry};
    use crate::state::{
        ActivePane, AppState, InputMode, TransferControl, TransferDialogState, TransferOperation,
    };

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
        assert!(rendered.contains("From"));
        assert!(rendered.contains("To"));
        assert!(rendered.contains("/tmp/dest"));
        assert!(rendered.contains("Confirm (Enter)"));
        assert!(rendered.contains("Cancel (Esc)"));
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
        assert!(rendered.contains("Path"));
        assert!(rendered.contains("/tmp/new-dir"));
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
        assert!(rendered.contains("Target"));
        assert!(rendered.contains("victim.txt"));
    }

    #[test]
    fn transfer_dialog_maps_hits_to_controls() {
        let temp = TempDir::new().expect("temp dir");
        let mut app = AppState::new(Config::default(), temp.path().to_path_buf()).expect("app");
        app.mode = InputMode::Transfer;
        app.transfer = Some(TransferDialogState::new(
            TransferOperation::Copy,
            temp.path().join("source.txt"),
            "/tmp/dest".to_string(),
        ));

        assert_eq!(
            transfer_dialog_hit_target(&app, Rect::new(0, 0, 60, 12), 16, 9),
            Some(TransferControl::ConfirmButton)
        );
        assert_eq!(
            transfer_dialog_hit_target(&app, Rect::new(0, 0, 60, 12), 36, 9),
            Some(TransferControl::CancelButton)
        );
    }

    #[test]
    fn hovered_transfer_button_uses_highlighted_border() {
        let temp = TempDir::new().expect("temp dir");
        let mut app = AppState::new(Config::default(), temp.path().to_path_buf()).expect("app");
        app.mode = InputMode::Transfer;
        app.transfer = Some(TransferDialogState::new(
            TransferOperation::Copy,
            temp.path().join("source.txt"),
            "/tmp/dest".to_string(),
        ));
        app.transfer.as_mut().expect("dialog").hovered = Some(TransferControl::CancelButton);

        let backend = TestBackend::new(60, 12);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal.draw(|frame| super::render(frame, &mut app)).expect("draw");

        let cancel_button = transfer_dialog_layout(
            app.transfer.as_ref().expect("dialog"),
            Rect::new(0, 0, 60, 12),
        )
        .cancel_button;
        let border_cell = &terminal.backend().buffer()[(cancel_button.x, cancel_button.y)];

        assert_eq!(border_cell.fg, Color::Yellow);
    }
}
