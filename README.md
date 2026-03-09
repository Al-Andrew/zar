# zar

A cross-platform terminal file manager prototype in Rust.

## Current MVP

- Two symmetric directory panes
- One active pane at a time
- Keyboard navigation with arrows, `Tab`, `Enter`, `Backspace`, `F1`, `F2`, `F3`, `F4`, `F5`, `F6`, `F7`, `F8`, `/`, and `q`
- Bottom command mode for internal commands: `cd`, `pane left`, `pane right`, `pwd`, `quit`
- Switchable pane sources for local paths plus saved FTP / SSH profiles
- Add-location dialog on `F4` for saved local, FTP, SMB, and SSH locations
- Copy and move dialogs with destination prefilled from the other pane and source-aware destination resolution
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

[sources.local.home]
label = "Home"
path = "/home/aaldea"

[sources.ftp.archive]
label = "Archive"
host = "ftp.example.com"
username = "alice"
initial_path = "/incoming"

[sources.ssh.prod]
label = "Prod"
host = "prod.example.com"
username = "deploy"
auth = "password"
initial_path = "/var/www"
```

Secrets are loaded from `secrets.toml` in the same config directory:

```toml
[secrets.ftp.archive]
password = "ftp-password"

[secrets.ssh.prod]
password = "ssh-password"
```

Only single-key bindings are supported in the current MVP.

`history.toml` is also stored in the same config directory and is updated automatically when pane sources or directories change.

`F4` opens a dialog that writes new saved locations into `config.toml` and `secrets.toml`. Target formats:

- Local: absolute or relative directory path
- FTP: `ftp://user@host[:port]/path`
- SMB: `smb://user@server/share/path?workgroup=WORK`
- SSH: `ssh://user@host[:port]/path`

For FTP, SMB, and SSH, enter the password in the dialog's `Secret` field or embed it in the URI. SSH locations created from the dialog are saved as password-auth profiles.

Saved SMB profiles can be created and browsed through the mock/test backend, but live SMB connections remain unavailable in this build until the native Samba dependency issue is resolved.
