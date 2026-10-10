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
    Cut,
    Copy,
    Paste,
    Duplicate,
    SelectAll,
    SelectChildren,
    SelectParent,
    Group,
    Ungroup,
    FrameSelection,
    BringToFront,
    BringForward,
    SendBackward,
    SendToBack,
    Cancel,
    NudgeLeft,
    NudgeRight,
    NudgeUp,
    NudgeDown,
    MoveTool,
    HandTool,
    FrameTool,
    RectangleTool,
    EllipseTool,
    PolygonTool,
    StarTool,
    LineTool,
    ArrowTool,
    ZoomIn,
    ZoomOut,
    ZoomTo100,
    ZoomToFit,
    ZoomToSelection,
    ToggleRulers,
    TogglePixelGrid,
    ToggleUi,
    ToggleVisible,
    ToggleLocked,
}

impl Command {
    pub const ALL: [Command; 46] = [
        Command::New,
        Command::Open,
        Command::Save,
        Command::SaveAs,
        Command::Quit,
        Command::Undo,
        Command::Redo,
        Command::Delete,
        Command::Cut,
        Command::Copy,
        Command::Paste,
        Command::Duplicate,
        Command::SelectAll,
        Command::SelectChildren,
        Command::SelectParent,
        Command::Group,
        Command::Ungroup,
        Command::FrameSelection,
        Command::BringToFront,
        Command::BringForward,
        Command::SendBackward,
        Command::SendToBack,
        Command::Cancel,
        Command::NudgeLeft,
        Command::NudgeRight,
        Command::NudgeUp,
        Command::NudgeDown,
        Command::MoveTool,
        Command::HandTool,
        Command::FrameTool,
        Command::RectangleTool,
        Command::EllipseTool,
        Command::PolygonTool,
        Command::StarTool,
        Command::LineTool,
        Command::ArrowTool,
        Command::ZoomIn,
        Command::ZoomOut,
        Command::ZoomTo100,
        Command::ZoomToFit,
        Command::ZoomToSelection,
        Command::ToggleRulers,
        Command::TogglePixelGrid,
        Command::ToggleUi,
        Command::ToggleVisible,
        Command::ToggleLocked,
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
            Command::Cut => "Cut",
            Command::Copy => "Copy",
            Command::Paste => "Paste",
            Command::Duplicate => "Duplicate",
            Command::SelectAll => "Select All",
            Command::SelectChildren => "Select Children",
            Command::SelectParent => "Select Parent",
            Command::Group => "Group Selection",
            Command::Ungroup => "Ungroup Selection",
            Command::FrameSelection => "Frame Selection",
            Command::BringToFront => "Bring to Front",
            Command::BringForward => "Bring Forward",
            Command::SendBackward => "Send Backward",
            Command::SendToBack => "Send to Back",
            Command::Cancel => "Cancel",
            Command::NudgeLeft => "Nudge Left",
            Command::NudgeRight => "Nudge Right",
            Command::NudgeUp => "Nudge Up",
            Command::NudgeDown => "Nudge Down",
            Command::MoveTool => "Move",
            Command::HandTool => "Hand",
            Command::FrameTool => "Frame",
            Command::RectangleTool => "Rectangle",
            Command::EllipseTool => "Ellipse",
            Command::PolygonTool => "Polygon",
            Command::StarTool => "Star",
            Command::LineTool => "Line",
            Command::ArrowTool => "Arrow",
            Command::ZoomIn => "Zoom In",
            Command::ZoomOut => "Zoom Out",
            Command::ZoomTo100 => "Zoom to 100%",
            Command::ZoomToFit => "Zoom to Fit",
            Command::ZoomToSelection => "Zoom to Selection",
            Command::ToggleRulers => "Rulers",
            Command::TogglePixelGrid => "Pixel Grid",
            Command::ToggleUi => "Show/Hide UI",
            Command::ToggleVisible => "Show/Hide Selection",
            Command::ToggleLocked => "Lock/Unlock Selection",
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
            Command::Cut => (Modifiers::COMMAND, Key::X),
            Command::Copy => (Modifiers::COMMAND, Key::C),
            Command::Paste => (Modifiers::COMMAND, Key::V),
            Command::Duplicate => (Modifiers::COMMAND, Key::D),
            Command::SelectAll => (Modifiers::COMMAND, Key::A),
            Command::SelectChildren => (Modifiers::NONE, Key::Enter),
            Command::SelectParent => (Modifiers::SHIFT, Key::Enter),
            Command::Group => (Modifiers::COMMAND, Key::G),
            Command::Ungroup => (both, Key::G),
            Command::FrameSelection => (Modifiers::COMMAND | Modifiers::ALT, Key::G),
            Command::BringToFront => (Modifiers::NONE, Key::CloseBracket),
            Command::BringForward => (Modifiers::COMMAND, Key::CloseBracket),
            Command::SendBackward => (Modifiers::COMMAND, Key::OpenBracket),
            Command::SendToBack => (Modifiers::NONE, Key::OpenBracket),
            Command::Cancel => (Modifiers::NONE, Key::Escape),
            // With Shift held these still match, and nudge by 10.
            Command::NudgeLeft => (Modifiers::NONE, Key::ArrowLeft),
            Command::NudgeRight => (Modifiers::NONE, Key::ArrowRight),
            Command::NudgeUp => (Modifiers::NONE, Key::ArrowUp),
            Command::NudgeDown => (Modifiers::NONE, Key::ArrowDown),
            Command::MoveTool => (Modifiers::NONE, Key::V),
            Command::HandTool => (Modifiers::NONE, Key::H),
            Command::FrameTool => (Modifiers::NONE, Key::F),
            Command::RectangleTool => (Modifiers::NONE, Key::R),
            Command::EllipseTool => (Modifiers::NONE, Key::O),
            Command::LineTool => (Modifiers::NONE, Key::L),
            Command::ArrowTool => (Modifiers::SHIFT, Key::L),
            // Figma gives these two no key.
            Command::PolygonTool | Command::StarTool => return None,
            Command::ZoomIn => (Modifiers::COMMAND, Key::Equals),
            Command::ZoomOut => (Modifiers::COMMAND, Key::Minus),
            Command::ZoomTo100 => (Modifiers::SHIFT, Key::Num0),
            Command::ZoomToFit => (Modifiers::SHIFT, Key::Num1),
            Command::ZoomToSelection => (Modifiers::SHIFT, Key::Num2),
            Command::ToggleRulers => (Modifiers::SHIFT, Key::R),
            Command::TogglePixelGrid => (Modifiers::SHIFT, Key::Quote),
            Command::ToggleUi => (Modifiers::COMMAND, Key::Backslash),
            Command::ToggleVisible => (both, Key::H),
            Command::ToggleLocked => (both, Key::L),
        };
        Some(KeyboardShortcut::new(modifiers, key))
    }

    /// Commands whose shortcut was pressed this frame, consuming the keys.
    /// `v_down` is whether V was down after the last call: the caller keeps it.
    pub fn pressed(ctx: &egui::Context, v_down: &mut bool) -> Vec<Command> {
        // egui lets a shortcut match with more modifiers held than it names,
        // so R would take Shift+R: try the ones with the most modifiers first.
        let weight = |command: &Command| {
            let modifiers = command.shortcut().map(|shortcut| shortcut.modifiers).unwrap_or_default();
            [modifiers.command || modifiers.ctrl, modifiers.shift, modifiers.alt].into_iter().filter(|held| *held).count()
        };
        let mut commands = Self::ALL;
        commands.sort_by_key(|command| std::cmp::Reverse(weight(command)));
        ctx.input_mut(|i| {
            // The window turns Ctrl+C and Ctrl+X into Copy and Cut events
            // instead of key presses, and Ctrl+V into a Paste event only if
            // the clipboard holds text. But the V is still released: a V
            // released that was never pressed was Ctrl+V.
            let mut pressed = Vec::new();
            for event in &i.events {
                match event {
                    egui::Event::Copy => pressed.push(Command::Copy),
                    egui::Event::Cut => pressed.push(Command::Cut),
                    egui::Event::Key { key: Key::V, pressed: down, .. } => {
                        if !*down && !*v_down {
                            pressed.push(Command::Paste);
                        }
                        *v_down = *down;
                    }
                    _ => {}
                }
            }
            pressed.extend(commands.into_iter().filter(|c| c.shortcut().is_some_and(|s| i.consume_shortcut(&s))));
            // Backspace deletes too, as on a laptop with no Delete key.
            if i.consume_key(Modifiers::NONE, Key::Backspace) {
                pressed.push(Command::Delete);
            }
            i.events.retain(|event| !matches!(event, egui::Event::Copy | egui::Event::Cut | egui::Event::Paste(_)));
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
                pressed = Command::pressed(ui.ctx(), &mut false);
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
        assert_eq!(pressed(Key::G, Modifiers::COMMAND | Modifiers::ALT), [Command::FrameSelection]);
        assert_eq!(pressed(Key::G, Modifiers::COMMAND | Modifiers::SHIFT), [Command::Ungroup]);
        assert_eq!(pressed(Key::CloseBracket, Modifiers::COMMAND), [Command::BringForward]);
        assert_eq!(pressed(Key::CloseBracket, Modifiers::NONE), [Command::BringToFront]);
        assert_eq!(pressed(Key::Enter, Modifiers::SHIFT), [Command::SelectParent]);
        // Ctrl+R is nobody's.
        assert_eq!(pressed(Key::R, Modifiers::COMMAND), []);
    }

    #[test]
    fn copy_cut_and_paste_come_as_the_window_sends_them() {
        let run = |events: Vec<egui::Event>, v_down: &mut bool| {
            let ctx = egui::Context::default();
            let mut pressed = Vec::new();
            let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| pressed = Command::pressed(ui.ctx(), v_down));
            output.textures_delta.clear();
            pressed
        };
        let v = |pressed, modifiers| egui::Event::Key { key: Key::V, physical_key: None, pressed, repeat: false, modifiers };
        assert_eq!(run(vec![egui::Event::Copy], &mut false), [Command::Copy]);
        assert_eq!(run(vec![egui::Event::Cut], &mut false), [Command::Cut]);
        // Ctrl+V: the press is swallowed, and only the release arrives.
        assert_eq!(run(vec![v(false, Modifiers::COMMAND)], &mut false), [Command::Paste]);
        assert_eq!(run(vec![egui::Event::Paste("text".into()), v(false, Modifiers::COMMAND)], &mut false), [Command::Paste]);
        // A bare V is the Move tool, and its release pastes nothing.
        let mut v_down = false;
        assert_eq!(run(vec![v(true, Modifiers::NONE)], &mut v_down), [Command::MoveTool]);
        assert!(v_down);
        assert_eq!(run(vec![v(false, Modifiers::NONE)], &mut v_down), []);
    }

    #[test]
    fn no_two_commands_share_a_shortcut() {
        let shortcuts: Vec<_> = Command::ALL.iter().filter_map(|c| c.shortcut()).collect();
        for (i, a) in shortcuts.iter().enumerate() {
            assert!(!shortcuts[i + 1..].contains(a), "{a:?} is bound twice");
        }
    }
}
