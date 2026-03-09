use std::io::{Seek, SeekFrom, Stdout};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use crossterm::event::{self, MouseButton, MouseEvent, MouseEventKind};
use directories::BaseDirs;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use tempfile::NamedTempFile;
use url::Url;

use crate::commands;
use crate::config::{
    Config, FtpSourceProfile, LocalSourceProfile, SmbSourceProfile, SshAuthMethod,
    SshSourceProfile, config_dir,
};
use crate::history::{HistoryEntry, HistoryStore, TomlHistoryStore};
use crate::input::{Action, CommandEditAction, event_to_action};
use crate::secrets::{PlaintextSecretStore, SecretStore};
use crate::source::{LocationPath, SourceCategory, SourceRef};
use crate::state::{
    ActivePane, AddLocationControl, AddLocationDialogState, AddLocationKind, AppState, InputMode,
    PaneState, PreviewState, SourceMenuEntry, SourceMenuLevel, SourceMenuState, StatusMessage,
    TransferControl, TransferDialogState, TransferOperation, default_status_text,
};
use crate::ui;
use crate::vfs::{DefaultSessionFactory, SessionFactory};

pub struct App {
    pub state: AppState,
    factory: Box<dyn SessionFactory>,
    secrets: Box<dyn SecretStore>,
    history: Box<dyn HistoryStore>,
    config_dir: PathBuf,
}

impl App {
    pub fn new(config: Config) -> Result<Self> {
        Self::new_with_factory(config, None, None, Box::new(DefaultSessionFactory))
    }

    pub fn new_with_start_dir(config: Config, start_dir: Option<PathBuf>) -> Result<Self> {
        Self::new_with_factory(config, start_dir, None, Box::new(DefaultSessionFactory))
    }

    pub fn new_with_start_dirs(
        config: Config,
        left_start_dir: Option<PathBuf>,
        right_start_dir: Option<PathBuf>,
    ) -> Result<Self> {
        Self::new_with_factory(
            config,
            left_start_dir,
            right_start_dir,
            Box::new(DefaultSessionFactory),
        )
    }

    pub fn new_with_factory(
        config: Config,
        left_start_dir: Option<PathBuf>,
        right_start_dir: Option<PathBuf>,
        factory: Box<dyn SessionFactory>,
    ) -> Result<Self> {
        let dir = config_dir().unwrap_or_else(|_| PathBuf::from("."));
        let secrets: Box<dyn SecretStore> = Box::new(PlaintextSecretStore::load_from_dir(&dir)?);
        let history: Box<dyn HistoryStore> = Box::new(TomlHistoryStore::load_from_dir(&dir)?);
        Self::new_with_services_at_dir(
            config,
            left_start_dir,
            right_start_dir,
            factory,
            secrets,
            history,
            dir,
        )
    }

    pub fn new_with_services(
        config: Config,
        left_start_dir: Option<PathBuf>,
        right_start_dir: Option<PathBuf>,
        factory: Box<dyn SessionFactory>,
        secrets: Box<dyn SecretStore>,
        history: Box<dyn HistoryStore>,
    ) -> Result<Self> {
        let dir = config_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self::new_with_services_at_dir(
            config,
            left_start_dir,
            right_start_dir,
            factory,
            secrets,
            history,
            dir,
        )
    }

    pub fn new_with_services_at_dir(
        config: Config,
        left_start_dir: Option<PathBuf>,
        right_start_dir: Option<PathBuf>,
        factory: Box<dyn SessionFactory>,
        secrets: Box<dyn SecretStore>,
        history: Box<dyn HistoryStore>,
        config_dir: PathBuf,
    ) -> Result<Self> {
        let (left_cwd, right_cwd) = resolve_start_dirs(left_start_dir, right_start_dir)?;
        let left_source = SourceRef::InlineLocal {
            label: display_name(&left_cwd),
            path: left_cwd.clone(),
        };
        let right_source = SourceRef::InlineLocal {
            label: display_name(&right_cwd),
            path: right_cwd.clone(),
        };
        let left_connected = factory.connect(&config, secrets.as_ref(), &left_source)?;
        let right_connected = factory.connect(&config, secrets.as_ref(), &right_source)?;

        let left = PaneState::new(
            left_connected.source_ref,
            left_connected.kind,
            left_connected.label,
            left_connected.default_path,
            left_connected.session,
        )?;
        let right = PaneState::new(
            right_connected.source_ref,
            right_connected.kind,
            right_connected.label,
            right_connected.default_path,
            right_connected.session,
        )?;

        Ok(Self {
            state: AppState::new(config, left, right),
            factory,
            secrets,
            history,
            config_dir,
        })
    }

    pub fn run(&mut self, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
        while !self.state.should_quit {
            terminal.draw(|frame| ui::render(frame, &mut self.state))?;

            if event::poll(Duration::from_millis(250))? {
                let event = event::read()?;
                match event {
                    event::Event::Mouse(mouse_event) => {
                        let size = terminal.size()?;
                        self.handle_mouse(
                            mouse_event,
                            ratatui::layout::Rect::new(0, 0, size.width, size.height),
                        )?;
                    }
                    other => {
                        if let Some(action) =
                            event_to_action(&self.state.config.key_bindings, self.state.mode, other)
                        {
                            self.handle_action(action)?;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    pub fn handle_action(&mut self, action: Action) -> Result<()> {
        match action {
            Action::MoveUp => match self.state.mode {
                InputMode::SourceMenu => self.move_source_menu_up(),
                InputMode::AddLocation => self.move_add_location_focus_up(),
                _ => self.state.active_pane_mut().move_up(),
            },
            Action::MoveDown => match self.state.mode {
                InputMode::SourceMenu => self.move_source_menu_down(),
                InputMode::AddLocation => self.move_add_location_focus_down(),
                _ => self.state.active_pane_mut().move_down(),
            },
            Action::SwitchPane => {
                self.state.active_pane = match self.state.active_pane {
                    ActivePane::Left => ActivePane::Right,
                    ActivePane::Right => ActivePane::Left,
                };
                self.state.set_status(StatusMessage::info(format!(
                    "active pane: {}",
                    match self.state.active_pane {
                        ActivePane::Left => "left",
                        ActivePane::Right => "right",
                    }
                )));
            }
            Action::OpenLeftSourceMenu => self.open_source_menu(ActivePane::Left),
            Action::OpenRightSourceMenu => self.open_source_menu(ActivePane::Right),
            Action::SourceMenuBack => self.source_menu_back(),
            Action::SourceMenuSelect => self.select_source_menu_item()?,
            Action::CloseSourceMenu => {
                self.state.source_menu = None;
                self.state.mode = InputMode::Normal;
                self.state
                    .set_status(StatusMessage::info("source selection cancelled"));
            }
            Action::BeginPreview => self.begin_preview(),
            Action::OpenSelection => self.open_selection(),
            Action::GoParent => match self.state.active_pane_mut().go_parent() {
                Ok(true) => {
                    let path = self.state.active_pane().cwd.display();
                    self.record_history_for_pane(self.state.active_pane)?;
                    self.state
                        .set_status(StatusMessage::info(format!("active pane: {path}")));
                }
                Ok(false) => self
                    .state
                    .set_status(StatusMessage::info("already at filesystem root")),
                Err(err) => self.state.set_status(StatusMessage::error(err.to_string())),
            },
            Action::BeginCopy => self.begin_transfer(TransferOperation::Copy),
            Action::BeginMove => self.begin_transfer(TransferOperation::Move),
            Action::BeginCreateDirectory => self.begin_create_directory(),
            Action::BeginDelete => self.begin_delete(),
            Action::BeginAddLocation => self.begin_add_location(),
            Action::EnterCommandMode => {
                self.state.mode = InputMode::Command;
                self.state.command.clear();
                self.state
                    .set_status(StatusMessage::info("command mode: enter a command"));
            }
            Action::EditCommand(edit) => self.edit_command(edit),
            Action::EditTransfer(edit) => self.edit_transfer(edit),
            Action::EditAddLocation(edit) => self.edit_add_location(edit),
            Action::TransferFocusUp => self.move_transfer_focus_up(),
            Action::TransferFocusDown => self.move_transfer_focus_down(),
            Action::TransferFocusLeft => self.move_transfer_focus_left(),
            Action::TransferFocusRight => self.move_transfer_focus_right(),
            Action::AddLocationFocusUp => self.move_add_location_focus_up(),
            Action::AddLocationFocusDown => self.move_add_location_focus_down(),
            Action::AddLocationFocusLeft => self.move_add_location_focus_left(),
            Action::AddLocationFocusRight => self.move_add_location_focus_right(),
            Action::PreviewUp => self.preview_up(),
            Action::PreviewDown => self.preview_down(),
            Action::SubmitCommand => self.submit_command(),
            Action::SubmitTransfer => self.submit_transfer()?,
            Action::SubmitAddLocation => self.submit_add_location()?,
            Action::CancelCommand => {
                self.state.command.clear();
                self.state.mode = InputMode::Normal;
                self.state
                    .set_status(StatusMessage::info("command cancelled"));
            }
            Action::CancelTransfer => {
                self.state.transfer = None;
                self.state.mode = InputMode::Normal;
                self.state
                    .set_status(StatusMessage::info("file operation cancelled"));
            }
            Action::CancelAddLocation => {
                self.state.add_location = None;
                self.state.mode = InputMode::Normal;
                self.state
                    .set_status(StatusMessage::info("add location cancelled"));
            }
            Action::ClosePreview => {
                self.state.preview = None;
                self.state.mode = InputMode::Normal;
                self.state.set_status(StatusMessage::info("preview closed"));
            }
            Action::Quit => self.state.should_quit = true,
            Action::ClearStatus => self
                .state
                .set_status(StatusMessage::info(default_status_text(&self.state.config))),
        }

        Ok(())
    }

    pub fn change_active_directory(&mut self, path: LocationPath) -> Result<()> {
        self.state.active_pane_mut().set_cwd(path)?;
        self.record_history_for_pane(self.state.active_pane)
    }

    pub fn switch_pane_source(
        &mut self,
        pane: ActivePane,
        source: SourceRef,
        preferred_path: Option<LocationPath>,
    ) -> Result<()> {
        let connected = self
            .factory
            .connect(&self.state.config, self.secrets.as_ref(), &source)?;
        let target_path = preferred_path
            .or_else(|| self.history.last_path_for(&source))
            .unwrap_or_else(|| connected.default_path.clone());

        let mut session = connected.session;
        let cwd = if target_path == connected.default_path {
            session.pwd()?
        } else {
            session.change_dir(&target_path)?
        };
        let entries = session.list_dir(&cwd)?;

        {
            let pane_state = self.state.pane_mut(pane);
            pane_state.session.disconnect()?;
            pane_state.source = crate::state::PaneSourceState {
                source_ref: connected.source_ref.clone(),
                kind: connected.kind,
                label: connected.label.clone(),
            };
            pane_state.cwd = cwd.clone();
            pane_state.entries = entries;
            pane_state.selected = 0;
            pane_state.scroll = 0;
            pane_state.last_error = None;
            pane_state.session = session;
        }

        self.record_history_for_pane(pane)?;
        Ok(())
    }

    fn open_source_menu(&mut self, pane: ActivePane) {
        self.state.active_pane = pane;
        self.state.source_menu = Some(SourceMenuState::new(pane));
        self.state.mode = InputMode::SourceMenu;
        self.state
            .set_status(StatusMessage::info("source selection: choose a category"));
    }

    fn move_source_menu_up(&mut self) {
        let Some(menu) = self.state.source_menu.as_mut() else {
            return;
        };

        match menu.level {
            SourceMenuLevel::Categories => {
                menu.category_selected = menu.category_selected.saturating_sub(1);
            }
            SourceMenuLevel::Items(_) => {
                menu.item_selected = menu.item_selected.saturating_sub(1);
            }
        }
    }

    fn move_source_menu_down(&mut self) {
        let Some(menu) = self.state.source_menu.as_mut() else {
            return;
        };

        match menu.level {
            SourceMenuLevel::Categories => {
                menu.category_selected =
                    (menu.category_selected + 1).min(SourceMenuState::categories().len() - 1);
            }
            SourceMenuLevel::Items(_) => {
                if !menu.items.is_empty() {
                    menu.item_selected = (menu.item_selected + 1).min(menu.items.len() - 1);
                }
            }
        }
    }

    fn source_menu_back(&mut self) {
        let Some(menu) = self.state.source_menu.as_mut() else {
            return;
        };

        match menu.level {
            SourceMenuLevel::Categories => {
                self.state.source_menu = None;
                self.state.mode = InputMode::Normal;
            }
            SourceMenuLevel::Items(_) => {
                menu.level = SourceMenuLevel::Categories;
                menu.item_selected = 0;
                menu.items.clear();
                self.state
                    .set_status(StatusMessage::info("source selection: choose a category"));
            }
        }
    }

    fn select_source_menu_item(&mut self) -> Result<()> {
        let Some(menu) = self.state.source_menu.clone() else {
            return Ok(());
        };

        match menu.level {
            SourceMenuLevel::Categories => {
                let category = menu.selected_category();
                let items = self.source_menu_items(category);
                let menu_state = self.state.source_menu.as_mut().expect("source menu");
                menu_state.level = SourceMenuLevel::Items(category);
                menu_state.item_selected = 0;
                menu_state.items = items;
                self.state.set_status(StatusMessage::info(format!(
                    "source selection: {}",
                    category.title()
                )));
            }
            SourceMenuLevel::Items(category) => {
                let Some(entry) = menu.items.get(menu.item_selected).cloned() else {
                    return Ok(());
                };
                self.switch_pane_source(
                    menu.target_pane,
                    entry.source_ref.clone(),
                    Some(entry.path_hint.clone()),
                )?;
                self.state.source_menu = None;
                self.state.mode = InputMode::Normal;
                self.state.set_status(StatusMessage::info(format!(
                    "active pane: {}",
                    self.state.pane(menu.target_pane).title()
                )));
                if matches!(category, SourceCategory::History) {
                    self.state.active_pane = menu.target_pane;
                }
            }
        }

        Ok(())
    }

    fn source_menu_items(&self, category: SourceCategory) -> Vec<SourceMenuEntry> {
        match category {
            SourceCategory::History => self
                .history
                .entries()
                .iter()
                .filter_map(|entry| history_entry_to_menu_entry(&self.state.config, entry))
                .collect(),
            SourceCategory::Local => self
                .state
                .config
                .sources
                .local
                .iter()
                .map(|(id, profile)| SourceMenuEntry {
                    source_ref: SourceRef::SavedLocal { id: id.clone() },
                    label: profile.label.clone(),
                    path_hint: LocationPath::Local(profile.path.clone()),
                })
                .collect(),
            SourceCategory::Ftp => self
                .state
                .config
                .sources
                .ftp
                .iter()
                .map(|(id, profile)| SourceMenuEntry {
                    source_ref: SourceRef::SavedFtp { id: id.clone() },
                    label: profile.label.clone(),
                    path_hint: self
                        .history
                        .last_path_for(&SourceRef::SavedFtp { id: id.clone() })
                        .unwrap_or_else(|| LocationPath::Remote(profile.initial_path.clone())),
                })
                .collect(),
            SourceCategory::Smb => self
                .state
                .config
                .sources
                .smb
                .iter()
                .map(|(id, profile)| SourceMenuEntry {
                    source_ref: SourceRef::SavedSmb { id: id.clone() },
                    label: profile.label.clone(),
                    path_hint: self
                        .history
                        .last_path_for(&SourceRef::SavedSmb { id: id.clone() })
                        .unwrap_or_else(|| LocationPath::Remote(profile.initial_path.clone())),
                })
                .collect(),
            SourceCategory::Ssh => self
                .state
                .config
                .sources
                .ssh
                .iter()
                .map(|(id, profile)| SourceMenuEntry {
                    source_ref: SourceRef::SavedSsh { id: id.clone() },
                    label: profile.label.clone(),
                    path_hint: self
                        .history
                        .last_path_for(&SourceRef::SavedSsh { id: id.clone() })
                        .unwrap_or_else(|| LocationPath::Remote(profile.initial_path.clone())),
                })
                .collect(),
        }
    }

    fn begin_transfer(&mut self, operation: TransferOperation) {
        let Some(entry) = self.state.active_pane().selected_entry().cloned() else {
            self.state
                .set_status(StatusMessage::info("nothing selected"));
            return;
        };

        if !matches!(
            operation,
            TransferOperation::CreateDirectory | TransferOperation::Delete
        ) && !entry.kind.is_file()
        {
            self.state.set_status(StatusMessage::error(format!(
                "{} supports files only",
                operation.label()
            )));
            return;
        }

        let destination = self.state.inactive_pane().cwd.display();
        let active_pane = self.state.active_pane();
        self.state.transfer = Some(TransferDialogState::new(
            operation,
            entry.path,
            active_pane.source.label.clone(),
            active_pane.source.kind,
            destination,
        ));
        self.state.mode = InputMode::Transfer;
    }

    fn begin_preview(&mut self) {
        let Some(entry) = self.state.active_pane().selected_entry().cloned() else {
            self.state
                .set_status(StatusMessage::info("nothing selected"));
            return;
        };

        if !entry.kind.is_file() {
            self.state.set_status(StatusMessage::error(format!(
                "{} is not a file",
                entry.display_name()
            )));
            return;
        }

        let pane = self.state.active_pane_mut();
        match pane.session.read_text_file(&entry.path) {
            Ok(contents) => {
                self.state.preview = Some(PreviewState::new(pane.title(), entry.path, contents));
                self.state.mode = InputMode::Preview;
            }
            Err(err) => self.state.set_status(StatusMessage::error(err.to_string())),
        }
    }

    fn begin_create_directory(&mut self) {
        let active_pane = self.state.active_pane();
        let destination = active_pane.cwd.join_child("new-dir").display();
        self.state.transfer = Some(TransferDialogState::new(
            TransferOperation::CreateDirectory,
            active_pane.cwd.clone(),
            active_pane.source.label.clone(),
            active_pane.source.kind,
            destination,
        ));
        self.state.mode = InputMode::Transfer;
    }

    fn begin_delete(&mut self) {
        let Some(entry) = self.state.active_pane().selected_entry().cloned() else {
            self.state
                .set_status(StatusMessage::info("nothing selected"));
            return;
        };

        let active_pane = self.state.active_pane();
        self.state.transfer = Some(TransferDialogState::new(
            TransferOperation::Delete,
            entry.path,
            active_pane.source.label.clone(),
            active_pane.source.kind,
            String::new(),
        ));
        self.state.mode = InputMode::Transfer;
    }

    fn begin_add_location(&mut self) {
        self.state.add_location = Some(AddLocationDialogState::new());
        self.state.mode = InputMode::AddLocation;
        self.state.set_status(StatusMessage::info(
            "add location: choose a type, label, and target",
        ));
    }

    fn open_selection(&mut self) {
        let Some(entry) = self.state.active_pane().selected_entry().cloned() else {
            self.state
                .set_status(StatusMessage::info("nothing to open"));
            return;
        };

        if entry.kind.is_directory() {
            match self.change_active_directory(entry.path.clone()) {
                Ok(()) => self.state.set_status(StatusMessage::info(format!(
                    "active pane: {}",
                    self.state.active_pane().cwd.display()
                ))),
                Err(err) => self.state.set_status(StatusMessage::error(err.to_string())),
            }
        } else {
            self.state.set_status(StatusMessage::info(format!(
                "{} is not a directory",
                entry.display_name()
            )));
        }
    }

    fn edit_command(&mut self, edit: CommandEditAction) {
        edit_text(
            &mut self.state.command.buffer,
            &mut self.state.command.cursor,
            edit,
        );
    }

    fn edit_transfer(&mut self, edit: CommandEditAction) {
        let Some(transfer) = self.state.transfer.as_mut() else {
            return;
        };

        if !transfer.operation.edits_destination()
            || transfer.focus != TransferControl::DestinationField
        {
            return;
        }

        edit_text(&mut transfer.destination, &mut transfer.cursor, edit);
    }

    fn edit_add_location(&mut self, edit: CommandEditAction) {
        let Some(dialog) = self.state.add_location.as_mut() else {
            return;
        };

        match dialog.focus {
            AddLocationControl::LabelField => {
                edit_text(&mut dialog.label, &mut dialog.label_cursor, edit)
            }
            AddLocationControl::TargetField => {
                edit_text(&mut dialog.target, &mut dialog.target_cursor, edit)
            }
            AddLocationControl::SecretField if dialog.kind.uses_secret() => {
                edit_text(&mut dialog.secret, &mut dialog.secret_cursor, edit)
            }
            _ => {}
        }
    }

    fn submit_command(&mut self) {
        let command = self.state.command.buffer.clone();
        self.state.command.clear();
        self.state.mode = InputMode::Normal;

        let result = commands::execute(self, &command);
        commands::apply_result(self, result);
    }

    fn submit_transfer(&mut self) -> Result<()> {
        let Some(dialog) = self.state.transfer.clone() else {
            return Ok(());
        };

        if dialog.focus == TransferControl::CancelButton {
            self.state.transfer = None;
            self.state.mode = InputMode::Normal;
            self.state
                .set_status(StatusMessage::info("file operation cancelled"));
            return Ok(());
        }

        match dialog.operation {
            TransferOperation::Copy | TransferOperation::Move => {
                let raw_destination = dialog.destination.trim();
                if raw_destination.is_empty() {
                    self.state
                        .set_status(StatusMessage::error("destination cannot be empty"));
                    return Ok(());
                }

                let destination =
                    self.resolve_transfer_destination(raw_destination, &dialog.source)?;
                let same_source = self.state.active_pane().source.source_ref.stable_key()
                    == self.state.inactive_pane().source.source_ref.stable_key();

                if same_source {
                    let active = self.state.active_pane_mut();
                    match dialog.operation {
                        TransferOperation::Copy => active
                            .session
                            .copy_file_within_source(&dialog.source, &destination)?,
                        TransferOperation::Move => active
                            .session
                            .move_entry_within_source(&dialog.source, &destination)?,
                        _ => unreachable!(),
                    }
                } else {
                    self.copy_between_panes(&dialog.source, &destination)?;
                    if dialog.operation == TransferOperation::Move {
                        self.state
                            .active_pane_mut()
                            .session
                            .delete_entry(&dialog.source)?;
                    }
                }

                self.finish_transfer(dialog, destination)?;
            }
            TransferOperation::CreateDirectory => {
                let raw_destination = dialog.destination.trim();
                if raw_destination.is_empty() {
                    self.state
                        .set_status(StatusMessage::error("destination cannot be empty"));
                    return Ok(());
                }
                let destination = LocationPath::from_input(
                    dialog.source_kind,
                    &self.state.active_pane().cwd,
                    raw_destination,
                );
                self.state
                    .active_pane_mut()
                    .session
                    .create_dir(&destination)?;
                self.finish_transfer(dialog, destination)?;
            }
            TransferOperation::Delete => {
                self.state
                    .active_pane_mut()
                    .session
                    .delete_entry(&dialog.source)?;
                self.finish_transfer(dialog.clone(), dialog.source.clone())?;
            }
        }

        Ok(())
    }

    fn submit_add_location(&mut self) -> Result<()> {
        let Some(dialog) = self.state.add_location.clone() else {
            return Ok(());
        };

        if dialog.focus == AddLocationControl::CancelButton {
            self.state.add_location = None;
            self.state.mode = InputMode::Normal;
            self.state
                .set_status(StatusMessage::info("add location cancelled"));
            return Ok(());
        }

        let label = dialog.label.trim();
        if label.is_empty() {
            self.state
                .set_status(StatusMessage::error("label cannot be empty"));
            return Ok(());
        }

        let target = dialog.target.trim();
        if target.is_empty() {
            self.state
                .set_status(StatusMessage::error("target cannot be empty"));
            return Ok(());
        }

        let id = unique_profile_id(label, &self.state.config);
        let normalized_secret = normalize_optional_secret(&dialog.secret);

        match dialog.kind {
            AddLocationKind::Local => {
                let path = resolve_saved_local_path(self.state.active_pane().cwd.clone(), target)?;
                self.state.config.sources.local.insert(
                    id.clone(),
                    LocalSourceProfile {
                        label: label.to_string(),
                        path,
                    },
                );
            }
            AddLocationKind::Ftp => {
                let mut parsed = parse_ftp_target(target, normalized_secret.as_deref())?;
                if parsed.secret.is_none() {
                    self.state
                        .set_status(StatusMessage::error("ftp locations require a password"));
                    return Ok(());
                }
                parsed.profile.label = label.to_string();
                self.state
                    .config
                    .sources
                    .ftp
                    .insert(id.clone(), parsed.profile);
                self.secrets
                    .set_ftp_password(&id, parsed.secret)
                    .context("failed to store ftp secret")?;
            }
            AddLocationKind::Smb => {
                let mut parsed = parse_smb_target(target, normalized_secret.as_deref())?;
                if parsed.secret.is_none() {
                    self.state
                        .set_status(StatusMessage::error("smb locations require a password"));
                    return Ok(());
                }
                parsed.profile.label = label.to_string();
                self.state
                    .config
                    .sources
                    .smb
                    .insert(id.clone(), parsed.profile);
                self.secrets
                    .set_smb_password(&id, parsed.secret)
                    .context("failed to store smb secret")?;
            }
            AddLocationKind::Ssh => {
                let mut parsed = parse_ssh_target(target, normalized_secret.as_deref())?;
                if parsed.secret.is_none() {
                    self.state
                        .set_status(StatusMessage::error("ssh locations require a password"));
                    return Ok(());
                }
                parsed.profile.label = label.to_string();
                self.state
                    .config
                    .sources
                    .ssh
                    .insert(id.clone(), parsed.profile);
                self.secrets
                    .set_ssh_password(&id, parsed.secret)
                    .context("failed to store ssh secret")?;
                self.secrets
                    .set_ssh_key_passphrase(&id, None)
                    .context("failed to clear ssh key passphrase")?;
            }
        }

        self.state.config.save_to_dir(&self.config_dir)?;
        self.secrets.save_to_dir(&self.config_dir)?;
        self.state.add_location = None;
        self.state.mode = InputMode::Normal;
        self.state.set_status(StatusMessage::info(format!(
            "saved {} location: {}",
            dialog.kind.label().to_lowercase(),
            label
        )));
        Ok(())
    }

    fn finish_transfer(
        &mut self,
        dialog: TransferDialogState,
        destination: LocationPath,
    ) -> Result<()> {
        self.state.mode = InputMode::Normal;
        self.state.transfer = None;
        self.refresh_panes()?;
        self.state.set_status(StatusMessage::info(format!(
            "{} {}",
            dialog.operation.past_tense(),
            success_target(&dialog, &destination)
        )));
        Ok(())
    }

    fn resolve_transfer_destination(
        &mut self,
        raw_destination: &str,
        source: &LocationPath,
    ) -> Result<LocationPath> {
        let destination = {
            let dest_pane = self.state.inactive_pane();
            LocationPath::from_input(dest_pane.source.kind, &dest_pane.cwd, raw_destination)
        };

        let is_directory = {
            let dest_pane = self.state.inactive_pane_mut();
            dest_pane.session.exists(&destination)?
                && dest_pane.session.entry_kind(&destination)?.is_directory()
        };

        if is_directory {
            Ok(destination.join_child(&source.file_name().unwrap_or_default()))
        } else {
            Ok(destination)
        }
    }

    fn copy_between_panes(
        &mut self,
        source: &LocationPath,
        destination: &LocationPath,
    ) -> Result<()> {
        let mut temp = NamedTempFile::new().context("failed to create temporary transfer file")?;
        let size = self
            .state
            .active_pane_mut()
            .session
            .copy_file_to_writer(source, temp.as_file_mut())?;
        temp.as_file_mut()
            .seek(SeekFrom::Start(0))
            .context("failed to rewind transfer file")?;
        self.state
            .inactive_pane_mut()
            .session
            .create_file_from_reader(destination, temp.as_file_mut(), size)?;
        Ok(())
    }

    fn refresh_panes(&mut self) -> Result<()> {
        self.state.left.refresh()?;
        self.state.right.refresh()?;
        self.record_history_for_pane(ActivePane::Left)?;
        self.record_history_for_pane(ActivePane::Right)?;
        Ok(())
    }

    fn record_history_for_pane(&mut self, pane: ActivePane) -> Result<()> {
        let pane = self.state.pane(pane);
        self.history
            .record(&pane.source.source_ref, &pane.source.label, &pane.cwd)
    }

    fn preview_up(&mut self) {
        if let Some(preview) = self.state.preview.as_mut() {
            preview.move_up();
        }
    }

    fn preview_down(&mut self) {
        if let Some(preview) = self.state.preview.as_mut() {
            preview.move_down(20);
        }
    }

    fn handle_mouse(&mut self, event: MouseEvent, frame_area: ratatui::layout::Rect) -> Result<()> {
        match self.state.mode {
            InputMode::Normal => match event.kind {
                MouseEventKind::Moved => {
                    self.state.footer_hovered =
                        ui::bottom_bar_hit_target(&self.state, frame_area, event.column, event.row);
                }
                MouseEventKind::Up(MouseButton::Left) => {
                    let hovered =
                        ui::bottom_bar_hit_target(&self.state, frame_area, event.column, event.row);
                    self.state.footer_hovered = hovered;
                    if let Some(action) = hovered.and_then(footer_button_action) {
                        self.handle_action(action)?;
                    }
                }
                _ => {}
            },
            InputMode::Transfer => match event.kind {
                MouseEventKind::Moved => {
                    let hovered = ui::transfer_dialog_hit_target(
                        &self.state,
                        frame_area,
                        event.column,
                        event.row,
                    );
                    if let Some(transfer) = self.state.transfer.as_mut() {
                        transfer.hovered = hovered;
                    }
                }
                MouseEventKind::Up(MouseButton::Left) => {
                    if let Some(control) = ui::transfer_dialog_hit_target(
                        &self.state,
                        frame_area,
                        event.column,
                        event.row,
                    ) {
                        if let Some(transfer) = self.state.transfer.as_mut() {
                            transfer.focus = control;
                            transfer.hovered = Some(control);
                        }
                        if control == TransferControl::ConfirmButton {
                            self.handle_action(Action::SubmitTransfer)?;
                        } else if control == TransferControl::CancelButton {
                            self.handle_action(Action::CancelTransfer)?;
                        }
                    }
                }
                _ => {}
            },
            InputMode::Command
            | InputMode::Preview
            | InputMode::SourceMenu
            | InputMode::AddLocation => {}
        }

        Ok(())
    }

    fn move_transfer_focus_up(&mut self) {
        let Some(transfer) = self.state.transfer.as_mut() else {
            return;
        };

        transfer.focus = match transfer.focus {
            TransferControl::SourceField => TransferControl::SourceField,
            TransferControl::DestinationField => {
                if transfer.operation.shows_source() {
                    TransferControl::SourceField
                } else {
                    TransferControl::DestinationField
                }
            }
            TransferControl::ConfirmButton | TransferControl::CancelButton => {
                TransferControl::DestinationField
            }
        };
        transfer.hovered = Some(transfer.focus);
    }

    fn move_transfer_focus_down(&mut self) {
        let Some(transfer) = self.state.transfer.as_mut() else {
            return;
        };

        transfer.focus = match transfer.focus {
            TransferControl::SourceField => TransferControl::DestinationField,
            TransferControl::DestinationField => TransferControl::ConfirmButton,
            TransferControl::ConfirmButton => TransferControl::ConfirmButton,
            TransferControl::CancelButton => TransferControl::CancelButton,
        };
        transfer.hovered = Some(transfer.focus);
    }

    fn move_transfer_focus_left(&mut self) {
        let Some(transfer) = self.state.transfer.as_mut() else {
            return;
        };

        match transfer.focus {
            TransferControl::DestinationField if transfer.operation.edits_destination() => {
                edit_text(
                    &mut transfer.destination,
                    &mut transfer.cursor,
                    CommandEditAction::MoveCursorLeft,
                );
            }
            TransferControl::CancelButton => transfer.focus = TransferControl::ConfirmButton,
            _ => {}
        }
        transfer.hovered = Some(transfer.focus);
    }

    fn move_transfer_focus_right(&mut self) {
        let Some(transfer) = self.state.transfer.as_mut() else {
            return;
        };

        match transfer.focus {
            TransferControl::DestinationField if transfer.operation.edits_destination() => {
                edit_text(
                    &mut transfer.destination,
                    &mut transfer.cursor,
                    CommandEditAction::MoveCursorRight,
                );
            }
            TransferControl::ConfirmButton => transfer.focus = TransferControl::CancelButton,
            _ => {}
        }
        transfer.hovered = Some(transfer.focus);
    }

    fn move_add_location_focus_up(&mut self) {
        let Some(dialog) = self.state.add_location.as_mut() else {
            return;
        };

        dialog.focus = match dialog.focus {
            AddLocationControl::KindField => AddLocationControl::KindField,
            AddLocationControl::LabelField => AddLocationControl::KindField,
            AddLocationControl::TargetField => AddLocationControl::LabelField,
            AddLocationControl::SecretField => AddLocationControl::TargetField,
            AddLocationControl::ConfirmButton | AddLocationControl::CancelButton => {
                if dialog.kind.uses_secret() {
                    AddLocationControl::SecretField
                } else {
                    AddLocationControl::TargetField
                }
            }
        };
        dialog.hovered = Some(dialog.focus);
    }

    fn move_add_location_focus_down(&mut self) {
        let Some(dialog) = self.state.add_location.as_mut() else {
            return;
        };

        dialog.focus = match dialog.focus {
            AddLocationControl::KindField => AddLocationControl::LabelField,
            AddLocationControl::LabelField => AddLocationControl::TargetField,
            AddLocationControl::TargetField => {
                if dialog.kind.uses_secret() {
                    AddLocationControl::SecretField
                } else {
                    AddLocationControl::ConfirmButton
                }
            }
            AddLocationControl::SecretField => AddLocationControl::ConfirmButton,
            AddLocationControl::ConfirmButton => AddLocationControl::ConfirmButton,
            AddLocationControl::CancelButton => AddLocationControl::CancelButton,
        };
        dialog.hovered = Some(dialog.focus);
    }

    fn move_add_location_focus_left(&mut self) {
        let Some(dialog) = self.state.add_location.as_mut() else {
            return;
        };

        match dialog.focus {
            AddLocationControl::KindField => dialog.kind = dialog.kind.previous(),
            AddLocationControl::LabelField => edit_text(
                &mut dialog.label,
                &mut dialog.label_cursor,
                CommandEditAction::MoveCursorLeft,
            ),
            AddLocationControl::TargetField => edit_text(
                &mut dialog.target,
                &mut dialog.target_cursor,
                CommandEditAction::MoveCursorLeft,
            ),
            AddLocationControl::SecretField if dialog.kind.uses_secret() => edit_text(
                &mut dialog.secret,
                &mut dialog.secret_cursor,
                CommandEditAction::MoveCursorLeft,
            ),
            AddLocationControl::CancelButton => dialog.focus = AddLocationControl::ConfirmButton,
            _ => {}
        }
        dialog.hovered = Some(dialog.focus);
    }

    fn move_add_location_focus_right(&mut self) {
        let Some(dialog) = self.state.add_location.as_mut() else {
            return;
        };

        match dialog.focus {
            AddLocationControl::KindField => dialog.kind = dialog.kind.next(),
            AddLocationControl::LabelField => edit_text(
                &mut dialog.label,
                &mut dialog.label_cursor,
                CommandEditAction::MoveCursorRight,
            ),
            AddLocationControl::TargetField => edit_text(
                &mut dialog.target,
                &mut dialog.target_cursor,
                CommandEditAction::MoveCursorRight,
            ),
            AddLocationControl::SecretField if dialog.kind.uses_secret() => edit_text(
                &mut dialog.secret,
                &mut dialog.secret_cursor,
                CommandEditAction::MoveCursorRight,
            ),
            AddLocationControl::ConfirmButton => dialog.focus = AddLocationControl::CancelButton,
            _ => {}
        }
        dialog.hovered = Some(dialog.focus);
    }
}

fn history_entry_to_menu_entry(config: &Config, entry: &HistoryEntry) -> Option<SourceMenuEntry> {
    if let Some(path) = entry.source_key.strip_prefix("inline:") {
        return Some(SourceMenuEntry {
            source_ref: SourceRef::InlineLocal {
                path: PathBuf::from(path),
                label: entry.label.clone(),
            },
            label: entry.label.clone(),
            path_hint: entry.last_path.clone(),
        });
    }

    let (kind, id) = entry.source_key.split_once(':')?;
    match kind {
        "local" if config.sources.local.contains_key(id) => Some(SourceMenuEntry {
            source_ref: SourceRef::SavedLocal { id: id.to_string() },
            label: entry.label.clone(),
            path_hint: entry.last_path.clone(),
        }),
        "ftp" if config.sources.ftp.contains_key(id) => Some(SourceMenuEntry {
            source_ref: SourceRef::SavedFtp { id: id.to_string() },
            label: entry.label.clone(),
            path_hint: entry.last_path.clone(),
        }),
        "smb" if config.sources.smb.contains_key(id) => Some(SourceMenuEntry {
            source_ref: SourceRef::SavedSmb { id: id.to_string() },
            label: entry.label.clone(),
            path_hint: entry.last_path.clone(),
        }),
        "ssh" if config.sources.ssh.contains_key(id) => Some(SourceMenuEntry {
            source_ref: SourceRef::SavedSsh { id: id.to_string() },
            label: entry.label.clone(),
            path_hint: entry.last_path.clone(),
        }),
        _ => None,
    }
}

fn footer_button_action(slot: usize) -> Option<Action> {
    match slot {
        2 => Some(Action::BeginPreview),
        3 => Some(Action::BeginAddLocation),
        4 => Some(Action::BeginCopy),
        5 => Some(Action::BeginMove),
        6 => Some(Action::BeginCreateDirectory),
        7 => Some(Action::BeginDelete),
        _ => None,
    }
}

fn resolve_start_dirs(
    left_start_dir: Option<PathBuf>,
    right_start_dir: Option<PathBuf>,
) -> Result<(PathBuf, PathBuf)> {
    let default_dir = resolve_default_start_dir()?;
    let left_dir = match left_start_dir {
        Some(path) => resolve_explicit_start_dir(path)?,
        None => default_dir.clone(),
    };
    let right_dir = match right_start_dir {
        Some(path) => resolve_explicit_start_dir(path)?,
        None => default_dir,
    };

    Ok((left_dir, right_dir))
}

fn resolve_explicit_start_dir(start_dir: PathBuf) -> Result<PathBuf> {
    let resolved = if start_dir.is_absolute() {
        start_dir
    } else {
        std::env::current_dir()
            .context("failed to resolve relative startup directory")?
            .join(start_dir)
    };

    if !resolved.is_dir() {
        bail!("not a directory: {}", resolved.display());
    }

    Ok(resolved)
}

fn resolve_default_start_dir() -> Result<PathBuf> {
    match std::env::current_dir() {
        Ok(dir) => Ok(dir),
        Err(current_dir_error) => {
            let base_dirs = BaseDirs::new().context("failed to resolve fallback home directory")?;
            let home = base_dirs.home_dir().to_path_buf();
            if Path::new(&home).exists() {
                Ok(home)
            } else {
                Err(current_dir_error).context("failed to resolve startup directory")
            }
        }
    }
}

fn previous_char_boundary(text: &str, index: usize) -> usize {
    let mut last = 0;
    for (offset, _) in text[..index].char_indices() {
        last = offset;
    }
    last
}

fn next_char_boundary(text: &str, index: usize) -> usize {
    text[index..]
        .char_indices()
        .nth(1)
        .map(|(offset, _)| index + offset)
        .unwrap_or(text.len())
}

fn edit_text(buffer: &mut String, cursor: &mut usize, edit: CommandEditAction) {
    match edit {
        CommandEditAction::Insert(ch) => {
            buffer.insert(*cursor, ch);
            *cursor += ch.len_utf8();
        }
        CommandEditAction::Backspace => {
            if *cursor > 0 {
                let remove_at = previous_char_boundary(buffer, *cursor);
                buffer.drain(remove_at..*cursor);
                *cursor = remove_at;
            }
        }
        CommandEditAction::MoveCursorLeft => {
            if *cursor > 0 {
                *cursor = previous_char_boundary(buffer, *cursor);
            }
        }
        CommandEditAction::MoveCursorRight => {
            if *cursor < buffer.len() {
                *cursor = next_char_boundary(buffer, *cursor);
            }
        }
    }
}

struct ParsedRemoteProfile<T> {
    profile: T,
    secret: Option<String>,
}

fn normalize_optional_secret(secret: &str) -> Option<String> {
    let trimmed = secret.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn resolve_saved_local_path(cwd: LocationPath, raw: &str) -> Result<PathBuf> {
    let candidate = PathBuf::from(raw);
    let resolved = if candidate.is_absolute() {
        candidate
    } else {
        match cwd {
            LocationPath::Local(path) => path.join(candidate),
            LocationPath::Remote(_) => std::env::current_dir()
                .context("failed to resolve relative saved location path")?
                .join(candidate),
        }
    };

    if !resolved.is_dir() {
        bail!("not a directory: {}", resolved.display());
    }

    Ok(resolved)
}

fn parse_ftp_target(
    target: &str,
    secret_override: Option<&str>,
) -> Result<ParsedRemoteProfile<FtpSourceProfile>> {
    let url = parse_target_url(target, "ftp")?;
    let username = remote_username(&url)?;
    let host = remote_host(&url)?;
    let initial_path = remote_initial_path(&url);

    Ok(ParsedRemoteProfile {
        profile: FtpSourceProfile {
            label: String::new(),
            host,
            port: url.port().unwrap_or(21),
            username,
            initial_path,
        },
        secret: secret_override
            .map(ToOwned::to_owned)
            .or_else(|| url.password().map(ToOwned::to_owned)),
    })
}

fn parse_smb_target(
    target: &str,
    secret_override: Option<&str>,
) -> Result<ParsedRemoteProfile<SmbSourceProfile>> {
    let url = parse_target_url(target, "smb")?;
    let username = remote_username(&url)?;
    let server = remote_host(&url)?;
    let mut segments = url
        .path_segments()
        .with_context(|| format!("invalid smb target: {target}"))?;
    let share = segments
        .next()
        .filter(|segment| !segment.is_empty())
        .with_context(|| "smb target must include a share name")?;
    let remainder: Vec<_> = segments.filter(|segment| !segment.is_empty()).collect();
    let initial_path = if remainder.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", remainder.join("/"))
    };
    let workgroup = url
        .query_pairs()
        .find_map(|(key, value)| (key == "workgroup").then(|| value.into_owned()));

    Ok(ParsedRemoteProfile {
        profile: SmbSourceProfile {
            label: String::new(),
            server,
            share: format!("/{share}"),
            username,
            workgroup,
            initial_path,
        },
        secret: secret_override
            .map(ToOwned::to_owned)
            .or_else(|| url.password().map(ToOwned::to_owned)),
    })
}

fn parse_ssh_target(
    target: &str,
    secret_override: Option<&str>,
) -> Result<ParsedRemoteProfile<SshSourceProfile>> {
    let url = parse_target_url(target, "ssh")?;
    let username = remote_username(&url)?;
    let host = remote_host(&url)?;
    let initial_path = remote_initial_path(&url);

    Ok(ParsedRemoteProfile {
        profile: SshSourceProfile {
            label: String::new(),
            host,
            port: url.port().unwrap_or(22),
            username,
            initial_path,
            auth: SshAuthMethod::Password,
            key_path: None,
        },
        secret: secret_override
            .map(ToOwned::to_owned)
            .or_else(|| url.password().map(ToOwned::to_owned)),
    })
}

fn parse_target_url(target: &str, expected_scheme: &str) -> Result<Url> {
    let url = Url::parse(target).with_context(|| format!("invalid target: {target}"))?;
    if url.scheme() != expected_scheme {
        bail!("expected {expected_scheme} target");
    }
    Ok(url)
}

fn remote_username(url: &Url) -> Result<String> {
    if url.username().is_empty() {
        bail!("target must include a username");
    }
    Ok(url.username().to_string())
}

fn remote_host(url: &Url) -> Result<String> {
    url.host_str()
        .map(ToOwned::to_owned)
        .context("target must include a host")
}

fn remote_initial_path(url: &Url) -> String {
    let path = url.path();
    if path.is_empty() {
        "/".to_string()
    } else {
        path.to_string()
    }
}

fn unique_profile_id(label: &str, config: &Config) -> String {
    let base = slugify(label);
    let mut candidate = base.clone();
    let mut suffix = 2;
    while profile_id_exists(&candidate, config) {
        candidate = format!("{base}-{suffix}");
        suffix += 1;
    }
    candidate
}

fn slugify(label: &str) -> String {
    let mut slug = String::new();
    let mut last_was_dash = false;

    for ch in label.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_was_dash = false;
        } else if !last_was_dash {
            slug.push('-');
            last_was_dash = true;
        }
    }

    let trimmed = slug.trim_matches('-');
    if trimmed.is_empty() {
        "location".to_string()
    } else {
        trimmed.to_string()
    }
}

fn profile_id_exists(id: &str, config: &Config) -> bool {
    config.sources.local.contains_key(id)
        || config.sources.ftp.contains_key(id)
        || config.sources.smb.contains_key(id)
        || config.sources.ssh.contains_key(id)
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| path.display().to_string())
}

fn success_target(dialog: &TransferDialogState, destination: &LocationPath) -> String {
    match dialog.operation {
        TransferOperation::Copy | TransferOperation::Move => format!(
            "{} to {}",
            dialog
                .source
                .file_name()
                .unwrap_or_else(|| dialog.source.display()),
            destination.display()
        ),
        TransferOperation::CreateDirectory => destination.display(),
        TransferOperation::Delete => dialog
            .source
            .file_name()
            .unwrap_or_else(|| dialog.source.display()),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use tempfile::TempDir;

    use crate::app::App;
    use crate::config::{
        Config, FtpSourceProfile, LocalSourceProfile, SmbSourceProfile, SshAuthMethod,
        SshSourceProfile,
    };
    use crate::history::HistoryStore;
    use crate::input::{Action, CommandEditAction};
    use crate::secrets::{PlaintextSecretStore, SecretStore};
    use crate::source::{LocationPath, SourceKind, SourceRef};
    use crate::state::{ActivePane, InputMode, SourceMenuLevel, TransferOperation};
    use crate::test_support::cwd_lock;
    use crate::vfs::MockSessionFactory;

    #[test]
    fn startup_uses_current_directory_in_both_panes() {
        let _guard = cwd_lock();
        let temp = TempDir::new().expect("temp dir");
        let previous = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(temp.path()).expect("set cwd");

        let app = App::new_with_services(
            Config::default(),
            None,
            None,
            Box::new(MockSessionFactory::default()),
            Box::new(PlaintextSecretStore::default()),
            Box::new(crate::history::TomlHistoryStore::default()),
        )
        .expect("app");

        assert_eq!(
            app.state.left.cwd,
            LocationPath::Local(temp.path().to_path_buf())
        );
        assert_eq!(
            app.state.right.cwd,
            LocationPath::Local(temp.path().to_path_buf())
        );

        std::env::set_current_dir(previous).expect("restore cwd");
    }

    #[test]
    fn source_menu_opens_and_lists_categories() {
        let _guard = cwd_lock();
        let temp = TempDir::new().expect("temp dir");
        let mut app = App::new_with_factory(
            Config::default(),
            Some(temp.path().to_path_buf()),
            None,
            Box::new(MockSessionFactory::default()),
        )
        .expect("app");

        app.handle_action(Action::OpenLeftSourceMenu)
            .expect("open menu");

        assert_eq!(app.state.mode, InputMode::SourceMenu);
        assert!(matches!(
            app.state.source_menu.as_ref().expect("menu").level,
            SourceMenuLevel::Categories
        ));
    }

    #[test]
    fn copy_dialog_defaults_destination_to_other_pane_and_copies_file() {
        let _guard = cwd_lock();
        let temp = TempDir::new().expect("temp dir");
        let left_dir = temp.path().join("left");
        let right_dir = temp.path().join("right");
        fs::create_dir(&left_dir).expect("left dir");
        fs::create_dir(&right_dir).expect("right dir");
        fs::write(left_dir.join("report.txt"), b"report").expect("source file");

        let mut app = App::new_with_factory(
            Config::default(),
            Some(left_dir.clone()),
            Some(right_dir.clone()),
            Box::new(MockSessionFactory::default()),
        )
        .expect("app");

        app.handle_action(Action::BeginCopy).expect("begin copy");

        let dialog = app.state.transfer.clone().expect("transfer dialog");
        assert_eq!(app.state.mode, InputMode::Transfer);
        assert_eq!(dialog.operation, TransferOperation::Copy);
        assert_eq!(dialog.destination, right_dir.display().to_string());

        app.handle_action(Action::SubmitTransfer)
            .expect("submit transfer");

        assert_eq!(app.state.mode, InputMode::Normal);
        assert!(left_dir.join("report.txt").exists());
        assert!(right_dir.join("report.txt").exists());
    }

    #[test]
    fn relative_transfer_destinations_resolve_from_destination_pane() {
        let _guard = cwd_lock();
        let temp = TempDir::new().expect("temp dir");
        let left_dir = temp.path().join("left");
        let right_dir = temp.path().join("right");
        fs::create_dir_all(left_dir.join("sub")).expect("left dir");
        fs::create_dir_all(right_dir.join("dest")).expect("right dir");
        fs::write(left_dir.join("report.txt"), b"report").expect("source file");

        let mut app = App::new_with_factory(
            Config::default(),
            Some(left_dir.clone()),
            Some(right_dir.join("dest")),
            Box::new(MockSessionFactory::default()),
        )
        .expect("app");
        app.state.left.selected = 1;

        app.handle_action(Action::BeginCopy).expect("begin copy");

        let destination_len = app
            .state
            .transfer
            .as_ref()
            .expect("transfer dialog")
            .destination
            .len();
        for _ in 0..destination_len {
            app.handle_action(Action::EditTransfer(CommandEditAction::Backspace))
                .expect("clear destination");
        }
        for ch in "renamed.txt".chars() {
            app.handle_action(Action::EditTransfer(CommandEditAction::Insert(ch)))
                .expect("type destination");
        }
        app.handle_action(Action::SubmitTransfer)
            .expect("submit transfer");

        assert!(right_dir.join("dest/renamed.txt").exists());
    }

    #[test]
    fn can_switch_to_mock_remote_sources_and_restore_history_path() {
        let _guard = cwd_lock();
        let temp = TempDir::new().expect("temp dir");
        fs::create_dir_all(temp.path().join("incoming")).expect("incoming dir");
        fs::write(temp.path().join("incoming/readme.txt"), b"hello").expect("file");

        let mut config = Config::default();
        config.sources.ftp.insert(
            "archive".to_string(),
            FtpSourceProfile {
                label: "Archive".to_string(),
                host: "ftp.example.com".to_string(),
                port: 21,
                username: "alice".to_string(),
                initial_path: "/".to_string(),
            },
        );
        config.sources.local.insert(
            "home".to_string(),
            LocalSourceProfile {
                label: "Home".to_string(),
                path: temp.path().to_path_buf(),
            },
        );
        config.sources.smb.insert(
            "media".to_string(),
            SmbSourceProfile {
                label: "Media".to_string(),
                server: "smb://nas".to_string(),
                share: "/media".to_string(),
                username: "alice".to_string(),
                workgroup: None,
                initial_path: "/".to_string(),
            },
        );
        config.sources.ssh.insert(
            "prod".to_string(),
            SshSourceProfile {
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
            SourceRef::SavedFtp {
                id: "archive".to_string(),
            },
            SourceKind::Ftp,
            "Archive",
            temp.path().to_path_buf(),
        );

        let mut history = crate::history::TomlHistoryStore::default();
        history
            .record(
                &SourceRef::SavedFtp {
                    id: "archive".to_string(),
                },
                "Archive",
                &LocationPath::Remote("/incoming".to_string()),
            )
            .expect("record");

        let mut app = App::new_with_services(
            config,
            Some(temp.path().to_path_buf()),
            None,
            Box::new(factory),
            Box::new(PlaintextSecretStore::default()),
            Box::new(history),
        )
        .expect("app");
        app.switch_pane_source(
            ActivePane::Left,
            SourceRef::SavedFtp {
                id: "archive".to_string(),
            },
            None,
        )
        .expect("switch source");

        assert_eq!(
            app.state.left.cwd,
            LocationPath::Remote("/incoming".to_string())
        );
        assert!(app.state.left.title().contains("ftp: Archive | /incoming"));
    }

    #[test]
    fn cross_source_copy_and_move_work_with_mock_remotes() {
        let _guard = cwd_lock();
        let local = TempDir::new().expect("local");
        let remote = TempDir::new().expect("remote");
        fs::write(local.path().join("report.txt"), b"report").expect("local file");

        let mut config = Config::default();
        config.sources.ssh.insert(
            "prod".to_string(),
            SshSourceProfile {
                label: "Prod".to_string(),
                host: "prod".to_string(),
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
            remote.path().to_path_buf(),
        );

        let mut app = App::new_with_factory(
            config,
            Some(local.path().to_path_buf()),
            Some(local.path().to_path_buf()),
            Box::new(factory),
        )
        .expect("app");
        app.switch_pane_source(
            ActivePane::Right,
            SourceRef::SavedSsh {
                id: "prod".to_string(),
            },
            Some(LocationPath::Remote("/".to_string())),
        )
        .expect("switch right");
        app.handle_action(Action::BeginCopy).expect("begin copy");
        app.handle_action(Action::SubmitTransfer)
            .expect("submit copy");
        assert!(remote.path().join("report.txt").exists());

        app.handle_action(Action::BeginMove).expect("begin move");
        app.handle_action(Action::SubmitTransfer)
            .expect("submit move");
        assert!(!local.path().join("report.txt").exists());
    }

    #[test]
    fn remote_preview_reads_through_mock_session() {
        let _guard = cwd_lock();
        let remote = TempDir::new().expect("remote");
        fs::write(remote.path().join("note.txt"), b"hello\nworld").expect("file");

        let mut config = Config::default();
        config.sources.ftp.insert(
            "archive".to_string(),
            FtpSourceProfile {
                label: "Archive".to_string(),
                host: "ftp".to_string(),
                port: 21,
                username: "alice".to_string(),
                initial_path: "/".to_string(),
            },
        );

        let factory = MockSessionFactory::default();
        factory.add_remote(
            SourceRef::SavedFtp {
                id: "archive".to_string(),
            },
            SourceKind::Ftp,
            "Archive",
            remote.path().to_path_buf(),
        );

        let mut app = App::new_with_factory(
            config,
            Some(remote.path().to_path_buf()),
            None,
            Box::new(factory),
        )
        .expect("app");
        app.switch_pane_source(
            ActivePane::Left,
            SourceRef::SavedFtp {
                id: "archive".to_string(),
            },
            Some(LocationPath::Remote("/".to_string())),
        )
        .expect("switch source");
        app.handle_action(Action::BeginPreview).expect("preview");

        assert_eq!(app.state.mode, InputMode::Preview);
        assert_eq!(
            app.state.preview.as_ref().expect("preview").lines,
            vec!["hello".to_string(), "world".to_string()]
        );
        assert_eq!(PathBuf::from("ok").display().to_string(), "ok");
    }

    #[test]
    fn add_location_dialog_saves_local_profile_to_config_dir() {
        let _guard = cwd_lock();
        let workspace = TempDir::new().expect("workspace");
        let config_home = TempDir::new().expect("config home");
        let target_dir = workspace.path().join("saved-place");
        fs::create_dir(&target_dir).expect("target dir");

        let mut app = App::new_with_services_at_dir(
            Config::default(),
            Some(workspace.path().to_path_buf()),
            None,
            Box::new(MockSessionFactory::default()),
            Box::new(PlaintextSecretStore::default()),
            Box::new(crate::history::TomlHistoryStore::default()),
            config_home.path().to_path_buf(),
        )
        .expect("app");

        app.handle_action(Action::BeginAddLocation)
            .expect("open add location");
        app.handle_action(Action::AddLocationFocusDown)
            .expect("focus label");
        for ch in "Saved Place".chars() {
            app.handle_action(Action::EditAddLocation(CommandEditAction::Insert(ch)))
                .expect("type label");
        }
        app.handle_action(Action::AddLocationFocusDown)
            .expect("focus target");
        for ch in target_dir.display().to_string().chars() {
            app.handle_action(Action::EditAddLocation(CommandEditAction::Insert(ch)))
                .expect("type target");
        }
        app.handle_action(Action::SubmitAddLocation)
            .expect("save location");

        assert_eq!(app.state.mode, InputMode::Normal);
        assert!(app.state.config.sources.local.contains_key("saved-place"));

        let reloaded = Config::load_from_dir(config_home.path()).expect("reload config");
        assert_eq!(reloaded.sources.local["saved-place"].label, "Saved Place");
        assert_eq!(reloaded.sources.local["saved-place"].path, target_dir);
    }

    #[test]
    fn add_location_dialog_saves_ftp_profile_and_secret() {
        let _guard = cwd_lock();
        let workspace = TempDir::new().expect("workspace");
        let config_home = TempDir::new().expect("config home");

        let mut app = App::new_with_services_at_dir(
            Config::default(),
            Some(workspace.path().to_path_buf()),
            None,
            Box::new(MockSessionFactory::default()),
            Box::new(PlaintextSecretStore::default()),
            Box::new(crate::history::TomlHistoryStore::default()),
            config_home.path().to_path_buf(),
        )
        .expect("app");

        app.handle_action(Action::BeginAddLocation)
            .expect("open add location");
        app.handle_action(Action::AddLocationFocusRight)
            .expect("select ftp");
        app.handle_action(Action::AddLocationFocusDown)
            .expect("focus label");
        for ch in "Archive".chars() {
            app.handle_action(Action::EditAddLocation(CommandEditAction::Insert(ch)))
                .expect("type label");
        }
        app.handle_action(Action::AddLocationFocusDown)
            .expect("focus target");
        for ch in "ftp://alice@ftp.example.com/incoming".chars() {
            app.handle_action(Action::EditAddLocation(CommandEditAction::Insert(ch)))
                .expect("type target");
        }
        app.handle_action(Action::AddLocationFocusDown)
            .expect("focus secret");
        for ch in "ftp-pass".chars() {
            app.handle_action(Action::EditAddLocation(CommandEditAction::Insert(ch)))
                .expect("type secret");
        }
        app.handle_action(Action::SubmitAddLocation)
            .expect("save location");

        let reloaded = Config::load_from_dir(config_home.path()).expect("reload config");
        let secrets = PlaintextSecretStore::load_from_dir(config_home.path()).expect("secrets");
        assert_eq!(reloaded.sources.ftp["archive"].host, "ftp.example.com");
        assert_eq!(reloaded.sources.ftp["archive"].username, "alice");
        assert_eq!(reloaded.sources.ftp["archive"].initial_path, "/incoming");
        assert_eq!(secrets.ftp_password("archive"), Some("ftp-pass"));
    }
}
