use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

use crate::config::KeyBindings;
use crate::state::InputMode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandEditAction {
    Insert(char),
    Backspace,
    MoveCursorLeft,
    MoveCursorRight,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    MoveUp,
    MoveDown,
    SwitchPane,
    OpenSelection,
    BeginPreview,
    GoParent,
    BeginCopy,
    BeginMove,
    BeginCreateDirectory,
    BeginDelete,
    BeginAddLocation,
    OpenLeftSourceMenu,
    OpenRightSourceMenu,
    SourceMenuBack,
    SourceMenuSelect,
    EnterCommandMode,
    EditCommand(CommandEditAction),
    EditTransfer(CommandEditAction),
    EditAddLocation(CommandEditAction),
    TransferFocusUp,
    TransferFocusDown,
    TransferFocusLeft,
    TransferFocusRight,
    AddLocationFocusUp,
    AddLocationFocusDown,
    AddLocationFocusLeft,
    AddLocationFocusRight,
    PreviewUp,
    PreviewDown,
    SubmitCommand,
    SubmitTransfer,
    SubmitAddLocation,
    CancelCommand,
    CancelTransfer,
    CancelAddLocation,
    ClosePreview,
    CloseSourceMenu,
    Quit,
    ClearStatus,
}

impl KeyBindings {
    pub fn resolve(&self, event: KeyEvent, mode: InputMode) -> Option<Action> {
        match mode {
            InputMode::Normal => self.resolve_normal_mode(event),
            InputMode::Command => self.resolve_command_mode(event),
            InputMode::Transfer => self.resolve_transfer_mode(event),
            InputMode::Preview => self.resolve_preview_mode(event),
            InputMode::SourceMenu => self.resolve_source_menu_mode(event),
            InputMode::AddLocation => self.resolve_add_location_mode(event),
        }
    }

    fn resolve_normal_mode(&self, event: KeyEvent) -> Option<Action> {
        if event.code == KeyCode::Char('c') && event.modifiers == KeyModifiers::CONTROL {
            return Some(Action::Quit);
        }
        if event.code == KeyCode::F(1) && event.modifiers.is_empty() {
            return Some(Action::OpenLeftSourceMenu);
        }
        if event.code == KeyCode::F(2) && event.modifiers.is_empty() {
            return Some(Action::OpenRightSourceMenu);
        }
        if self.enter_command_mode.matches(event) {
            return Some(Action::EnterCommandMode);
        }
        if self.quit.matches(event) {
            return Some(Action::Quit);
        }
        if self.switch_pane.matches(event) {
            return Some(Action::SwitchPane);
        }
        if self.move_up.matches(event) {
            return Some(Action::MoveUp);
        }
        if self.move_down.matches(event) {
            return Some(Action::MoveDown);
        }
        if self.open.matches(event) {
            return Some(Action::OpenSelection);
        }
        if event.code == KeyCode::F(3) && event.modifiers.is_empty() {
            return Some(Action::BeginPreview);
        }
        if event.code == KeyCode::F(4) && event.modifiers.is_empty() {
            return Some(Action::BeginAddLocation);
        }
        if self.parent.matches(event) {
            return Some(Action::GoParent);
        }
        if event.code == KeyCode::F(5) && event.modifiers.is_empty() {
            return Some(Action::BeginCopy);
        }
        if event.code == KeyCode::F(6) && event.modifiers.is_empty() {
            return Some(Action::BeginMove);
        }
        if event.code == KeyCode::F(7) && event.modifiers.is_empty() {
            return Some(Action::BeginCreateDirectory);
        }
        if event.code == KeyCode::F(8) && event.modifiers.is_empty() {
            return Some(Action::BeginDelete);
        }
        if event.code == KeyCode::Esc && event.modifiers.is_empty() {
            return Some(Action::ClearStatus);
        }

        None
    }

    fn resolve_command_mode(&self, event: KeyEvent) -> Option<Action> {
        if event.code == KeyCode::Char('c') && event.modifiers == KeyModifiers::CONTROL {
            return Some(Action::Quit);
        }

        match event.code {
            KeyCode::Esc if event.modifiers.is_empty() => Some(Action::CancelCommand),
            KeyCode::Enter if event.modifiers.is_empty() => Some(Action::SubmitCommand),
            KeyCode::Backspace if event.modifiers.is_empty() => {
                Some(Action::EditCommand(CommandEditAction::Backspace))
            }
            KeyCode::Left if event.modifiers.is_empty() => {
                Some(Action::EditCommand(CommandEditAction::MoveCursorLeft))
            }
            KeyCode::Right if event.modifiers.is_empty() => {
                Some(Action::EditCommand(CommandEditAction::MoveCursorRight))
            }
            KeyCode::Char(ch)
                if event
                    .modifiers
                    .intersection(KeyModifiers::CONTROL | KeyModifiers::ALT)
                    .is_empty() =>
            {
                Some(Action::EditCommand(CommandEditAction::Insert(ch)))
            }
            _ => None,
        }
    }

    fn resolve_transfer_mode(&self, event: KeyEvent) -> Option<Action> {
        if event.code == KeyCode::Char('c') && event.modifiers == KeyModifiers::CONTROL {
            return Some(Action::Quit);
        }

        match event.code {
            KeyCode::Esc if event.modifiers.is_empty() => Some(Action::CancelTransfer),
            KeyCode::Enter if event.modifiers.is_empty() => Some(Action::SubmitTransfer),
            KeyCode::Up if event.modifiers.is_empty() => Some(Action::TransferFocusUp),
            KeyCode::Down if event.modifiers.is_empty() => Some(Action::TransferFocusDown),
            KeyCode::Left if event.modifiers.is_empty() => Some(Action::TransferFocusLeft),
            KeyCode::Right if event.modifiers.is_empty() => Some(Action::TransferFocusRight),
            KeyCode::Backspace if event.modifiers.is_empty() => {
                Some(Action::EditTransfer(CommandEditAction::Backspace))
            }
            KeyCode::Char(ch)
                if event
                    .modifiers
                    .intersection(KeyModifiers::CONTROL | KeyModifiers::ALT)
                    .is_empty() =>
            {
                Some(Action::EditTransfer(CommandEditAction::Insert(ch)))
            }
            _ => None,
        }
    }

    fn resolve_preview_mode(&self, event: KeyEvent) -> Option<Action> {
        if event.code == KeyCode::Char('c') && event.modifiers == KeyModifiers::CONTROL {
            return Some(Action::Quit);
        }
        if self.quit.matches(event) {
            return Some(Action::Quit);
        }

        match event.code {
            KeyCode::Esc if event.modifiers.is_empty() => Some(Action::ClosePreview),
            KeyCode::F(3) if event.modifiers.is_empty() => Some(Action::ClosePreview),
            KeyCode::Up if event.modifiers.is_empty() => Some(Action::PreviewUp),
            KeyCode::Down if event.modifiers.is_empty() => Some(Action::PreviewDown),
            _ => None,
        }
    }

    fn resolve_source_menu_mode(&self, event: KeyEvent) -> Option<Action> {
        if event.code == KeyCode::Char('c') && event.modifiers == KeyModifiers::CONTROL {
            return Some(Action::Quit);
        }

        match event.code {
            KeyCode::Esc if event.modifiers.is_empty() => Some(Action::CloseSourceMenu),
            KeyCode::Enter if event.modifiers.is_empty() => Some(Action::SourceMenuSelect),
            KeyCode::Up if event.modifiers.is_empty() => Some(Action::MoveUp),
            KeyCode::Down if event.modifiers.is_empty() => Some(Action::MoveDown),
            KeyCode::Left if event.modifiers.is_empty() => Some(Action::SourceMenuBack),
            KeyCode::Backspace if event.modifiers.is_empty() => Some(Action::SourceMenuBack),
            _ => None,
        }
    }

    fn resolve_add_location_mode(&self, event: KeyEvent) -> Option<Action> {
        if event.code == KeyCode::Char('c') && event.modifiers == KeyModifiers::CONTROL {
            return Some(Action::Quit);
        }

        match event.code {
            KeyCode::Esc if event.modifiers.is_empty() => Some(Action::CancelAddLocation),
            KeyCode::Enter if event.modifiers.is_empty() => Some(Action::SubmitAddLocation),
            KeyCode::Up if event.modifiers.is_empty() => Some(Action::AddLocationFocusUp),
            KeyCode::Down if event.modifiers.is_empty() => Some(Action::AddLocationFocusDown),
            KeyCode::Left if event.modifiers.is_empty() => Some(Action::AddLocationFocusLeft),
            KeyCode::Right if event.modifiers.is_empty() => Some(Action::AddLocationFocusRight),
            KeyCode::Backspace if event.modifiers.is_empty() => {
                Some(Action::EditAddLocation(CommandEditAction::Backspace))
            }
            KeyCode::Char(ch)
                if event
                    .modifiers
                    .intersection(KeyModifiers::CONTROL | KeyModifiers::ALT)
                    .is_empty() =>
            {
                Some(Action::EditAddLocation(CommandEditAction::Insert(ch)))
            }
            _ => None,
        }
    }
}

pub fn event_to_action(bindings: &KeyBindings, mode: InputMode, event: Event) -> Option<Action> {
    match event {
        Event::Key(key_event) => bindings.resolve(key_event, mode),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use crate::config::KeyBindings;
    use crate::state::InputMode;

    use super::{Action, CommandEditAction};

    #[test]
    fn function_keys_open_transfer_dialogs_in_normal_mode() {
        let bindings = KeyBindings::default();

        assert_eq!(
            bindings.resolve(
                KeyEvent::new(KeyCode::F(3), KeyModifiers::NONE),
                InputMode::Normal
            ),
            Some(Action::BeginPreview)
        );
        assert_eq!(
            bindings.resolve(
                KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE),
                InputMode::Normal
            ),
            Some(Action::OpenLeftSourceMenu)
        );
        assert_eq!(
            bindings.resolve(
                KeyEvent::new(KeyCode::F(4), KeyModifiers::NONE),
                InputMode::Normal
            ),
            Some(Action::BeginAddLocation)
        );
    }

    #[test]
    fn source_menu_maps_navigation_keys() {
        let bindings = KeyBindings::default();

        assert_eq!(
            bindings.resolve(
                KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
                InputMode::SourceMenu
            ),
            Some(Action::SourceMenuBack)
        );
        assert_eq!(
            bindings.resolve(
                KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                InputMode::SourceMenu
            ),
            Some(Action::SourceMenuSelect)
        );
    }

    #[test]
    fn add_location_mode_maps_editing_keys() {
        let bindings = KeyBindings::default();

        assert_eq!(
            bindings.resolve(
                KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
                InputMode::AddLocation
            ),
            Some(Action::AddLocationFocusRight)
        );
        assert_eq!(
            bindings.resolve(
                KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
                InputMode::AddLocation
            ),
            Some(Action::EditAddLocation(CommandEditAction::Insert('x')))
        );
    }
}
