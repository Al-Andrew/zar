use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigurableKey {
    Char(char),
    Up,
    Down,
    Enter,
    Backspace,
    Tab,
    Esc,
    Left,
    Right,
}

impl ConfigurableKey {
    pub fn parse(value: &str) -> Option<Self> {
        let normalized = value.trim().to_lowercase();
        match normalized.as_str() {
            "up" => Some(Self::Up),
            "down" => Some(Self::Down),
            "enter" => Some(Self::Enter),
            "backspace" => Some(Self::Backspace),
            "tab" => Some(Self::Tab),
            "esc" | "escape" => Some(Self::Esc),
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            _ => {
                let mut chars = value.chars();
                let first = chars.next()?;
                if chars.next().is_none() {
                    Some(Self::Char(first))
                } else {
                    None
                }
            }
        }
    }

    pub fn matches(&self, event: KeyEvent) -> bool {
        let modifiers = event.modifiers;
        match self {
            Self::Char(expected) => {
                modifiers
                    .intersection(KeyModifiers::CONTROL | KeyModifiers::ALT)
                    .is_empty()
                    && matches!(event.code, KeyCode::Char(actual) if actual == *expected)
            }
            Self::Up => modifiers.is_empty() && event.code == KeyCode::Up,
            Self::Down => modifiers.is_empty() && event.code == KeyCode::Down,
            Self::Enter => modifiers.is_empty() && event.code == KeyCode::Enter,
            Self::Backspace => modifiers.is_empty() && event.code == KeyCode::Backspace,
            Self::Tab => modifiers.is_empty() && event.code == KeyCode::Tab,
            Self::Esc => modifiers.is_empty() && event.code == KeyCode::Esc,
            Self::Left => modifiers.is_empty() && event.code == KeyCode::Left,
            Self::Right => modifiers.is_empty() && event.code == KeyCode::Right,
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::Char(ch) => ch.to_string(),
            Self::Up => "up".to_string(),
            Self::Down => "down".to_string(),
            Self::Enter => "enter".to_string(),
            Self::Backspace => "backspace".to_string(),
            Self::Tab => "tab".to_string(),
            Self::Esc => "esc".to_string(),
            Self::Left => "left".to_string(),
            Self::Right => "right".to_string(),
        }
    }

    pub fn as_config_value(&self) -> String {
        self.label()
    }
}

#[derive(Debug, Clone)]
pub struct KeyBindings {
    pub enter_command_mode: ConfigurableKey,
    pub quit: ConfigurableKey,
    pub switch_pane: ConfigurableKey,
    pub move_up: ConfigurableKey,
    pub move_down: ConfigurableKey,
    pub open: ConfigurableKey,
    pub parent: ConfigurableKey,
}

impl Default for KeyBindings {
    fn default() -> Self {
        Self {
            enter_command_mode: ConfigurableKey::Char('/'),
            quit: ConfigurableKey::Char('q'),
            switch_pane: ConfigurableKey::Tab,
            move_up: ConfigurableKey::Up,
            move_down: ConfigurableKey::Down,
            open: ConfigurableKey::Enter,
            parent: ConfigurableKey::Backspace,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub key_bindings: KeyBindings,
    pub startup_warnings: Vec<String>,
    pub sources: SavedSources,
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = config_dir()?.join("config.toml");
        Self::load_from_path(&path)
    }

    pub fn load_from_dir(dir: &Path) -> Result<Self> {
        Self::load_from_path(&dir.join("config.toml"))
    }

    pub fn load_from_path(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let contents = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        Self::from_toml_str(&contents)
    }

    fn from_toml_str(contents: &str) -> Result<Self> {
        let file: ConfigFile = toml::from_str(contents).context("failed to parse config.toml")?;
        Ok(Self::from_file(file))
    }

    fn from_file(file: ConfigFile) -> Self {
        let defaults = KeyBindings::default();
        let mut warnings = Vec::new();

        let mut bindings = KeyBindings {
            enter_command_mode: parse_override(
                file.keys
                    .as_ref()
                    .and_then(|keys| keys.enter_command_mode.as_deref()),
                defaults.enter_command_mode.clone(),
                "keys.enter_command_mode",
                &mut warnings,
            ),
            quit: parse_override(
                file.keys.as_ref().and_then(|keys| keys.quit.as_deref()),
                defaults.quit.clone(),
                "keys.quit",
                &mut warnings,
            ),
            switch_pane: parse_override(
                file.keys
                    .as_ref()
                    .and_then(|keys| keys.switch_pane.as_deref()),
                defaults.switch_pane.clone(),
                "keys.switch_pane",
                &mut warnings,
            ),
            move_up: parse_override(
                file.keys.as_ref().and_then(|keys| keys.move_up.as_deref()),
                defaults.move_up.clone(),
                "keys.move_up",
                &mut warnings,
            ),
            move_down: parse_override(
                file.keys
                    .as_ref()
                    .and_then(|keys| keys.move_down.as_deref()),
                defaults.move_down.clone(),
                "keys.move_down",
                &mut warnings,
            ),
            open: parse_override(
                file.keys.as_ref().and_then(|keys| keys.open.as_deref()),
                defaults.open.clone(),
                "keys.open",
                &mut warnings,
            ),
            parent: parse_override(
                file.keys.as_ref().and_then(|keys| keys.parent.as_deref()),
                defaults.parent.clone(),
                "keys.parent",
                &mut warnings,
            ),
        };

        if bindings.enter_command_mode == bindings.quit {
            warnings.push(
                "command trigger conflicts with quit binding; command trigger takes precedence"
                    .to_string(),
            );
            bindings.quit = defaults.quit;
        }
        if bindings.enter_command_mode == bindings.switch_pane {
            warnings.push(
                "command trigger conflicts with switch pane binding; command trigger takes precedence"
                    .to_string(),
            );
            bindings.switch_pane = defaults.switch_pane;
        }
        if bindings.enter_command_mode == bindings.move_up {
            warnings.push(
                "command trigger conflicts with move up binding; command trigger takes precedence"
                    .to_string(),
            );
            bindings.move_up = defaults.move_up;
        }
        if bindings.enter_command_mode == bindings.move_down {
            warnings.push(
                "command trigger conflicts with move down binding; command trigger takes precedence"
                    .to_string(),
            );
            bindings.move_down = defaults.move_down;
        }
        if bindings.enter_command_mode == bindings.open {
            warnings.push(
                "command trigger conflicts with open binding; command trigger takes precedence"
                    .to_string(),
            );
            bindings.open = defaults.open;
        }
        if bindings.enter_command_mode == bindings.parent {
            warnings.push(
                "command trigger conflicts with parent binding; command trigger takes precedence"
                    .to_string(),
            );
            bindings.parent = defaults.parent;
        }

        Self {
            key_bindings: bindings,
            startup_warnings: warnings,
            sources: SavedSources::from(file.sources.unwrap_or_default()),
        }
    }

    pub fn save_to_dir(&self, dir: &Path) -> Result<()> {
        fs::create_dir_all(dir).with_context(|| format!("failed to create {}", dir.display()))?;
        let path = dir.join("config.toml");
        let body = toml::to_string_pretty(&ConfigFileOut::from(self))
            .context("failed to serialize config.toml")?;
        fs::write(&path, body).with_context(|| format!("failed to write {}", path.display()))
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            key_bindings: KeyBindings::default(),
            startup_warnings: Vec::new(),
            sources: SavedSources::default(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SavedSources {
    pub local: BTreeMap<String, LocalSourceProfile>,
    pub ftp: BTreeMap<String, FtpSourceProfile>,
    pub smb: BTreeMap<String, SmbSourceProfile>,
    pub ssh: BTreeMap<String, SshSourceProfile>,
}

impl From<SourcesConfig> for SavedSources {
    fn from(value: SourcesConfig) -> Self {
        Self {
            local: value.local,
            ftp: value.ftp,
            smb: value.smb,
            ssh: value.ssh,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct LocalSourceProfile {
    pub label: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct FtpSourceProfile {
    pub label: String,
    pub host: String,
    #[serde(default = "default_ftp_port")]
    pub port: u16,
    pub username: String,
    #[serde(default = "default_remote_root")]
    pub initial_path: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct SmbSourceProfile {
    pub label: String,
    pub server: String,
    pub share: String,
    pub username: String,
    #[serde(default)]
    pub workgroup: Option<String>,
    #[serde(default = "default_remote_root")]
    pub initial_path: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct SshSourceProfile {
    pub label: String,
    pub host: String,
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    pub username: String,
    #[serde(default = "default_remote_root")]
    pub initial_path: String,
    pub auth: SshAuthMethod,
    #[serde(default)]
    pub key_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SshAuthMethod {
    Password,
    KeyFile,
}

fn default_ftp_port() -> u16 {
    21
}

fn default_ssh_port() -> u16 {
    22
}

fn default_remote_root() -> String {
    "/".to_string()
}

fn parse_override(
    value: Option<&str>,
    default: ConfigurableKey,
    field: &str,
    warnings: &mut Vec<String>,
) -> ConfigurableKey {
    match value {
        Some(raw) => match ConfigurableKey::parse(raw) {
            Some(key) => key,
            None => {
                warnings.push(format!(
                    "invalid key binding for {field}: {raw:?}; using default {}",
                    default.label()
                ));
                default
            }
        },
        None => default,
    }
}

#[derive(Debug, Serialize)]
struct ConfigFileOut {
    keys: KeysConfigOut,
    sources: SourcesConfigOut,
}

impl From<&Config> for ConfigFileOut {
    fn from(config: &Config) -> Self {
        Self {
            keys: KeysConfigOut {
                enter_command_mode: config.key_bindings.enter_command_mode.as_config_value(),
                quit: config.key_bindings.quit.as_config_value(),
                switch_pane: config.key_bindings.switch_pane.as_config_value(),
                move_up: config.key_bindings.move_up.as_config_value(),
                move_down: config.key_bindings.move_down.as_config_value(),
                open: config.key_bindings.open.as_config_value(),
                parent: config.key_bindings.parent.as_config_value(),
            },
            sources: SourcesConfigOut {
                local: config.sources.local.clone(),
                ftp: config.sources.ftp.clone(),
                smb: config.sources.smb.clone(),
                ssh: config.sources.ssh.clone(),
            },
        }
    }
}

#[derive(Debug, Serialize)]
struct KeysConfigOut {
    enter_command_mode: String,
    quit: String,
    switch_pane: String,
    move_up: String,
    move_down: String,
    open: String,
    parent: String,
}

#[derive(Debug, Serialize)]
struct SourcesConfigOut {
    local: BTreeMap<String, LocalSourceProfile>,
    ftp: BTreeMap<String, FtpSourceProfile>,
    smb: BTreeMap<String, SmbSourceProfile>,
    ssh: BTreeMap<String, SshSourceProfile>,
}

pub fn config_dir() -> Result<PathBuf> {
    let project_dirs =
        ProjectDirs::from("", "", "zar").context("failed to resolve config directory")?;
    Ok(project_dirs.config_dir().to_path_buf())
}

#[derive(Debug, Deserialize)]
struct ConfigFile {
    #[serde(default)]
    keys: Option<KeysConfig>,
    #[serde(default)]
    sources: Option<SourcesConfig>,
}

#[derive(Debug, Deserialize, Default)]
struct KeysConfig {
    enter_command_mode: Option<String>,
    quit: Option<String>,
    switch_pane: Option<String>,
    move_up: Option<String>,
    move_down: Option<String>,
    open: Option<String>,
    parent: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct SourcesConfig {
    #[serde(default)]
    local: BTreeMap<String, LocalSourceProfile>,
    #[serde(default)]
    ftp: BTreeMap<String, FtpSourceProfile>,
    #[serde(default)]
    smb: BTreeMap<String, SmbSourceProfile>,
    #[serde(default)]
    ssh: BTreeMap<String, SshSourceProfile>,
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use tempfile::TempDir;

    use super::{Config, ConfigurableKey, FtpSourceProfile, LocalSourceProfile, SshAuthMethod};

    #[test]
    fn missing_config_uses_defaults() {
        let temp = TempDir::new().expect("temp dir");
        let config = Config::load_from_path(&temp.path().join("missing.toml")).expect("config");

        assert_eq!(
            config.key_bindings.enter_command_mode,
            ConfigurableKey::Char('/')
        );
        assert!(config.sources.ftp.is_empty());
    }

    #[test]
    fn valid_config_overrides_command_trigger() {
        let temp = TempDir::new().expect("temp dir");
        let path = temp.path().join("config.toml");
        fs::write(
            &path,
            r#"
[keys]
enter_command_mode = ":"
"#,
        )
        .expect("write config");

        let config = Config::load_from_path(&path).expect("config");
        assert_eq!(
            config.key_bindings.enter_command_mode,
            ConfigurableKey::Char(':')
        );
    }

    #[test]
    fn invalid_config_key_falls_back_to_default() {
        let temp = TempDir::new().expect("temp dir");
        let path = temp.path().join("config.toml");
        fs::write(
            &path,
            r#"
[keys]
enter_command_mode = "spacebar"
"#,
        )
        .expect("write config");

        let config = Config::load_from_path(&path).expect("config");
        assert_eq!(
            config.key_bindings.enter_command_mode,
            ConfigurableKey::Char('/')
        );
        assert!(!config.startup_warnings.is_empty());
    }

    #[test]
    fn parses_saved_source_profiles() {
        let temp = TempDir::new().expect("temp dir");
        let path = temp.path().join("config.toml");
        fs::write(
            &path,
            r#"
[sources.local.home]
label = "Home"
path = "/home/test"

[sources.ftp.archive]
label = "Archive"
host = "ftp.example.com"
username = "alice"
initial_path = "/incoming"

[sources.smb.media]
label = "Media"
server = "smb://nas"
share = "/tv"
username = "alice"
workgroup = "WORK"
initial_path = "/shows"

[sources.ssh.prod]
label = "Prod"
host = "prod.example.com"
username = "deploy"
auth = "key_file"
key_path = "/home/test/.ssh/id_ed25519"
initial_path = "/var/www"
"#,
        )
        .expect("write config");

        let config = Config::load_from_path(&path).expect("config");
        assert_eq!(
            config.sources.local["home"].path,
            PathBuf::from("/home/test")
        );
        assert_eq!(config.sources.ftp["archive"].port, 21);
        assert_eq!(config.sources.smb["media"].share, "/tv");
        assert_eq!(config.sources.ssh["prod"].auth, SshAuthMethod::KeyFile);
    }

    #[test]
    fn saves_config_with_sources_and_keys() {
        let temp = TempDir::new().expect("temp dir");
        let mut config = Config::default();
        config.key_bindings.enter_command_mode = ConfigurableKey::Char(':');
        config.sources.local.insert(
            "home".to_string(),
            LocalSourceProfile {
                label: "Home".to_string(),
                path: PathBuf::from("/tmp/home"),
            },
        );
        config.sources.ftp.insert(
            "archive".to_string(),
            FtpSourceProfile {
                label: "Archive".to_string(),
                host: "ftp.example.com".to_string(),
                port: 21,
                username: "alice".to_string(),
                initial_path: "/incoming".to_string(),
            },
        );

        config.save_to_dir(temp.path()).expect("save config");
        let reloaded = Config::load_from_dir(temp.path()).expect("reload config");

        assert_eq!(
            reloaded.key_bindings.enter_command_mode,
            ConfigurableKey::Char(':')
        );
        assert_eq!(reloaded.sources.local["home"].label, "Home");
        assert_eq!(reloaded.sources.ftp["archive"].initial_path, "/incoming");
    }
}
