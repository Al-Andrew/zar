use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::source::{EntryKind, FileEntry};
use crate::state::{
    ActivePane, AddLocationControl, AddLocationDialogState, AppState, InputMode, PaneState,
    SourceMenuLevel, TransferControl, TransferDialogState,
};

const FOOTER_BUTTON_COUNT: usize = 10;
const FOOTER_HEIGHT: u16 = 3;

pub fn render(frame: &mut Frame<'_>, app: &mut AppState) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(FOOTER_HEIGHT)])
        .split(frame.area());

    render_panes(frame, app, layout[0]);
    render_bottom_bar(frame, app, layout[1]);

    match app.mode {
        InputMode::Transfer => render_transfer_dialog(frame, app),
        InputMode::Preview => render_preview(frame, app),
        InputMode::AddLocation => render_add_location_dialog(frame, app),
        _ => {}
    }
}

pub fn bottom_bar_hit_target(app: &AppState, frame_area: Rect, x: u16, y: u16) -> Option<usize> {
    if app.mode != InputMode::Normal {
        return None;
    }

    let area = bottom_bar_area(frame_area);
    let slots = bottom_bar_button_areas(area);
    slots.iter().enumerate().find_map(|(index, slot)| {
        let label = bottom_bar_buttons()[index];
        if !label.is_empty() && rect_contains(*slot, x, y) {
            Some(index)
        } else {
            None
        }
    })
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
        app.mode == InputMode::SourceMenu
            && app
                .source_menu
                .as_ref()
                .is_some_and(|menu| menu.target_pane == ActivePane::Left),
        app.source_menu.as_ref(),
    );
    render_pane(
        frame,
        &app.right,
        panes[1],
        matches!(app.active_pane, ActivePane::Right),
        app.mode == InputMode::SourceMenu
            && app
                .source_menu
                .as_ref()
                .is_some_and(|menu| menu.target_pane == ActivePane::Right),
        app.source_menu.as_ref(),
    );
}

fn render_pane(
    frame: &mut Frame<'_>,
    pane: &PaneState,
    area: Rect,
    active: bool,
    show_source_menu: bool,
    source_menu: Option<&crate::state::SourceMenuState>,
) {
    let border_style = if active {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let block = Block::default()
        .title(pane.title())
        .borders(Borders::ALL)
        .border_style(border_style);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if show_source_menu {
        render_source_menu(frame, inner, source_menu.expect("source menu"));
        return;
    }

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

fn render_source_menu(frame: &mut Frame<'_>, area: Rect, menu: &crate::state::SourceMenuState) {
    let lines: Vec<Line<'static>> = match menu.level {
        SourceMenuLevel::Categories => crate::state::SourceMenuState::categories()
            .iter()
            .enumerate()
            .map(|(index, category)| {
                let style = if index == menu.category_selected {
                    Style::default()
                        .bg(Color::Blue)
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Cyan)
                };
                Line::from(Span::styled(category.title().to_string(), style))
            })
            .collect(),
        SourceMenuLevel::Items(_) => {
            if menu.items.is_empty() {
                vec![Line::from(Span::styled(
                    "<empty>",
                    Style::default().fg(Color::DarkGray),
                ))]
            } else {
                menu.items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| {
                        let style = if index == menu.item_selected {
                            Style::default()
                                .bg(Color::Blue)
                                .fg(Color::White)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default()
                        };
                        Line::from(Span::styled(
                            format!("{} | {}", item.label, item.path_hint.display()),
                            style,
                        ))
                    })
                    .collect()
            }
        }
    };
    frame.render_widget(Paragraph::new(lines), area);
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
    match app.mode {
        InputMode::Normal
        | InputMode::Transfer
        | InputMode::Preview
        | InputMode::SourceMenu
        | InputMode::AddLocation => {
            for (index, slot) in bottom_bar_button_areas(area).into_iter().enumerate() {
                let label = bottom_bar_buttons()[index];
                if label.is_empty() {
                    continue;
                }

                render_dialog_button(
                    frame,
                    slot,
                    label,
                    app.mode == InputMode::Normal && app.footer_hovered == Some(index),
                );
            }
        }
        InputMode::Command => {
            let command_area = area;
            let block = Block::default().borders(Borders::ALL);
            let inner = block.inner(command_area);
            frame.render_widget(block, command_area);

            let prompt = format!("cmd> {}", app.command.buffer);
            let prompt_width = prompt.chars().count() as u16;
            let prompt_x = inner
                .x
                .saturating_add(inner.width.saturating_sub(prompt_width) / 2);
            frame.render_widget(
                Paragraph::new(prompt.clone()).alignment(Alignment::Center),
                inner,
            );

            let cursor_x = prompt_x
                .saturating_add(5)
                .saturating_add(app.command.buffer[..app.command.cursor].chars().count() as u16);
            frame.set_cursor_position((cursor_x, inner.y));
        }
    }
}

fn render_preview(frame: &mut Frame<'_>, app: &AppState) {
    let Some(preview) = app.preview.as_ref() else {
        return;
    };

    let area = frame.area();
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(preview.title.clone())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    let visible_height = rows[0].height as usize;
    let end = (preview.scroll + visible_height).min(preview.lines.len());
    let lines: Vec<Line<'static>> = preview.lines[preview.scroll..end]
        .iter()
        .cloned()
        .map(Line::from)
        .collect();

    frame.render_widget(Paragraph::new(lines), rows[0]);
    frame.render_widget(
        Paragraph::new("F3/Esc close | Up/Down scroll").alignment(Alignment::Center),
        rows[1],
    );
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
            [
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Min(0),
            ]
        })
        .split(inner);

    let input_area = if dialog.operation.shows_source() {
        render_transfer_field(
            frame,
            rows[0],
            "From",
            &format!("{} | {}", dialog.source_label, dialog.source.display()),
            transfer_control_highlighted(dialog, TransferControl::SourceField),
        );
        rows[1]
    } else {
        rows[0]
    };

    let input_value = if dialog.operation.edits_destination() {
        dialog.destination.clone()
    } else {
        dialog.source.display()
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

fn render_add_location_dialog(frame: &mut Frame<'_>, app: &AppState) {
    let Some(dialog) = app.add_location.as_ref() else {
        return;
    };

    let layout = add_location_dialog_layout(frame.area());
    let area = layout.area;
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title("Add Location")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(2),
        ])
        .split(inner);

    render_transfer_field(
        frame,
        rows[0],
        "Type",
        dialog.kind.label(),
        add_location_control_highlighted(dialog, AddLocationControl::KindField),
    );
    render_transfer_field(
        frame,
        rows[1],
        "Label",
        &dialog.label,
        add_location_control_highlighted(dialog, AddLocationControl::LabelField),
    );
    render_transfer_field(
        frame,
        rows[2],
        dialog.kind.target_label(),
        &dialog.target,
        add_location_control_highlighted(dialog, AddLocationControl::TargetField),
    );
    let secret_value = if dialog.kind.uses_secret() {
        "*".repeat(dialog.secret.chars().count())
    } else {
        "<not used>".to_string()
    };
    render_transfer_field(
        frame,
        rows[3],
        "Secret",
        &secret_value,
        add_location_control_highlighted(dialog, AddLocationControl::SecretField),
    );

    frame.render_widget(
        Paragraph::new(format!(
            "Example: {} | Enter save | Esc cancel",
            dialog.kind.target_example()
        ))
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::DarkGray)),
        layout.help_text,
    );

    render_dialog_button(
        frame,
        layout.confirm_button,
        "Save (Enter)",
        add_location_control_highlighted(dialog, AddLocationControl::ConfirmButton),
    );
    render_dialog_button(
        frame,
        layout.cancel_button,
        "Cancel (Esc)",
        add_location_control_highlighted(dialog, AddLocationControl::CancelButton),
    );

    let cursor_area = match dialog.focus {
        AddLocationControl::LabelField => Some((rows[1], dialog.label_cursor)),
        AddLocationControl::TargetField => Some((rows[2], dialog.target_cursor)),
        AddLocationControl::SecretField if dialog.kind.uses_secret() => {
            Some((rows[3], dialog.secret_cursor))
        }
        _ => None,
    };
    if let Some((field_area, cursor)) = cursor_area {
        let input_inner = padded_inner_rect(field_area, 1);
        let text = match dialog.focus {
            AddLocationControl::LabelField => &dialog.label,
            AddLocationControl::TargetField => &dialog.target,
            AddLocationControl::SecretField => &dialog.secret,
            _ => "",
        };
        let cursor_x = input_inner
            .x
            .saturating_add(text[..cursor].chars().count() as u16);
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
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(
        Paragraph::new(label)
            .style(text_style)
            .alignment(Alignment::Center),
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

fn bottom_bar_buttons() -> [&'static str; FOOTER_BUTTON_COUNT] {
    [
        "",
        "",
        "View (F3)",
        "Add (F4)",
        "Copy (F5)",
        "Move (F6)",
        "Mkdir (F7)",
        "Delete (F8)",
        "",
        "",
    ]
}

fn bottom_bar_area(frame_area: Rect) -> Rect {
    Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(FOOTER_HEIGHT)])
        .split(frame_area)[1]
}

fn bottom_bar_button_areas(area: Rect) -> Vec<Rect> {
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1, FOOTER_BUTTON_COUNT as u32); FOOTER_BUTTON_COUNT])
        .split(area)
        .iter()
        .copied()
        .collect()
}

fn transfer_control_highlighted(dialog: &TransferDialogState, control: TransferControl) -> bool {
    dialog.focus == control || dialog.hovered == Some(control)
}

fn add_location_control_highlighted(
    dialog: &AddLocationDialogState,
    control: AddLocationControl,
) -> bool {
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

struct AddLocationDialogLayout {
    area: Rect,
    help_text: Rect,
    confirm_button: Rect,
    cancel_button: Rect,
}

fn transfer_dialog_layout(dialog: &TransferDialogState, frame_area: Rect) -> TransferDialogLayout {
    let area = centered_rect(
        70,
        if dialog.operation.shows_source() {
            11
        } else {
            9
        },
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
            [
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Min(0),
            ]
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

fn add_location_dialog_layout(frame_area: Rect) -> AddLocationDialogLayout {
    let area = centered_rect(72, 19, frame_area);
    let inner = Block::default().borders(Borders::ALL).inner(area);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(2),
        ])
        .split(inner);
    let buttons = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[5]);

    AddLocationDialogLayout {
        area,
        help_text: rows[4],
        confirm_button: buttons[0],
        cancel_button: buttons[1],
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
    use std::fs;

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::layout::Rect;
    use tempfile::TempDir;

    use super::{bottom_bar_hit_target, render_entry};
    use crate::config::Config;
    use crate::source::{EntryKind, FileEntry, LocationPath, SourceKind, SourceRef};
    use crate::state::{AppState, InputMode, PaneState, SourceMenuState};
    use crate::vfs::LocalSession;

    fn test_entry(kind: EntryKind, name: &str) -> FileEntry {
        FileEntry {
            name: name.to_string(),
            path: LocationPath::Remote(format!("/{name}")),
            kind,
            is_hidden: false,
        }
    }

    fn test_state() -> AppState {
        let temp = TempDir::new().expect("temp dir");
        fs::create_dir(temp.path().join("src")).expect("dir");
        let left = PaneState::new(
            SourceRef::InlineLocal {
                path: temp.path().to_path_buf(),
                label: "Tmp".to_string(),
            },
            SourceKind::Local,
            "Tmp".to_string(),
            LocationPath::Local(temp.path().to_path_buf()),
            Box::new(
                LocalSession::new("Tmp".to_string(), temp.path().to_path_buf()).expect("session"),
            ),
        )
        .expect("pane");
        let right = PaneState::new(
            SourceRef::InlineLocal {
                path: temp.path().to_path_buf(),
                label: "Tmp".to_string(),
            },
            SourceKind::Local,
            "Tmp".to_string(),
            LocationPath::Local(temp.path().to_path_buf()),
            Box::new(
                LocalSession::new("Tmp".to_string(), temp.path().to_path_buf()).expect("session"),
            ),
        )
        .expect("pane");
        AppState::new(Config::default(), left, right)
    }

    #[test]
    fn directory_entries_render_without_prefix_and_keep_directory_color() {
        let line = render_entry(&test_entry(EntryKind::Directory, "src"), false);

        assert_eq!(line.spans.len(), 1);
        assert_eq!(line.spans[0].content, "src");
    }

    #[test]
    fn bottom_bar_hit_target_ignores_empty_slots_and_maps_f_buttons() {
        let app = test_state();
        assert_eq!(
            bottom_bar_hit_target(&app, Rect::new(0, 0, 100, 20), 25, 18),
            Some(2)
        );
    }

    #[test]
    fn source_menu_renders_inside_target_pane() {
        let mut app = test_state();
        app.mode = InputMode::SourceMenu;
        app.source_menu = Some(SourceMenuState::new(crate::state::ActivePane::Left));

        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| super::render(frame, &mut app))
            .expect("draw");
    }
}
