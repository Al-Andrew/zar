use std::path::PathBuf;

use crate::app::App;
use crate::state::{ActivePane, StatusMessage};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandResult {
    Success(String),
    Error(String),
    QuitRequested,
}

pub fn execute(app: &mut App, input: &str) -> CommandResult {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return CommandResult::Success("command cancelled".to_string());
    }

    let mut parts = trimmed.split_whitespace();
    let Some(command) = parts.next() else {
        return CommandResult::Success("command cancelled".to_string());
    };

    match command {
        "cd" => {
            let Some(path) = parts.next() else {
                return CommandResult::Error("cd requires a path".to_string());
            };

            let target = PathBuf::from(path);
            let current = app.state.active_pane().cwd.clone();
            let resolved = if target.is_absolute() {
                target
            } else {
                current.join(target)
            };

            match app.change_active_directory(resolved) {
                Ok(()) => CommandResult::Success(format!(
                    "active pane: {}",
                    app.state.active_pane().cwd.display()
                )),
                Err(err) => CommandResult::Error(err.to_string()),
            }
        }
        "pane" => match parts.next() {
            Some("left") => {
                app.state.active_pane = ActivePane::Left;
                CommandResult::Success("active pane: left".to_string())
            }
            Some("right") => {
                app.state.active_pane = ActivePane::Right;
                CommandResult::Success("active pane: right".to_string())
            }
            _ => CommandResult::Error("usage: pane <left|right>".to_string()),
        },
        "pwd" => CommandResult::Success(app.state.active_pane().cwd.display().to_string()),
        "quit" => CommandResult::QuitRequested,
        _ => CommandResult::Error(format!("unknown command: {command}")),
    }
}

pub fn apply_result(app: &mut App, result: CommandResult) {
    match result {
        CommandResult::Success(message) => app.state.set_status(StatusMessage::info(message)),
        CommandResult::Error(message) => app.state.set_status(StatusMessage::error(message)),
        CommandResult::QuitRequested => app.state.should_quit = true,
    }
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;

    use tempfile::TempDir;

    use crate::app::App;
    use crate::config::Config;
    use crate::state::ActivePane;
    use crate::test_support::cwd_lock;

    use super::{CommandResult, execute};

    #[test]
    fn command_parser_covers_mvp_commands() {
        let _guard = cwd_lock();
        let temp = TempDir::new().expect("temp dir");
        fs::create_dir(temp.path().join("child")).expect("child dir");

        let previous = env::current_dir().expect("cwd");
        env::set_current_dir(temp.path()).expect("set cwd");

        let mut app = App::new(Config::default()).expect("app");

        assert_eq!(
            execute(&mut app, "pane right"),
            CommandResult::Success("active pane: right".into())
        );
        assert_eq!(app.state.active_pane, ActivePane::Right);

        assert_eq!(
            execute(&mut app, "pwd"),
            CommandResult::Success(temp.path().display().to_string())
        );

        let cd_result = execute(&mut app, "cd child");
        assert!(matches!(cd_result, CommandResult::Success(_)));
        assert_eq!(app.state.active_pane().cwd, temp.path().join("child"));

        assert_eq!(execute(&mut app, "quit"), CommandResult::QuitRequested);
        assert!(matches!(
            execute(&mut app, "bogus"),
            CommandResult::Error(message) if message == "unknown command: bogus"
        ));

        env::set_current_dir(previous).expect("restore cwd");
    }
}
