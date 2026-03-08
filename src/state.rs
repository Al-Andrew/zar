use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::config::{Config, ConfigurableKey};
use crate::fs::{self, FileEntry};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivePane {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Command,
    Transfer,
    Preview,
}

#[derive(Debug, Clone)]
pub struct StatusMessage {
    pub text: String,
}

impl StatusMessage {
    pub fn info(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }
}

#[derive(Debug, Clone)]
pub struct CommandState {
    pub buffer: String,
    pub cursor: usize,
    pub trigger_key: ConfigurableKey,
}

impl CommandState {
    pub fn new(trigger_key: ConfigurableKey) -> Self {
        Self {
            buffer: String::new(),
            cursor: 0,
            trigger_key,
        }
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
        self.cursor = 0;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferOperation {
    Copy,
    Move,
    CreateDirectory,
    Delete,
}

impl TransferOperation {
    pub fn label(self) -> &'static str {
        match self {
            Self::Copy => "copy",
            Self::Move => "move",
            Self::CreateDirectory => "create directory",
            Self::Delete => "delete",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Copy => "Copy File",
            Self::Move => "Move File",
            Self::CreateDirectory => "Create Directory",
            Self::Delete => "Delete",
        }
    }

    pub fn destination_label(self) -> &'static str {
        match self {
            Self::Copy | Self::Move => "To",
            Self::CreateDirectory => "Path",
            Self::Delete => "Target",
        }
    }

    pub fn shows_source(self) -> bool {
        matches!(self, Self::Copy | Self::Move)
    }

    pub fn edits_destination(self) -> bool {
        matches!(self, Self::Copy | Self::Move | Self::CreateDirectory)
    }

    pub fn past_tense(self) -> &'static str {
        match self {
            Self::Copy => "copied",
            Self::Move => "moved",
            Self::CreateDirectory => "created",
            Self::Delete => "deleted",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferControl {
    SourceField,
    DestinationField,
    ConfirmButton,
    CancelButton,
}

#[derive(Debug, Clone)]
pub struct TransferDialogState {
    pub operation: TransferOperation,
    pub source: PathBuf,
    pub destination: String,
    pub cursor: usize,
    pub focus: TransferControl,
    pub hovered: Option<TransferControl>,
}

impl TransferDialogState {
    pub fn new(operation: TransferOperation, source: PathBuf, destination: String) -> Self {
        let cursor = destination.len();
        Self {
            operation,
            source,
            destination,
            cursor,
            focus: if operation.edits_destination() {
                TransferControl::DestinationField
            } else {
                TransferControl::ConfirmButton
            },
            hovered: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PreviewState {
    pub path: PathBuf,
    pub lines: Vec<String>,
    pub scroll: usize,
}

impl PreviewState {
    pub fn new(path: PathBuf, contents: String) -> Self {
        let lines = if contents.is_empty() {
            vec![String::new()]
        } else {
            contents.lines().map(ToOwned::to_owned).collect()
        };

        Self {
            path,
            lines,
            scroll: 0,
        }
    }

    pub fn move_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(1);
    }

    pub fn move_down(&mut self, viewport_height: usize) {
        let max_scroll = self.lines.len().saturating_sub(viewport_height.max(1));
        self.scroll = (self.scroll + 1).min(max_scroll);
    }
}

#[derive(Debug, Clone)]
pub struct PaneState {
    pub cwd: PathBuf,
    pub entries: Vec<FileEntry>,
    pub selected: usize,
    pub scroll: usize,
    pub last_error: Option<String>,
}

impl PaneState {
    pub fn new(cwd: PathBuf) -> Result<Self> {
        let entries = fs::read_directory(&cwd)?;
        Ok(Self {
            cwd,
            entries,
            selected: 0,
            scroll: 0,
            last_error: None,
        })
    }

    pub fn refresh(&mut self) -> Result<()> {
        let previous_name = self.selected_entry().map(|entry| entry.name.clone());
        let entries = fs::read_directory(&self.cwd)?;

        self.entries = entries;
        self.last_error = None;

        self.selected = previous_name
            .and_then(|name| self.entries.iter().position(|entry| entry.name == name))
            .unwrap_or(self.selected);
        self.clamp_selection();

        Ok(())
    }

    pub fn set_cwd(&mut self, cwd: PathBuf) -> Result<()> {
        let previous_cwd = self.cwd.clone();
        let previous_entries = self.entries.clone();
        let previous_selected = self.selected;
        let previous_scroll = self.scroll;

        self.cwd = cwd;
        match self.refresh() {
            Ok(()) => Ok(()),
            Err(err) => {
                self.cwd = previous_cwd;
                self.entries = previous_entries;
                self.selected = previous_selected;
                self.scroll = previous_scroll;
                self.last_error = Some(err.to_string());
                Err(err)
            }
        }
    }

    pub fn selected_entry(&self) -> Option<&FileEntry> {
        self.entries.get(self.selected)
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if self.selected + 1 < self.entries.len() {
            self.selected += 1;
        }
    }

    pub fn go_parent(&mut self) -> Result<bool> {
        if let Some(parent) = self.cwd.parent().map(Path::to_path_buf) {
            if parent != self.cwd {
                self.set_cwd(parent)?;
                return Ok(true);
            }
        }

        Ok(false)
    }

    pub fn ensure_visible(&mut self, viewport_height: usize) {
        if viewport_height == 0 {
            self.scroll = self.selected;
            return;
        }

        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + viewport_height {
            self.scroll = self.selected + 1 - viewport_height;
        }
    }

    pub fn clamp_selection(&mut self) {
        if self.entries.is_empty() {
            self.selected = 0;
            self.scroll = 0;
            return;
        }

        self.selected = self.selected.min(self.entries.len() - 1);
        self.scroll = self.scroll.min(self.selected);
    }
}

#[derive(Debug)]
pub struct AppState {
    pub left: PaneState,
    pub right: PaneState,
    pub active_pane: ActivePane,
    pub mode: InputMode,
    pub command: CommandState,
    pub transfer: Option<TransferDialogState>,
    pub preview: Option<PreviewState>,
    pub status: StatusMessage,
    pub should_quit: bool,
    pub config: Config,
}

impl AppState {
    pub fn new(config: Config, cwd: PathBuf) -> Result<Self> {
        Self::new_with_dirs(config, cwd.clone(), cwd)
    }

    pub fn new_with_dirs(config: Config, left_cwd: PathBuf, right_cwd: PathBuf) -> Result<Self> {
        let left = PaneState::new(left_cwd)?;
        let right = PaneState::new(right_cwd)?;
        let status = if config.startup_warnings.is_empty() {
            StatusMessage::info(format!(
                "Tab switch pane | Enter open | Backspace up | {} command | q quit",
                config.key_bindings.enter_command_mode.label()
            ))
        } else {
            StatusMessage::error(config.startup_warnings.join(" | "))
        };

        Ok(Self {
            left,
            right,
            active_pane: ActivePane::Left,
            mode: InputMode::Normal,
            command: CommandState::new(config.key_bindings.enter_command_mode.clone()),
            transfer: None,
            preview: None,
            status,
            should_quit: false,
            config,
        })
    }

    pub fn active_pane(&self) -> &PaneState {
        match self.active_pane {
            ActivePane::Left => &self.left,
            ActivePane::Right => &self.right,
        }
    }

    pub fn active_pane_mut(&mut self) -> &mut PaneState {
        match self.active_pane {
            ActivePane::Left => &mut self.left,
            ActivePane::Right => &mut self.right,
        }
    }

    pub fn inactive_pane(&self) -> &PaneState {
        match self.active_pane {
            ActivePane::Left => &self.right,
            ActivePane::Right => &self.left,
        }
    }

    pub fn set_status(&mut self, status: StatusMessage) {
        self.status = status;
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::PaneState;

    #[test]
    fn refresh_clamps_selection_when_entries_shrink() {
        let temp = TempDir::new().expect("temp dir");
        fs::write(temp.path().join("a.txt"), b"a").expect("write a");
        fs::write(temp.path().join("b.txt"), b"b").expect("write b");

        let mut pane = PaneState::new(temp.path().to_path_buf()).expect("pane");
        pane.selected = 1;

        fs::remove_file(temp.path().join("b.txt")).expect("remove b");
        pane.refresh().expect("refresh");

        assert_eq!(pane.selected, 0);
    }
}
