# zar

A cross-platform terminal file manager prototype in Rust.

## Current MVP

- Two symmetric directory panes
- One active pane at a time
- Keyboard navigation with arrows, `Tab`, `Enter`, `Backspace`, `F3`, `F5`, `F6`, `F7`, `F8`, `/`, and `q`
- Bottom command mode for internal commands: `cd`, `pane left`, `pane right`, `pwd`, `quit`
- Copy and move dialogs with destination prefilled from the other pane
- Text preview mode on `F3`
- Create-directory dialog on `F7`
- Delete confirmation dialog on `F8`
- Configurable single-key bindings from a TOML config file

## Run

```bash
cargo run
```

Open at a specific directory:

```bash
cargo run -- /home/aaldea
```

Open left and right panes at different directories:

```bash
cargo run -- /home/aaldea /tmp
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
