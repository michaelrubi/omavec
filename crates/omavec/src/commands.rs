//! Everything the user can do is a `Command`, so menus, shortcuts, the
//! command palette, `OMAVEC_SCRIPT` and the CLI can't drift apart.

use egui::{Key, KeyboardShortcut, Modifiers};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Quit,
    ToggleUi,
    ZoomIn,
    ZoomOut,
    ZoomTo100,
    ToggleRulers,
    TogglePixelGrid,
}

impl Command {
    pub const ALL: [Command; 7] = [
        Command::Quit,
        Command::ToggleUi,
        Command::ZoomIn,
        Command::ZoomOut,
        Command::ZoomTo100,
        Command::ToggleRulers,
        Command::TogglePixelGrid,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Command::Quit => "Quit",
            Command::ToggleUi => "Show/Hide UI",
            Command::ZoomIn => "Zoom In",
            Command::ZoomOut => "Zoom Out",
            Command::ZoomTo100 => "Zoom to 100%",
            Command::ToggleRulers => "Rulers",
            Command::TogglePixelGrid => "Pixel Grid",
        }
    }

    /// Figma's shortcut for the command, where it has one.
    pub fn shortcut(self) -> Option<KeyboardShortcut> {
        let (modifiers, key) = match self {
            Command::Quit => (Modifiers::COMMAND, Key::Q),
            Command::ToggleUi => (Modifiers::COMMAND, Key::Backslash),
            Command::ZoomIn => (Modifiers::COMMAND, Key::Equals),
            Command::ZoomOut => (Modifiers::COMMAND, Key::Minus),
            Command::ZoomTo100 => (Modifiers::SHIFT, Key::Num0),
            Command::ToggleRulers => (Modifiers::SHIFT, Key::R),
            Command::TogglePixelGrid => (Modifiers::SHIFT, Key::Quote),
        };
        Some(KeyboardShortcut::new(modifiers, key))
    }

    /// Commands whose shortcut was pressed this frame, consuming the keys.
    pub fn pressed(ctx: &egui::Context) -> Vec<Command> {
        ctx.input_mut(|i| {
            Self::ALL
                .into_iter()
                .filter(|c| c.shortcut().is_some_and(|s| i.consume_shortcut(&s)))
                .collect()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_two_commands_share_a_shortcut() {
        let shortcuts: Vec<_> = Command::ALL.iter().filter_map(|c| c.shortcut()).collect();
        for (i, a) in shortcuts.iter().enumerate() {
            assert!(!shortcuts[i + 1..].contains(a), "{a:?} is bound twice");
        }
    }
}
