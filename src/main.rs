mod app;
mod commands;
mod config;
mod fs;
mod input;
mod state;
#[cfg(test)]
mod test_support;
mod ui;

use std::io::{self, Stdout};
use std::panic;
use std::path::PathBuf;

use anyhow::{Result, bail};
use crossterm::ExecutableCommand;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::app::App;
use crate::config::Config;

fn main() -> Result<()> {
    let (left_start_dir, right_start_dir) = parse_start_dir_args(std::env::args_os())?;
    let config = Config::load()?;
    for warning in &config.startup_warnings {
        eprintln!("config warning: {warning}");
    }

    install_panic_hook();

    let mut terminal = setup_terminal()?;
    let _guard = TerminalGuard;

    let mut app = App::new_with_start_dirs(config, left_start_dir, right_start_dir)?;
    let result = app.run(&mut terminal);

    match result {
        Ok(()) => Ok(()),
        Err(err) => Err(err),
    }
}

type AppTerminal = Terminal<CrosstermBackend<Stdout>>;

fn setup_terminal() -> Result<AppTerminal> {
    enable_raw_mode()?;

    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = stdout.execute(LeaveAlternateScreen);
    }
}

fn install_panic_hook() {
    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = stdout.execute(LeaveAlternateScreen);
        previous(panic_info);
    }));
}

fn parse_start_dir_args<I, S>(args: I) -> Result<(Option<PathBuf>, Option<PathBuf>)>
where
    I: IntoIterator<Item = S>,
    S: Into<std::ffi::OsString>,
{
    let mut args = args.into_iter().map(Into::into);
    let _program = args.next();
    let left_start_dir = args.next().map(PathBuf::from);
    let right_start_dir = args.next().map(PathBuf::from);

    if args.next().is_some() {
        bail!("usage: zar [LEFT_START_DIR] [RIGHT_START_DIR]");
    }

    Ok((left_start_dir, right_start_dir))
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;

    use super::parse_start_dir_args;

    #[test]
    fn parse_start_dir_accepts_single_positional_path() {
        let start_dirs = parse_start_dir_args([
            OsString::from("zar"),
            OsString::from("/home/aaldea"),
        ])
        .expect("parse");

        assert_eq!(start_dirs, (Some(PathBuf::from("/home/aaldea")), None));
    }

    #[test]
    fn parse_start_dir_accepts_two_positional_paths() {
        let start_dirs = parse_start_dir_args([
            OsString::from("zar"),
            OsString::from("/tmp/one"),
            OsString::from("/tmp/two"),
        ])
        .expect("parse");

        assert_eq!(
            start_dirs,
            (
                Some(PathBuf::from("/tmp/one")),
                Some(PathBuf::from("/tmp/two"))
            )
        );
    }

    #[test]
    fn parse_start_dir_rejects_more_than_two_positionals() {
        let result = parse_start_dir_args([
            OsString::from("zar"),
            OsString::from("/tmp/one"),
            OsString::from("/tmp/two"),
            OsString::from("/tmp/three"),
        ]);

        assert!(result.is_err());
    }
}
