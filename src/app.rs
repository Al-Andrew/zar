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
use crate::input::{Action, CommandEditAction, event_to_action};
use crate::state::{ActivePane, AppState, InputMode, StatusMessage};
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
            Action::EnterCommandMode => {
                self.state.mode = InputMode::Command;
                self.state.command.clear();
                self.state
                    .set_status(StatusMessage::info("command mode: enter a command"));
            }
            Action::EditCommand(edit) => self.edit_command(edit),
            Action::SubmitCommand => self.submit_command(),
            Action::CancelCommand => {
                self.state.command.clear();
                self.state.mode = InputMode::Normal;
                self.state
                    .set_status(StatusMessage::info("command cancelled"));
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
        match edit {
            CommandEditAction::Insert(ch) => {
                self.state
                    .command
                    .buffer
                    .insert(self.state.command.cursor, ch);
                self.state.command.cursor += ch.len_utf8();
            }
            CommandEditAction::Backspace => {
                if self.state.command.cursor > 0 {
                    let remove_at = previous_char_boundary(
                        &self.state.command.buffer,
                        self.state.command.cursor,
                    );
                    self.state
                        .command
                        .buffer
                        .drain(remove_at..self.state.command.cursor);
                    self.state.command.cursor = remove_at;
                }
            }
            CommandEditAction::MoveCursorLeft => {
                if self.state.command.cursor > 0 {
                    self.state.command.cursor = previous_char_boundary(
                        &self.state.command.buffer,
                        self.state.command.cursor,
                    );
                }
            }
            CommandEditAction::MoveCursorRight => {
                if self.state.command.cursor < self.state.command.buffer.len() {
                    self.state.command.cursor =
                        next_char_boundary(&self.state.command.buffer, self.state.command.cursor);
                }
            }
        }
    }

    fn submit_command(&mut self) {
        let command = self.state.command.buffer.clone();
        self.state.command.clear();
        self.state.mode = InputMode::Normal;

        let result = commands::execute(self, &command);
        commands::apply_result(self, result);
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

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;

    use tempfile::TempDir;

    use crate::config::Config;
    use crate::input::Action;
    use crate::state::ActivePane;
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
}
