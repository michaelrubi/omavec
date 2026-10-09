//! Everything the user can do is a `Command`, so menus, shortcuts, the
//! command palette, `OMAVEC_SCRIPT` and the CLI can't drift apart.

use egui::{Key, KeyboardShortcut, Modifiers};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    New,
    Open,
    Save,
    SaveAs,
    Quit,
    Undo,
    Redo,
    Delete,
    Cancel,
    MoveTool,
    FrameTool,
    RectangleTool,
    EllipseTool,
    ZoomIn,
    ZoomOut,
    ZoomTo100,
    ToggleRulers,
    TogglePixelGrid,
    ToggleUi,
}

impl Command {
    pub const ALL: [Command; 19] = [
        Command::New,
        Command::Open,
        Command::Save,
        Command::SaveAs,
        Command::Quit,
        Command::Undo,
        Command::Redo,
        Command::Delete,
        Command::Cancel,
        Command::MoveTool,
        Command::FrameTool,
        Command::RectangleTool,
        Command::EllipseTool,
        Command::ZoomIn,
        Command::ZoomOut,
        Command::ZoomTo100,
        Command::ToggleRulers,
        Command::TogglePixelGrid,
        Command::ToggleUi,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Command::New => "New",
            Command::Open => "Open…",
            Command::Save => "Save",
            Command::SaveAs => "Save As…",
            Command::Quit => "Quit",
            Command::Undo => "Undo",
            Command::Redo => "Redo",
            Command::Delete => "Delete",
            Command::Cancel => "Cancel",
            Command::MoveTool => "Move",
            Command::FrameTool => "Frame",
            Command::RectangleTool => "Rectangle",
            Command::EllipseTool => "Ellipse",
            Command::ZoomIn => "Zoom In",
            Command::ZoomOut => "Zoom Out",
            Command::ZoomTo100 => "Zoom to 100%",
            Command::ToggleRulers => "Rulers",
            Command::TogglePixelGrid => "Pixel Grid",
            Command::ToggleUi => "Show/Hide UI",
        }
    }

    /// Figma's shortcut for the command, where it has one.
    pub fn shortcut(self) -> Option<KeyboardShortcut> {
        let both = Modifiers::COMMAND | Modifiers::SHIFT;
        let (modifiers, key) = match self {
            Command::New => (Modifiers::COMMAND, Key::N),
            Command::Open => (Modifiers::COMMAND, Key::O),
            Command::Save => (Modifiers::COMMAND, Key::S),
            Command::SaveAs => (both, Key::S),
            Command::Quit => (Modifiers::COMMAND, Key::Q),
            Command::Undo => (Modifiers::COMMAND, Key::Z),
            Command::Redo => (both, Key::Z),
            Command::Delete => (Modifiers::NONE, Key::Delete),
            Command::Cancel => (Modifiers::NONE, Key::Escape),
            Command::MoveTool => (Modifiers::NONE, Key::V),
            Command::FrameTool => (Modifiers::NONE, Key::F),
            Command::RectangleTool => (Modifiers::NONE, Key::R),
            Command::EllipseTool => (Modifiers::NONE, Key::O),
            Command::ZoomIn => (Modifiers::COMMAND, Key::Equals),
            Command::ZoomOut => (Modifiers::COMMAND, Key::Minus),
            Command::ZoomTo100 => (Modifiers::SHIFT, Key::Num0),
            Command::ToggleRulers => (Modifiers::SHIFT, Key::R),
            Command::TogglePixelGrid => (Modifiers::SHIFT, Key::Quote),
            Command::ToggleUi => (Modifiers::COMMAND, Key::Backslash),
        };
        Some(KeyboardShortcut::new(modifiers, key))
    }

    /// Commands whose shortcut was pressed this frame, consuming the keys.
    pub fn pressed(ctx: &egui::Context) -> Vec<Command> {
        // egui lets a shortcut match with more modifiers held than it names,
        // so R would take Shift+R: try the ones with the most modifiers first.
        let weight = |command: &Command| {
            let modifiers = command.shortcut().map(|shortcut| shortcut.modifiers).unwrap_or_default();
            [modifiers.command || modifiers.ctrl, modifiers.shift, modifiers.alt].into_iter().filter(|held| *held).count()
        };
        let mut commands = Self::ALL;
        commands.sort_by_key(|command| std::cmp::Reverse(weight(command)));
        ctx.input_mut(|i| {
            let mut pressed: Vec<Command> = commands.into_iter().filter(|c| c.shortcut().is_some_and(|s| i.consume_shortcut(&s))).collect();
            // Backspace deletes too, as on a laptop with no Delete key.
            if i.consume_key(Modifiers::NONE, Key::Backspace) {
                pressed.push(Command::Delete);
            }
            pressed
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_letter_with_ctrl_or_shift_is_not_the_bare_letter() {
        let pressed = |key, modifiers| {
            let ctx = egui::Context::default();
            let event = egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
            let mut pressed = Vec::new();
            let mut output = ctx.run_ui(egui::RawInput { events: vec![egui::Event::ModifiersChanged(modifiers), event], ..Default::default() }, |ui| {
                pressed = Command::pressed(ui.ctx());
            });
            // There's no renderer to upload textures to.
            output.textures_delta.clear();
            pressed
        };
        assert_eq!(pressed(Key::O, Modifiers::NONE), [Command::EllipseTool]);
        assert_eq!(pressed(Key::O, Modifiers::COMMAND), [Command::Open]);
        assert_eq!(pressed(Key::R, Modifiers::SHIFT), [Command::ToggleRulers]);
        assert_eq!(pressed(Key::S, Modifiers::COMMAND), [Command::Save]);
        assert_eq!(pressed(Key::S, Modifiers::COMMAND | Modifiers::SHIFT), [Command::SaveAs]);
        assert_eq!(pressed(Key::Z, Modifiers::COMMAND | Modifiers::SHIFT), [Command::Redo]);
        assert_eq!(pressed(Key::Backspace, Modifiers::NONE), [Command::Delete]);
        // Ctrl+R is nobody's.
        assert_eq!(pressed(Key::R, Modifiers::COMMAND), []);
    }

    #[test]
    fn no_two_commands_share_a_shortcut() {
        let shortcuts: Vec<_> = Command::ALL.iter().filter_map(|c| c.shortcut()).collect();
        for (i, a) in shortcuts.iter().enumerate() {
            assert!(!shortcuts[i + 1..].contains(a), "{a:?} is bound twice");
        }
    }
}
