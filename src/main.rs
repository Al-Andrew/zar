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

use anyhow::Result;
use crossterm::ExecutableCommand;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::app::App;
use crate::config::Config;

fn main() -> Result<()> {
    let config = Config::load()?;
    for warning in &config.startup_warnings {
        eprintln!("config warning: {warning}");
    }

    install_panic_hook();

    let mut terminal = setup_terminal()?;
    let _guard = TerminalGuard;

    let mut app = App::new(config)?;
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
