//! The command palette: lists and filters the app's commands, and picks one.
//! What is typed that is no command is handed back, to be taken as a line
//! of script.

use egui::{Key, Modifiers};
use crate::commands::Command;

/// The commands in `commands` that `query` matches, best first.
pub fn matches(query: &str, commands: &[Command]) -> Vec<Command> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return commands.to_vec();
    }
    let mut classes: [Vec<Command>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
    for &command in commands {
        let label = command.label().to_lowercase();
        let class = if label.starts_with(&query) {
            0
        } else if label.split([' ', '/']).any(|word| word.starts_with(&query)) {
            1
        } else if label.contains(&query) {
            2
        } else {
            let mut chars = label.chars();
            if query.chars().all(|qc| chars.any(|lc| lc == qc)) {
                3
            } else {
                continue;
            }
        };
        classes[class].push(command);
    }
    classes.into_iter().flatten().collect()
}

#[derive(Default)]
pub struct Palette {
    open: bool,
    query: String,
    selected: usize,
    /// What Enter was pressed on that matched no command.
    pub typed: Option<String>,
}

impl Palette {
    /// Opens it with an empty query.
    pub fn open(&mut self) {
        self.open = true;
        self.query.clear();
        self.selected = 0;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Shows the palette if it is open, listing `commands`. Returns the command the user chose this frame, having closed itself.
    pub fn show(&mut self, ctx: &egui::Context, commands: &[Command]) -> Option<Command> {
        if !self.open {
            return None;
        }

        let mut matching = matches(&self.query, commands);
        let down = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowDown));
        let up = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowUp));
        let enter = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));

        if down {
            self.selected = (self.selected + 1).min(matching.len().saturating_sub(1));
        }
        if up {
            self.selected = self.selected.saturating_sub(1);
        }
        if down || up {
            ctx.memory_mut(|m| m.move_focus(egui::FocusDirection::None));
        }

        let mut clicked = None;
        let modal = egui::Modal::new(egui::Id::new("command_palette")).show(ctx, |ui| {
            ui.set_min_width(320.0);
            let prev = self.query.clone();
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .desired_width(f32::INFINITY)
                    .event_filter(egui::EventFilter {
                        vertical_arrows: true,
                        horizontal_arrows: true,
                        ..Default::default()
                    }),
            );
            response.request_focus();
            if self.query != prev {
                self.selected = 0;
                matching = matches(&self.query, commands);
            }
            if !matching.is_empty() && self.selected >= matching.len() {
                self.selected = matching.len() - 1;
            }
            ui.add_space(4.0);
            egui::ScrollArea::vertical().max_height(300.0).auto_shrink([false, true]).show(ui, |ui| {
                for (i, &command) in matching.iter().enumerate() {
                    let mut button = egui::Button::new(command.label()).selected(i == self.selected);
                    if let Some(shortcut) = command.shortcut() {
                        button = button.shortcut_text(ctx.format_shortcut(&shortcut));
                    }
                    let row = ui.add_sized([ui.available_width(), 0.0], button);
                    // The keys can take the highlight out of sight.
                    if i == self.selected && (down || up) {
                        row.scroll_to_me(None);
                    }
                    if row.clicked() {
                        clicked = Some(command);
                    }
                }
            });
        });

        let picked = if modal.should_close() {
            self.open = false;
            None
        } else if let Some(command) = clicked {
            self.open = false;
            Some(command)
        } else if enter {
            self.open = false;
            let picked = matching.get(self.selected).copied();
            if picked.is_none() {
                self.typed = Some(self.query.trim().to_owned()).filter(|typed| !typed.is_empty());
            }
            picked
        } else {
            None
        };
        // The field goes with the palette, and must not keep the keyboard:
        // the next key is a shortcut again.
        if !self.open {
            ctx.memory_mut(|memory| {
                if let Some(focused) = memory.focused() {
                    memory.surrender_focus(focused);
                }
            });
        }
        picked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_matches() {
        assert_eq!(matches("", &Command::ALL), Command::ALL);
        assert_eq!(
            matches("zo", &Command::ALL),
            [Command::ZoomIn, Command::ZoomOut, Command::ZoomTo100, Command::ZoomToFit, Command::ZoomToSelection]
        );
        assert_eq!(
            matches("sel", &Command::ALL),
            [
                Command::SelectAll,
                Command::SelectChildren,
                Command::SelectParent,
                Command::Group,
                Command::Ungroup,
                Command::FrameSelection,
                Command::ZoomToSelection,
                Command::ToggleVisible,
                Command::ToggleLocked
            ]
        );
        assert_eq!(
            matches("fr", &Command::ALL),
            [Command::FrameSelection, Command::FrameTool, Command::BringToFront, Command::BringForward]
        );
        assert_eq!(matches("UNDO", &Command::ALL), [Command::Undo]);
        assert_eq!(matches("  undo ", &Command::ALL), [Command::Undo]);
        assert_eq!(matches("xyz", &Command::ALL), []);
        assert_eq!(
            matches("zo", &[Command::ZoomOut, Command::Undo, Command::ZoomIn]),
            [Command::ZoomOut, Command::ZoomIn]
        );
    }

    struct Harness {
        ctx: egui::Context,
        palette: Palette,
        time: f64,
    }

    impl Harness {
        fn new() -> Self {
            Self {
                ctx: egui::Context::default(),
                palette: Palette::default(),
                time: 0.0,
            }
        }

        fn frame(&mut self, events: Vec<egui::Event>) -> Option<Command> {
            self.time += 0.05;
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0))),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            let mut chosen = None;
            let mut output = self.ctx.run_ui(input, |ui| {
                chosen = self.palette.show(ui.ctx(), &Command::ALL);
            });
            output.textures_delta.clear();
            chosen
        }

        fn open(&mut self) {
            self.palette.open();
            self.frame(vec![]);
        }

        fn type_text(&mut self, text: &str) -> Option<Command> {
            self.frame(vec![egui::Event::Text(text.into())])
        }

        fn key(&mut self, key: Key) -> Option<Command> {
            self.frame(vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            }])
        }
    }

    #[test]
    fn test_closed_and_open() {
        let mut h = Harness::new();
        assert_eq!(h.frame(vec![]), None);
        assert!(!h.palette.is_open());
        h.palette.open();
        assert!(h.palette.is_open());
    }

    #[test]
    fn test_type_and_enter() {
        let mut h = Harness::new();
        h.open();
        h.type_text("zo");
        assert_eq!(h.key(Key::Enter), Some(Command::ZoomIn));
        assert!(!h.palette.is_open());
    }

    #[test]
    fn test_arrow_down_and_enter() {
        let mut h = Harness::new();
        h.open();
        h.type_text("zo");
        h.key(Key::ArrowDown);
        h.key(Key::ArrowDown);
        assert_eq!(h.key(Key::Enter), Some(Command::ZoomTo100));
    }

    #[test]
    fn test_arrow_bounds() {
        let mut h = Harness::new();
        h.open();
        h.type_text("zo");
        h.key(Key::ArrowUp);
        for _ in 0..6 {
            h.key(Key::ArrowDown);
        }
        assert_eq!(h.key(Key::Enter), Some(Command::ZoomToSelection));
    }

    #[test]
    fn test_typing_resets_selected() {
        let mut h = Harness::new();
        h.open();
        h.type_text("zo");
        h.key(Key::ArrowDown);
        h.type_text("o");
        assert_eq!(h.key(Key::Enter), Some(Command::ZoomIn));
    }

    #[test]
    fn test_no_match_enter() {
        let mut h = Harness::new();
        h.open();
        h.type_text("xyz");
        assert_eq!(h.key(Key::Enter), None);
        assert!(!h.palette.is_open());
    }

    #[test]
    fn test_escape_and_reopen() {
        let mut h = Harness::new();
        h.open();
        assert_eq!(h.key(Key::Escape), None);
        assert!(!h.palette.is_open());
        h.open();
        h.type_text("undo");
        assert_eq!(h.key(Key::Enter), Some(Command::Undo));
    }
}
