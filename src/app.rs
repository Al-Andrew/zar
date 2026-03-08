use std::ffi::OsStr;
use std::io::Stdout;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::event;
use directories::BaseDirs;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::commands;
use crate::config::Config;
use crate::fs::{self, EntryKind};
use crate::input::{Action, CommandEditAction, event_to_action};
use crate::state::{
    ActivePane, AppState, InputMode, StatusMessage, TransferDialogState, TransferOperation,
};
use crate::ui;

pub struct App {
    pub state: AppState,
}

impl App {
    pub fn new(config: Config) -> Result<Self> {
        let cwd = resolve_start_dir()?;
        let state = AppState::new(config, cwd)?;
        Ok(Self { state })
    }

    pub fn run(&mut self, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
        while !self.state.should_quit {
            terminal.draw(|frame| ui::render(frame, &mut self.state))?;

            if event::poll(Duration::from_millis(250))? {
                let event = event::read()?;
                if let Some(action) =
                    event_to_action(&self.state.config.key_bindings, self.state.mode, event)
                {
                    self.handle_action(action)?;
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
            Action::EnterCommandMode => {
                self.state.mode = InputMode::Command;
                self.state.command.clear();
                self.state
                    .set_status(StatusMessage::info("command mode: enter a command"));
            }
            Action::EditCommand(edit) => self.edit_command(edit),
            Action::EditTransfer(edit) => self.edit_transfer(edit),
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
        };

        match result {
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
                    success_target(&dialog, &destination)
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
}

fn resolve_start_dir() -> Result<PathBuf> {
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
    }
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;

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
}
