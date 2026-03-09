use anyhow::Result;

use crate::config::{Config, ConfigurableKey};
use crate::source::{FileEntry, LocationPath, SourceCategory, SourceKind, SourceRef};
use crate::vfs::VfsSession;

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
    SourceMenu,
    AddLocation,
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
    pub source: LocationPath,
    pub source_label: String,
    pub source_kind: SourceKind,
    pub destination: String,
    pub cursor: usize,
    pub focus: TransferControl,
    pub hovered: Option<TransferControl>,
}

impl TransferDialogState {
    pub fn new(
        operation: TransferOperation,
        source: LocationPath,
        source_label: String,
        source_kind: SourceKind,
        destination: String,
    ) -> Self {
        let cursor = destination.len();
        Self {
            operation,
            source,
            source_label,
            source_kind,
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
    pub title: String,
    pub path: LocationPath,
    pub lines: Vec<String>,
    pub scroll: usize,
}

impl PreviewState {
    pub fn new(title: String, path: LocationPath, contents: String) -> Self {
        let lines = if contents.is_empty() {
            vec![String::new()]
        } else {
            contents.lines().map(ToOwned::to_owned).collect()
        };

        Self {
            title,
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
pub struct PaneSourceState {
    pub source_ref: SourceRef,
    pub kind: SourceKind,
    pub label: String,
}

pub struct PaneState {
    pub source: PaneSourceState,
    pub cwd: LocationPath,
    pub entries: Vec<FileEntry>,
    pub selected: usize,
    pub scroll: usize,
    pub last_error: Option<String>,
    pub session: Box<dyn VfsSession>,
}

impl PaneState {
    pub fn new(
        source_ref: SourceRef,
        kind: SourceKind,
        label: String,
        cwd: LocationPath,
        mut session: Box<dyn VfsSession>,
    ) -> Result<Self> {
        let entries = session.list_dir(&cwd)?;
        Ok(Self {
            source: PaneSourceState {
                source_ref,
                kind,
                label,
            },
            cwd,
            entries,
            selected: 0,
            scroll: 0,
            last_error: None,
            session,
        })
    }

    pub fn title(&self) -> String {
        format!(
            "{}: {} | {}",
            self.source.kind.label(),
            self.source.label,
            self.cwd.display()
        )
    }

    pub fn refresh(&mut self) -> Result<()> {
        let previous_name = self.selected_entry().map(|entry| entry.name.clone());
        let entries = self.session.list_dir(&self.cwd)?;
        self.entries = entries;
        self.last_error = None;
        self.selected = previous_name
            .and_then(|name| self.entries.iter().position(|entry| entry.name == name))
            .unwrap_or(self.selected);
        self.clamp_selection();
        Ok(())
    }

    pub fn set_cwd(&mut self, cwd: LocationPath) -> Result<()> {
        let previous_cwd = self.cwd.clone();
        let previous_entries = self.entries.clone();
        let previous_selected = self.selected;
        let previous_scroll = self.scroll;

        self.cwd = self.session.change_dir(&cwd)?;
        match self.refresh() {
            Ok(()) => Ok(()),
            Err(err) => {
                let _ = self.session.change_dir(&previous_cwd);
                self.cwd = previous_cwd;
                self.entries = previous_entries;
                self.selected = previous_selected;
                self.scroll = previous_scroll;
                self.last_error = Some(err.to_string());
                Err(err)
            }
        }
    }

    pub fn replace_source(
        &mut self,
        source_ref: SourceRef,
        kind: SourceKind,
        label: String,
        cwd: LocationPath,
        mut session: Box<dyn VfsSession>,
    ) -> Result<()> {
        let entries = session.list_dir(&cwd)?;
        self.source = PaneSourceState {
            source_ref,
            kind,
            label,
        };
        self.cwd = cwd;
        self.entries = entries;
        self.selected = 0;
        self.scroll = 0;
        self.last_error = None;
        self.session = session;
        Ok(())
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
        if let Some(parent) = self.cwd.parent() {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceMenuLevel {
    Categories,
    Items(SourceCategory),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceMenuEntry {
    pub source_ref: SourceRef,
    pub label: String,
    pub path_hint: LocationPath,
}

#[derive(Debug, Clone)]
pub struct SourceMenuState {
    pub target_pane: ActivePane,
    pub level: SourceMenuLevel,
    pub category_selected: usize,
    pub item_selected: usize,
    pub items: Vec<SourceMenuEntry>,
}

impl SourceMenuState {
    pub fn new(target_pane: ActivePane) -> Self {
        Self {
            target_pane,
            level: SourceMenuLevel::Categories,
            category_selected: 0,
            item_selected: 0,
            items: Vec::new(),
        }
    }

    pub fn categories() -> [SourceCategory; 5] {
        [
            SourceCategory::History,
            SourceCategory::Local,
            SourceCategory::Ftp,
            SourceCategory::Smb,
            SourceCategory::Ssh,
        ]
    }

    pub fn selected_category(&self) -> SourceCategory {
        Self::categories()[self.category_selected]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddLocationKind {
    Local,
    Ftp,
    Smb,
    Ssh,
}

impl AddLocationKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Local => "Local",
            Self::Ftp => "FTP",
            Self::Smb => "SMB",
            Self::Ssh => "SSH",
        }
    }

    pub fn target_label(self) -> &'static str {
        match self {
            Self::Local => "Path",
            Self::Ftp | Self::Smb | Self::Ssh => "Target",
        }
    }

    pub fn target_example(self) -> &'static str {
        match self {
            Self::Local => "/srv/data",
            Self::Ftp => "ftp://alice@example.com:21/incoming",
            Self::Smb => "smb://alice@nas/media/shows?workgroup=WORK",
            Self::Ssh => "ssh://deploy@example.com:22/var/www",
        }
    }

    pub fn uses_secret(self) -> bool {
        !matches!(self, Self::Local)
    }

    pub fn previous(self) -> Self {
        match self {
            Self::Local => Self::Ssh,
            Self::Ftp => Self::Local,
            Self::Smb => Self::Ftp,
            Self::Ssh => Self::Smb,
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Local => Self::Ftp,
            Self::Ftp => Self::Smb,
            Self::Smb => Self::Ssh,
            Self::Ssh => Self::Local,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddLocationControl {
    KindField,
    LabelField,
    TargetField,
    SecretField,
    ConfirmButton,
    CancelButton,
}

#[derive(Debug, Clone)]
pub struct AddLocationDialogState {
    pub kind: AddLocationKind,
    pub label: String,
    pub label_cursor: usize,
    pub target: String,
    pub target_cursor: usize,
    pub secret: String,
    pub secret_cursor: usize,
    pub focus: AddLocationControl,
    pub hovered: Option<AddLocationControl>,
}

impl AddLocationDialogState {
    pub fn new() -> Self {
        Self {
            kind: AddLocationKind::Local,
            label: String::new(),
            label_cursor: 0,
            target: String::new(),
            target_cursor: 0,
            secret: String::new(),
            secret_cursor: 0,
            focus: AddLocationControl::KindField,
            hovered: None,
        }
    }
}

pub struct AppState {
    pub left: PaneState,
    pub right: PaneState,
    pub active_pane: ActivePane,
    pub mode: InputMode,
    pub command: CommandState,
    pub transfer: Option<TransferDialogState>,
    pub preview: Option<PreviewState>,
    pub source_menu: Option<SourceMenuState>,
    pub add_location: Option<AddLocationDialogState>,
    pub footer_hovered: Option<usize>,
    pub status: StatusMessage,
    pub should_quit: bool,
    pub config: Config,
}

impl AppState {
    pub fn new(config: Config, left: PaneState, right: PaneState) -> Self {
        let status = if config.startup_warnings.is_empty() {
            StatusMessage::info(default_status_text(&config))
        } else {
            StatusMessage::error(config.startup_warnings.join(" | "))
        };

        Self {
            left,
            right,
            active_pane: ActivePane::Left,
            mode: InputMode::Normal,
            command: CommandState::new(config.key_bindings.enter_command_mode.clone()),
            transfer: None,
            preview: None,
            source_menu: None,
            add_location: None,
            footer_hovered: None,
            status,
            should_quit: false,
            config,
        }
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

    pub fn inactive_pane_mut(&mut self) -> &mut PaneState {
        match self.active_pane {
            ActivePane::Left => &mut self.right,
            ActivePane::Right => &mut self.left,
        }
    }

    pub fn pane(&self, pane: ActivePane) -> &PaneState {
        match pane {
            ActivePane::Left => &self.left,
            ActivePane::Right => &self.right,
        }
    }

    pub fn pane_mut(&mut self, pane: ActivePane) -> &mut PaneState {
        match pane {
            ActivePane::Left => &mut self.left,
            ActivePane::Right => &mut self.right,
        }
    }

    pub fn set_status(&mut self, status: StatusMessage) {
        self.status = status;
    }
}

pub fn default_status_text(config: &Config) -> String {
    format!(
        "Tab switch pane | F1/F2 source | F4 add location | Enter open | Backspace up | {} command | q quit",
        config.key_bindings.enter_command_mode.label()
    )
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use tempfile::TempDir;

    use crate::config::Config;
    use crate::source::{LocationPath, SourceKind, SourceRef};
    use crate::state::PaneState;
    use crate::vfs::LocalSession;

    #[test]
    fn refresh_clamps_selection_when_entries_shrink() {
        let temp = TempDir::new().expect("temp dir");
        fs::write(temp.path().join("a.txt"), b"a").expect("write a");
        fs::write(temp.path().join("b.txt"), b"b").expect("write b");

        let mut pane = PaneState::new(
            SourceRef::InlineLocal {
                path: temp.path().to_path_buf(),
                label: "Tmp".to_string(),
            },
            SourceKind::Local,
            "Tmp".to_string(),
            LocationPath::Local(temp.path().to_path_buf()),
            Box::new(
                LocalSession::new("Tmp".to_string(), temp.path().to_path_buf()).expect("session"),
            ),
        )
        .expect("pane");
        pane.selected = 1;

        fs::remove_file(temp.path().join("b.txt")).expect("remove b");
        pane.refresh().expect("refresh");

        assert_eq!(pane.selected, 0);
        assert!(crate::state::default_status_text(&Config::default()).contains("F1/F2"));
        assert_eq!(pane.cwd, LocationPath::Local(PathBuf::from(temp.path())));
    }
}
