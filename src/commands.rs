use crate::app::App;
use crate::source::LocationPath;
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

            let pane = app.state.active_pane();
            let resolved = LocationPath::from_input(pane.source.kind, &pane.cwd, path);

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
        "pwd" => CommandResult::Success(app.state.active_pane().cwd.display()),
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
    use std::fs;
    use std::path::PathBuf;

    use tempfile::TempDir;

    use crate::app::App;
    use crate::commands::{CommandResult, execute};
    use crate::config::{Config, SshAuthMethod};
    use crate::source::{LocationPath, SourceKind, SourceRef};
    use crate::state::ActivePane;
    use crate::test_support::cwd_lock;
    use crate::vfs::MockSessionFactory;

    #[test]
    fn command_parser_covers_source_aware_commands() {
        let _guard = cwd_lock();
        let temp = TempDir::new().expect("temp dir");
        fs::create_dir(temp.path().join("child")).expect("child dir");

        let mut app = App::new_with_factory(
            Config::default(),
            Some(temp.path().to_path_buf()),
            None,
            Box::new(MockSessionFactory::default()),
        )
        .expect("app");

        assert_eq!(
            execute(&mut app, "pwd"),
            CommandResult::Success(temp.path().display().to_string())
        );

        let cd_result = execute(&mut app, "cd child");
        assert!(matches!(cd_result, CommandResult::Success(_)));
        assert_eq!(
            app.state.active_pane().cwd,
            LocationPath::Local(temp.path().join("child"))
        );
    }

    #[test]
    fn pwd_works_on_mock_remote_sources() {
        let _guard = cwd_lock();
        let temp = TempDir::new().expect("temp dir");
        fs::create_dir_all(temp.path().join("var/www")).expect("dirs");

        let mut config = Config::default();
        config.sources.ssh.insert(
            "prod".to_string(),
            crate::config::SshSourceProfile {
                label: "Prod".to_string(),
                host: "prod.example.com".to_string(),
                port: 22,
                username: "deploy".to_string(),
                initial_path: "/".to_string(),
                auth: SshAuthMethod::Password,
                key_path: None,
            },
        );

        let factory = MockSessionFactory::default();
        factory.add_remote(
            SourceRef::SavedSsh {
                id: "prod".to_string(),
            },
            SourceKind::Ssh,
            "Prod",
            temp.path().to_path_buf(),
        );

        let mut app = App::new_with_factory(
            config,
            Some(temp.path().to_path_buf()),
            None,
            Box::new(factory),
        )
        .expect("app");
        app.switch_pane_source(
            ActivePane::Left,
            SourceRef::SavedSsh {
                id: "prod".to_string(),
            },
            Some(LocationPath::Remote("/var/www".to_string())),
        )
        .expect("switch source");

        assert_eq!(
            execute(&mut app, "pwd"),
            CommandResult::Success("/var/www".to_string())
        );
        assert_eq!(PathBuf::from("ignored").display().to_string(), "ignored");
    }
}
