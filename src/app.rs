use std::ffi::OsStr;
use std::io::Stdout;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::event::{self, MouseButton, MouseEvent, MouseEventKind};
use directories::BaseDirs;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::commands;
use crate::config::Config;
use crate::fs::{self, EntryKind};
use crate::input::{Action, CommandEditAction, event_to_action};
use crate::state::{
    ActivePane, AppState, InputMode, StatusMessage, TransferControl, TransferDialogState,
    TransferOperation,
};
use crate::ui;

pub struct App {
    pub state: AppState,
}

impl App {
    pub fn new(config: Config) -> Result<Self> {
        Self::new_with_start_dirs(config, None, None)
    }

    pub fn new_with_start_dir(config: Config, start_dir: Option<PathBuf>) -> Result<Self> {
        Self::new_with_start_dirs(config, start_dir, None)
    }

    pub fn new_with_start_dirs(
        config: Config,
        left_start_dir: Option<PathBuf>,
        right_start_dir: Option<PathBuf>,
    ) -> Result<Self> {
        let (left_cwd, right_cwd) = resolve_start_dirs(left_start_dir, right_start_dir)?;
        let state = AppState::new_with_dirs(config, left_cwd, right_cwd)?;
        Ok(Self { state })
    }

    pub fn run(&mut self, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
        while !self.state.should_quit {
            terminal.draw(|frame| ui::render(frame, &mut self.state))?;

            if event::poll(Duration::from_millis(250))? {
                let event = event::read()?;
                match event {
                    event::Event::Mouse(mouse_event) => {
                        let size = terminal.size()?;
                        self.handle_mouse(
                            mouse_event,
                            ratatui::layout::Rect::new(0, 0, size.width, size.height),
                        )?;
                    }
                    other => {
                        if let Some(action) = event_to_action(
                            &self.state.config.key_bindings,
                            self.state.mode,
                            other,
                        ) {
                            self.handle_action(action)?;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    pub fn handle_action(&mut self, action: Action) -> Result<()> {
        match action {
            Action::MoveUp => self.state.active_pane_mut().move_up(),
            Action::MoveDown => self.state.active_pane_mut().move_down(),
            Action::SwitchPane => {
                self.state.active_pane = match self.state.active_pane {
                    ActivePane::Left => ActivePane::Right,
                    ActivePane::Right => ActivePane::Left,
                };
                self.state.set_status(StatusMessage::info(format!(
                    "active pane: {}",
                    match self.state.active_pane {
                        ActivePane::Left => "left",
                        ActivePane::Right => "right",
                    }
                )));
            }
            Action::OpenSelection => self.open_selection(),
            Action::GoParent => match self.state.active_pane_mut().go_parent() {
                Ok(true) => {
                    let path = self.state.active_pane().cwd.display().to_string();
                    self.state
                        .set_status(StatusMessage::info(format!("active pane: {path}")));
                }
                Ok(false) => self
                    .state
                    .set_status(StatusMessage::info("already at filesystem root")),
                Err(err) => self.state.set_status(StatusMessage::error(err.to_string())),
            },
            Action::BeginCopy => self.begin_transfer(TransferOperation::Copy),
            Action::BeginMove => self.begin_transfer(TransferOperation::Move),
            Action::BeginCreateDirectory => self.begin_create_directory(),
            Action::BeginDelete => self.begin_delete(),
            Action::EnterCommandMode => {
                self.state.mode = InputMode::Command;
                self.state.command.clear();
                self.state
                    .set_status(StatusMessage::info("command mode: enter a command"));
            }
            Action::EditCommand(edit) => self.edit_command(edit),
            Action::EditTransfer(edit) => self.edit_transfer(edit),
            Action::TransferFocusUp => self.move_transfer_focus_up(),
            Action::TransferFocusDown => self.move_transfer_focus_down(),
            Action::TransferFocusLeft => self.move_transfer_focus_left(),
            Action::TransferFocusRight => self.move_transfer_focus_right(),
            Action::SubmitCommand => self.submit_command(),
            Action::SubmitTransfer => self.submit_transfer(),
            Action::CancelCommand => {
                self.state.command.clear();
                self.state.mode = InputMode::Normal;
                self.state
                    .set_status(StatusMessage::info("command cancelled"));
            }
            Action::CancelTransfer => {
                self.state.transfer = None;
                self.state.mode = InputMode::Normal;
                self.state
                    .set_status(StatusMessage::info("file operation cancelled"));
            }
            Action::Quit => self.state.should_quit = true,
            Action::ClearStatus => self.state.set_status(StatusMessage::info(format!(
                "Tab switch pane | Enter open | Backspace up | {} command | q quit",
                self.state.command.trigger_key.label()
            ))),
        }

        Ok(())
    }

    pub fn change_active_directory(&mut self, path: PathBuf) -> Result<()> {
        if !path.is_dir() {
            anyhow::bail!("not a directory: {}", path.display());
        }

        self.state.active_pane_mut().set_cwd(path)
    }

    fn begin_transfer(&mut self, operation: TransferOperation) {
        let Some(entry) = self.state.active_pane().selected_entry().cloned() else {
            self.state.set_status(StatusMessage::info("nothing selected"));
            return;
        };

        if !matches!(entry.kind, EntryKind::File) {
            self.state.set_status(StatusMessage::error(format!(
                "{} supports files only",
                operation.label()
            )));
            return;
        }

        let destination = self.state.inactive_pane().cwd.display().to_string();
        self.state.transfer = Some(TransferDialogState::new(operation, entry.path, destination));
        self.state.mode = InputMode::Transfer;
    }

    fn begin_create_directory(&mut self) {
        let destination = self
            .state
            .active_pane()
            .cwd
            .join("new-dir")
            .display()
            .to_string();
        self.state.transfer = Some(TransferDialogState::new(
            TransferOperation::CreateDirectory,
            self.state.active_pane().cwd.clone(),
            destination,
        ));
        self.state.mode = InputMode::Transfer;
    }

    fn begin_delete(&mut self) {
        let Some(entry) = self.state.active_pane().selected_entry().cloned() else {
            self.state.set_status(StatusMessage::info("nothing selected"));
            return;
        };

        self.state.transfer = Some(TransferDialogState::new(
            TransferOperation::Delete,
            entry.path,
            String::new(),
        ));
        self.state.mode = InputMode::Transfer;
    }

    fn open_selection(&mut self) {
        let Some(entry) = self.state.active_pane().selected_entry().cloned() else {
            self.state
                .set_status(StatusMessage::info("nothing to open"));
            return;
        };

        if entry.kind.is_directory() {
            match self.change_active_directory(entry.path.clone()) {
                Ok(()) => self.state.set_status(StatusMessage::info(format!(
                    "active pane: {}",
                    self.state.active_pane().cwd.display()
                ))),
                Err(err) => self.state.set_status(StatusMessage::error(err.to_string())),
            }
        } else {
            self.state.set_status(StatusMessage::info(format!(
                "{} is not a directory",
                entry.display_name()
            )));
        }
    }

    fn edit_command(&mut self, edit: CommandEditAction) {
        edit_text(
            &mut self.state.command.buffer,
            &mut self.state.command.cursor,
            edit,
        );
    }

    fn edit_transfer(&mut self, edit: CommandEditAction) {
        let Some(transfer) = self.state.transfer.as_mut() else {
            return;
        };

        if !transfer.operation.edits_destination()
            || transfer.focus != TransferControl::DestinationField
        {
            return;
        }

        edit_text(&mut transfer.destination, &mut transfer.cursor, edit);
    }

    fn submit_command(&mut self) {
        let command = self.state.command.buffer.clone();
        self.state.command.clear();
        self.state.mode = InputMode::Normal;

        let result = commands::execute(self, &command);
        commands::apply_result(self, result);
    }

    fn submit_transfer(&mut self) {
        let Some(dialog) = self.state.transfer.clone() else {
            return;
        };

        if dialog.focus == TransferControl::CancelButton {
            self.state.transfer = None;
            self.state.mode = InputMode::Normal;
            self.state
                .set_status(StatusMessage::info("file operation cancelled"));
            return;
        }

        let result = match dialog.operation {
            TransferOperation::Copy | TransferOperation::Move | TransferOperation::CreateDirectory => {
                let raw_destination = dialog.destination.trim();
                if raw_destination.is_empty() {
                    self.state
                        .set_status(StatusMessage::error("destination cannot be empty"));
                    return;
                }

                let destination = self.resolve_transfer_destination(raw_destination, &dialog.source);
                let result = match dialog.operation {
                    TransferOperation::Copy => fs::copy_file(&dialog.source, &destination),
                    TransferOperation::Move => fs::move_file(&dialog.source, &destination),
                    TransferOperation::CreateDirectory => fs::create_directory(&destination),
                    TransferOperation::Delete => unreachable!("delete handled separately"),
                };
                (result, destination)
            }
            TransferOperation::Delete => (
                fs::delete_entry(&dialog.source),
                dialog.source.clone(),
            ),
        };

        match result.0 {
            Ok(()) => {
                self.state.mode = InputMode::Normal;
                self.state.transfer = None;
                if let Err(err) = self.refresh_panes() {
                    self.state.set_status(StatusMessage::error(format!(
                        "{} succeeded, but refresh failed: {}",
                        dialog.operation.label(),
                        err
                    )));
                    return;
                }

                self.state.set_status(StatusMessage::info(format!(
                    "{} {}",
                    dialog.operation.past_tense(),
                    success_target(&dialog, &result.1)
                )));
            }
            Err(err) => self.state.set_status(StatusMessage::error(err.to_string())),
        }
    }

    fn resolve_transfer_destination(&self, raw_destination: &str, source: &Path) -> PathBuf {
        let destination = PathBuf::from(raw_destination);
        let resolved = if destination.is_absolute() {
            destination
        } else {
            self.state.active_pane().cwd.join(destination)
        };

        if resolved.is_dir() {
            resolved.join(source.file_name().unwrap_or_else(|| OsStr::new("")))
        } else {
            resolved
        }
    }

    fn refresh_panes(&mut self) -> Result<()> {
        self.state.left.refresh()?;
        self.state.right.refresh()?;
        Ok(())
    }

    fn handle_mouse(&mut self, event: MouseEvent, frame_area: ratatui::layout::Rect) -> Result<()> {
        if self.state.mode != InputMode::Transfer {
            return Ok(());
        }

        match event.kind {
            MouseEventKind::Moved => {
                let hovered =
                    ui::transfer_dialog_hit_target(&self.state, frame_area, event.column, event.row);
                if let Some(transfer) = self.state.transfer.as_mut() {
                    transfer.hovered = hovered;
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if let Some(control) =
                    ui::transfer_dialog_hit_target(&self.state, frame_area, event.column, event.row)
                {
                    if let Some(transfer) = self.state.transfer.as_mut() {
                        transfer.focus = control;
                        transfer.hovered = Some(control);
                    }
                    if control == TransferControl::ConfirmButton {
                        self.handle_action(Action::SubmitTransfer)?;
                    } else if control == TransferControl::CancelButton {
                        self.handle_action(Action::CancelTransfer)?;
                    }
                }
            }
            _ => {}
        }

        Ok(())
    }

    fn move_transfer_focus_up(&mut self) {
        let Some(transfer) = self.state.transfer.as_mut() else {
            return;
        };

        transfer.focus = match transfer.focus {
            TransferControl::SourceField => TransferControl::SourceField,
            TransferControl::DestinationField => {
                if transfer.operation.shows_source() {
                    TransferControl::SourceField
                } else {
                    TransferControl::DestinationField
                }
            }
            TransferControl::ConfirmButton | TransferControl::CancelButton => {
                TransferControl::DestinationField
            }
        };
        transfer.hovered = Some(transfer.focus);
    }

    fn move_transfer_focus_down(&mut self) {
        let Some(transfer) = self.state.transfer.as_mut() else {
            return;
        };

        transfer.focus = match transfer.focus {
            TransferControl::SourceField => TransferControl::DestinationField,
            TransferControl::DestinationField => TransferControl::ConfirmButton,
            TransferControl::ConfirmButton => TransferControl::ConfirmButton,
            TransferControl::CancelButton => TransferControl::CancelButton,
        };
        transfer.hovered = Some(transfer.focus);
    }

    fn move_transfer_focus_left(&mut self) {
        let Some(transfer) = self.state.transfer.as_mut() else {
            return;
        };

        match transfer.focus {
            TransferControl::DestinationField if transfer.operation.edits_destination() => {
                edit_text(
                    &mut transfer.destination,
                    &mut transfer.cursor,
                    CommandEditAction::MoveCursorLeft,
                );
            }
            TransferControl::CancelButton => transfer.focus = TransferControl::ConfirmButton,
            _ => {}
        }
        transfer.hovered = Some(transfer.focus);
    }

    fn move_transfer_focus_right(&mut self) {
        let Some(transfer) = self.state.transfer.as_mut() else {
            return;
        };

        match transfer.focus {
            TransferControl::DestinationField if transfer.operation.edits_destination() => {
                edit_text(
                    &mut transfer.destination,
                    &mut transfer.cursor,
                    CommandEditAction::MoveCursorRight,
                );
            }
            TransferControl::ConfirmButton => transfer.focus = TransferControl::CancelButton,
            _ => {}
        }
        transfer.hovered = Some(transfer.focus);
    }
}

fn resolve_start_dirs(
    left_start_dir: Option<PathBuf>,
    right_start_dir: Option<PathBuf>,
) -> Result<(PathBuf, PathBuf)> {
    let default_dir = resolve_default_start_dir()?;
    let left_dir = match left_start_dir {
        Some(path) => resolve_explicit_start_dir(path)?,
        None => default_dir.clone(),
    };
    let right_dir = match right_start_dir {
        Some(path) => resolve_explicit_start_dir(path)?,
        None => default_dir,
    };

    Ok((left_dir, right_dir))
}

fn resolve_explicit_start_dir(start_dir: PathBuf) -> Result<PathBuf> {
    let resolved = if start_dir.is_absolute() {
        start_dir
    } else {
        std::env::current_dir()
            .context("failed to resolve relative startup directory")?
            .join(start_dir)
    };

    if !resolved.is_dir() {
        anyhow::bail!("not a directory: {}", resolved.display());
    }

    Ok(resolved)
}

fn resolve_default_start_dir() -> Result<PathBuf> {
    match std::env::current_dir() {
        Ok(dir) => Ok(dir),
        Err(current_dir_error) => {
            let base_dirs = BaseDirs::new().context("failed to resolve fallback home directory")?;
            let home = base_dirs.home_dir().to_path_buf();
            if Path::new(&home).exists() {
                Ok(home)
            } else {
                Err(current_dir_error).context("failed to resolve startup directory")
            }
        }
    }
}

fn previous_char_boundary(text: &str, index: usize) -> usize {
    let mut last = 0;
    for (offset, _) in text[..index].char_indices() {
        last = offset;
    }
    last
}

fn next_char_boundary(text: &str, index: usize) -> usize {
    text[index..]
        .char_indices()
        .nth(1)
        .map(|(offset, _)| index + offset)
        .unwrap_or(text.len())
}

fn edit_text(buffer: &mut String, cursor: &mut usize, edit: CommandEditAction) {
    match edit {
        CommandEditAction::Insert(ch) => {
            buffer.insert(*cursor, ch);
            *cursor += ch.len_utf8();
        }
        CommandEditAction::Backspace => {
            if *cursor > 0 {
                let remove_at = previous_char_boundary(buffer, *cursor);
                buffer.drain(remove_at..*cursor);
                *cursor = remove_at;
            }
        }
        CommandEditAction::MoveCursorLeft => {
            if *cursor > 0 {
                *cursor = previous_char_boundary(buffer, *cursor);
            }
        }
        CommandEditAction::MoveCursorRight => {
            if *cursor < buffer.len() {
                *cursor = next_char_boundary(buffer, *cursor);
            }
        }
    }
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| path.display().to_string())
}

fn success_target(dialog: &TransferDialogState, destination: &Path) -> String {
    match dialog.operation {
        TransferOperation::Copy | TransferOperation::Move => {
            format!("{} to {}", display_name(&dialog.source), destination.display())
        }
        TransferOperation::CreateDirectory => destination.display().to_string(),
        TransferOperation::Delete => display_name(&dialog.source),
    }
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;

    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    use tempfile::TempDir;

    use crate::config::Config;
    use crate::input::{Action, CommandEditAction};
    use crate::state::{ActivePane, InputMode, TransferOperation};
    use crate::test_support::cwd_lock;

    use super::App;

    #[test]
    fn startup_uses_current_directory_in_both_panes() {
        let _guard = cwd_lock();
        let temp = TempDir::new().expect("temp dir");
        let previous = env::current_dir().expect("cwd");
        env::set_current_dir(temp.path()).expect("set cwd");

        let app = App::new(Config::default()).expect("app");

        assert_eq!(app.state.left.cwd, temp.path().to_path_buf());
        assert_eq!(app.state.right.cwd, temp.path().to_path_buf());

        env::set_current_dir(previous).expect("restore cwd");
    }

    #[test]
    fn startup_uses_explicit_start_directory_in_both_panes() {
        let _guard = cwd_lock();
        let temp = TempDir::new().expect("temp dir");
        let target = temp.path().join("target");
        fs::create_dir(&target).expect("target dir");
        let previous = env::current_dir().expect("cwd");
        env::set_current_dir(temp.path()).expect("set cwd");

        let app = App::new_with_start_dir(Config::default(), Some(target.clone())).expect("app");

        assert_eq!(app.state.left.cwd, target);
        assert_eq!(app.state.right.cwd, temp.path().to_path_buf());

        env::set_current_dir(previous).expect("restore cwd");
    }

    #[test]
    fn startup_uses_explicit_directories_for_both_panes() {
        let _guard = cwd_lock();
        let temp = TempDir::new().expect("temp dir");
        let left = temp.path().join("left");
        let right = temp.path().join("right");
        fs::create_dir(&left).expect("left dir");
        fs::create_dir(&right).expect("right dir");

        let app = App::new_with_start_dirs(Config::default(), Some(left.clone()), Some(right.clone()))
            .expect("app");

        assert_eq!(app.state.left.cwd, left);
        assert_eq!(app.state.right.cwd, right);
    }

    #[test]
    fn switching_panes_preserves_paths_and_selection() {
        let _guard = cwd_lock();
        let temp = TempDir::new().expect("temp dir");
        fs::create_dir(temp.path().join("left-dir")).expect("dir");
        fs::create_dir(temp.path().join("right-dir")).expect("dir");

        let previous = env::current_dir().expect("cwd");
        env::set_current_dir(temp.path()).expect("set cwd");

        let mut app = App::new(Config::default()).expect("app");
        app.state.left.selected = 1;
        app.handle_action(Action::SwitchPane).expect("switch");
        app.state.right.selected = 0;

        assert_eq!(app.state.active_pane, ActivePane::Right);
        assert_eq!(app.state.left.selected, 1);
        assert_eq!(app.state.right.cwd, temp.path().to_path_buf());

        env::set_current_dir(previous).expect("restore cwd");
    }

    #[test]
    fn opening_file_is_non_fatal() {
        let _guard = cwd_lock();
        let temp = TempDir::new().expect("temp dir");
        fs::write(temp.path().join("note.txt"), b"text").expect("file");

        let previous = env::current_dir().expect("cwd");
        env::set_current_dir(temp.path()).expect("set cwd");

        let mut app = App::new(Config::default()).expect("app");
        app.handle_action(Action::OpenSelection).expect("open");

        assert!(!app.state.should_quit);
        assert!(app.state.status.text.contains("not a directory"));

        env::set_current_dir(previous).expect("restore cwd");
    }

    #[test]
    fn copy_dialog_defaults_destination_to_other_pane_and_copies_file() {
        let _guard = cwd_lock();
        let temp = TempDir::new().expect("temp dir");
        let left_dir = temp.path().join("left");
        let right_dir = temp.path().join("right");
        fs::create_dir(&left_dir).expect("left dir");
        fs::create_dir(&right_dir).expect("right dir");
        fs::write(left_dir.join("report.txt"), b"report").expect("source file");

        let previous = env::current_dir().expect("cwd");
        env::set_current_dir(temp.path()).expect("set cwd");

        let mut app = App::new(Config::default()).expect("app");
        app.state.left.set_cwd(left_dir.clone()).expect("left cwd");
        app.state.right.set_cwd(right_dir.clone()).expect("right cwd");

        app.handle_action(Action::BeginCopy).expect("begin copy");

        let dialog = app.state.transfer.clone().expect("transfer dialog");
        assert_eq!(app.state.mode, InputMode::Transfer);
        assert_eq!(dialog.operation, TransferOperation::Copy);
        assert_eq!(dialog.destination, right_dir.display().to_string());

        app.handle_action(Action::SubmitTransfer).expect("submit transfer");

        assert_eq!(app.state.mode, InputMode::Normal);
        assert!(left_dir.join("report.txt").exists());
        assert!(right_dir.join("report.txt").exists());

        env::set_current_dir(previous).expect("restore cwd");
    }

    #[test]
    fn move_dialog_moves_file_to_entered_destination() {
        let _guard = cwd_lock();
        let temp = TempDir::new().expect("temp dir");
        let left_dir = temp.path().join("left");
        let right_dir = temp.path().join("right");
        fs::create_dir(&left_dir).expect("left dir");
        fs::create_dir(&right_dir).expect("right dir");
        fs::write(left_dir.join("report.txt"), b"report").expect("source file");

        let previous = env::current_dir().expect("cwd");
        env::set_current_dir(temp.path()).expect("set cwd");

        let mut app = App::new(Config::default()).expect("app");
        app.state.left.set_cwd(left_dir.clone()).expect("left cwd");
        app.state.right.set_cwd(right_dir.clone()).expect("right cwd");

        app.handle_action(Action::BeginMove).expect("begin move");

        let destination_len = app
            .state
            .transfer
            .as_ref()
            .expect("transfer dialog")
            .destination
            .len();
        for _ in 0..destination_len {
            app.handle_action(Action::EditTransfer(CommandEditAction::Backspace))
                .expect("clear destination");
        }
        for ch in right_dir.join("renamed.txt").display().to_string().chars() {
            app.handle_action(Action::EditTransfer(CommandEditAction::Insert(ch)))
                .expect("type destination");
        }

        app.handle_action(Action::SubmitTransfer).expect("submit transfer");

        assert_eq!(app.state.mode, InputMode::Normal);
        assert!(!left_dir.join("report.txt").exists());
        assert!(right_dir.join("renamed.txt").exists());

        env::set_current_dir(previous).expect("restore cwd");
    }

    #[test]
    fn create_directory_dialog_creates_directory_in_active_pane() {
        let _guard = cwd_lock();
        let temp = TempDir::new().expect("temp dir");
        let left_dir = temp.path().join("left");
        fs::create_dir(&left_dir).expect("left dir");

        let previous = env::current_dir().expect("cwd");
        env::set_current_dir(temp.path()).expect("set cwd");

        let mut app = App::new(Config::default()).expect("app");
        app.state.left.set_cwd(left_dir.clone()).expect("left cwd");

        app.handle_action(Action::BeginCreateDirectory)
            .expect("begin create directory");

        let dialog = app.state.transfer.clone().expect("transfer dialog");
        assert_eq!(dialog.operation, TransferOperation::CreateDirectory);
        assert_eq!(
            dialog.destination,
            left_dir.join("new-dir").display().to_string()
        );

        app.handle_action(Action::SubmitTransfer).expect("submit mkdir");

        assert_eq!(app.state.mode, InputMode::Normal);
        assert!(left_dir.join("new-dir").is_dir());

        env::set_current_dir(previous).expect("restore cwd");
    }

    #[test]
    fn delete_dialog_removes_selected_entry() {
        let _guard = cwd_lock();
        let temp = TempDir::new().expect("temp dir");
        let left_dir = temp.path().join("left");
        fs::create_dir(&left_dir).expect("left dir");
        fs::write(left_dir.join("victim.txt"), b"bye").expect("victim");

        let previous = env::current_dir().expect("cwd");
        env::set_current_dir(temp.path()).expect("set cwd");

        let mut app = App::new(Config::default()).expect("app");
        app.state.left.set_cwd(left_dir.clone()).expect("left cwd");

        app.handle_action(Action::BeginDelete).expect("begin delete");

        let dialog = app.state.transfer.clone().expect("transfer dialog");
        assert_eq!(dialog.operation, TransferOperation::Delete);
        assert_eq!(dialog.source, left_dir.join("victim.txt"));

        app.handle_action(Action::SubmitTransfer).expect("submit delete");

        assert_eq!(app.state.mode, InputMode::Normal);
        assert!(!left_dir.join("victim.txt").exists());

        env::set_current_dir(previous).expect("restore cwd");
    }

    #[test]
    fn clicking_transfer_cancel_button_closes_dialog() {
        let _guard = cwd_lock();
        let temp = TempDir::new().expect("temp dir");
        let left_dir = temp.path().join("left");
        let right_dir = temp.path().join("right");
        fs::create_dir(&left_dir).expect("left dir");
        fs::create_dir(&right_dir).expect("right dir");
        fs::write(left_dir.join("report.txt"), b"report").expect("source file");

        let previous = env::current_dir().expect("cwd");
        env::set_current_dir(temp.path()).expect("set cwd");

        let mut app = App::new(Config::default()).expect("app");
        app.state.left.set_cwd(left_dir).expect("left cwd");
        app.state.right.set_cwd(right_dir).expect("right cwd");
        app.handle_action(Action::BeginCopy).expect("begin copy");

        app.handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: 36,
                row: 9,
                modifiers: crossterm::event::KeyModifiers::NONE,
            },
            ratatui::layout::Rect::new(0, 0, 60, 12),
        )
        .expect("click cancel");

        assert_eq!(app.state.mode, InputMode::Normal);
        assert!(app.state.transfer.is_none());

        env::set_current_dir(previous).expect("restore cwd");
    }

    #[test]
    fn arrow_keys_can_focus_cancel_button_and_enter_cancels() {
        let _guard = cwd_lock();
        let temp = TempDir::new().expect("temp dir");
        let left_dir = temp.path().join("left");
        let right_dir = temp.path().join("right");
        fs::create_dir(&left_dir).expect("left dir");
        fs::create_dir(&right_dir).expect("right dir");
        fs::write(left_dir.join("report.txt"), b"report").expect("source file");

        let previous = env::current_dir().expect("cwd");
        env::set_current_dir(temp.path()).expect("set cwd");

        let mut app = App::new(Config::default()).expect("app");
        app.state.left.set_cwd(left_dir).expect("left cwd");
        app.state.right.set_cwd(right_dir).expect("right cwd");
        app.handle_action(Action::BeginCopy).expect("begin copy");

        app.handle_action(Action::TransferFocusDown)
            .expect("focus confirm");
        app.handle_action(Action::TransferFocusRight)
            .expect("focus cancel");
        app.handle_action(Action::SubmitTransfer)
            .expect("activate cancel");

        assert_eq!(app.state.mode, InputMode::Normal);
        assert!(app.state.transfer.is_none());

        env::set_current_dir(previous).expect("restore cwd");
    }
}
