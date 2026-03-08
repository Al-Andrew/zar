# zar

A cross-platform terminal file manager prototype in Rust.

## Current MVP

- Two symmetric directory panes
- One active pane at a time
- Keyboard navigation with arrows, `Tab`, `Enter`, `Backspace`, `F5`, `F6`, `F7`, `/`, and `q`
- Bottom command mode for internal commands: `cd`, `pane left`, `pane right`, `pwd`, `quit`
- Copy and move dialogs with destination prefilled from the other pane
- Create-directory dialog on `F7`
- Configurable single-key bindings from a TOML config file

## Run

```bash
cargo run
```

## Config

Configuration is loaded from:

- Linux: `~/.config/zar/config.toml`
- Windows: `%APPDATA%\zar\config.toml`

Example:

```toml
[keys]
enter_command_mode = "/"
quit = "q"
switch_pane = "tab"
move_up = "up"
move_down = "down"
open = "enter"
parent = "backspace"
```

Only single-key bindings are supported in the current MVP.
