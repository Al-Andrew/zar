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
    GoParent,
    BeginCopy,
    BeginMove,
    BeginCreateDirectory,
    BeginDelete,
    EnterCommandMode,
    EditCommand(CommandEditAction),
    EditTransfer(CommandEditAction),
    TransferFocusUp,
    TransferFocusDown,
    TransferFocusLeft,
    TransferFocusRight,
    SubmitCommand,
    SubmitTransfer,
    CancelCommand,
    CancelTransfer,
    Quit,
    ClearStatus,
}

impl KeyBindings {
    pub fn resolve(&self, event: KeyEvent, mode: InputMode) -> Option<Action> {
        match mode {
            InputMode::Normal => self.resolve_normal_mode(event),
            InputMode::Command => self.resolve_command_mode(event),
            InputMode::Transfer => self.resolve_transfer_mode(event),
        }
    }

    fn resolve_normal_mode(&self, event: KeyEvent) -> Option<Action> {
        if event.code == KeyCode::Char('c') && event.modifiers == KeyModifiers::CONTROL {
            return Some(Action::Quit);
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

    use super::Action;

    #[test]
    fn function_keys_open_transfer_dialogs_in_normal_mode() {
        let bindings = KeyBindings::default();

        assert_eq!(
            bindings.resolve(
                KeyEvent::new(KeyCode::F(5), KeyModifiers::NONE),
                InputMode::Normal
            ),
            Some(Action::BeginCopy)
        );
        assert_eq!(
            bindings.resolve(
                KeyEvent::new(KeyCode::F(6), KeyModifiers::NONE),
                InputMode::Normal
            ),
            Some(Action::BeginMove)
        );
        assert_eq!(
            bindings.resolve(
                KeyEvent::new(KeyCode::F(7), KeyModifiers::NONE),
                InputMode::Normal
            ),
            Some(Action::BeginCreateDirectory)
        );
        assert_eq!(
            bindings.resolve(
                KeyEvent::new(KeyCode::F(8), KeyModifiers::NONE),
                InputMode::Normal
            ),
            Some(Action::BeginDelete)
        );
    }

    #[test]
    fn arrow_keys_navigate_transfer_dialog() {
        let bindings = KeyBindings::default();

        assert_eq!(
            bindings.resolve(
                KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
                InputMode::Transfer
            ),
            Some(Action::TransferFocusUp)
        );
        assert_eq!(
            bindings.resolve(
                KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
                InputMode::Transfer
            ),
            Some(Action::TransferFocusRight)
        );
    }
}
