//! The Layers panel: the page's node tree with selection, rename, hide and lock.

use std::sync::Arc;
use egui::text::{CCursor, CCursorRange};
use egui::{Align, Button, Key, Layout, RichText, Sense, TextEdit, Ui, UiBuilder};
use omavec_engine::{Error, History, Node, NodeId};
use crate::theme::Theme;

#[derive(Clone, Copy)]
enum Flag { Visible, Locked }

/// Flips `flag` across `nodes`. If any is in its default active state (shown
/// or unlocked) all become inactive; otherwise all become active.
fn toggle_flag(history: &mut History, nodes: &[NodeId], flag: Flag) -> Result<(), Error> {
    if nodes.is_empty() { return Ok(()); }
    let doc = history.document();
    let (name, target) = match flag {
        Flag::Visible => ("Show/Hide", !nodes.iter().any(|id| doc.node(*id).is_some_and(|n| n.visible))),
        Flag::Locked => ("Lock/Unlock", nodes.iter().any(|id| doc.node(*id).is_some_and(|n| !n.locked))),
    };
    history.edit(name, |doc| {
        for &id in nodes {
            match flag {
                Flag::Visible => doc.node_mut(id)?.visible = target,
                Flag::Locked => doc.node_mut(id)?.locked = target,
            }
        }
        Ok(())
    })
}

/// Hides the nodes if any of them is shown, otherwise shows them all (Figma's Ctrl+Shift+H). One undo step, "Show/Hide".
pub fn toggle_visible(history: &mut History, nodes: &[NodeId]) -> Result<(), Error> { toggle_flag(history, nodes, Flag::Visible) }

/// Locks the nodes if any of them is unlocked, otherwise unlocks them all (Ctrl+Shift+L). One undo step, "Lock/Unlock".
pub fn toggle_locked(history: &mut History, nodes: &[NodeId]) -> Result<(), Error> { toggle_flag(history, nodes, Flag::Locked) }

pub(crate) fn row_id(id: NodeId) -> egui::Id { egui::Id::new(("layer-row", id)) }

struct Row { id: NodeId, depth: usize, name: String, visible: bool, locked: bool }

fn collect(nodes: &[Arc<Node>], depth: usize, rows: &mut Vec<Row>) {
    for node in nodes.iter().rev() {
        rows.push(Row { id: node.id, depth, name: node.name.clone(), visible: node.visible, locked: node.locked });
        collect(&node.children, depth + 1, rows);
    }
}

#[derive(Default)]
pub struct LayersPanel {
    /// The node being renamed and the text typed so far.
    pub(crate) renaming: Option<(NodeId, String)>,
    focused: bool,
}

impl LayersPanel {
    /// Draws the rows of `page`, front-most first, and makes the changes the user asks for.
    pub fn show(&mut self, ui: &mut Ui, history: &mut History, page: NodeId, selection: &mut Vec<NodeId>, theme: &Theme) -> Result<(), Error> {
        let mut rows = Vec::new();
        if let Some(page) = history.document().node(page) { collect(&page.children, 0, &mut rows); }
        for row in rows {
            let selected = selection.contains(&row.id);
            let is_renaming = self.renaming.as_ref().is_some_and(|(id, _)| *id == row.id);
            let (mut eye_clicked, mut lock_clicked) = (false, false);

            // Senses clicks beneath child widgets so the eye and lock buttons get their own clicks.
            let builder = UiBuilder::new().id(row_id(row.id)).sense(Sense::click());
            let scope = ui.scope_builder(builder, |ui| {
                ui.horizontal(|ui| {
                    ui.set_width(ui.available_width());
                    ui.add_space(row.depth as f32 * 12.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let mut toggle = |ui: &mut Ui, on: bool, (yes, no): (&str, &str), flag: Flag| -> Result<bool, Error> {
                            let resp = ui.add(Button::new(if on { yes } else { no }).frame(false));
                            let hit = ui.interact(resp.rect, row_id(row.id).with(yes), Sense::click());
                            let clicked = resp.clicked() || hit.clicked();
                            if clicked { toggle_flag(history, &[row.id], flag)?; }
                            Ok(clicked)
                        };
                        lock_clicked = toggle(ui, row.locked, ("🔒", "·"), Flag::Locked)?;
                        eye_clicked = toggle(ui, row.visible, ("👁", "–"), Flag::Visible)?;

                        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                            if is_renaming {
                                let mut text = match &self.renaming {
                                    Some((id, t)) if *id == row.id => t.clone(),
                                    _ => String::new(),
                                };
                                let edit = ui.add(TextEdit::singleline(&mut text).desired_width(ui.available_width()));
                                if !self.focused {
                                    edit.request_focus();
                                    let mut state = TextEdit::load_state(ui.ctx(), edit.id).unwrap_or_default();
                                    state.cursor.set_char_range(Some(CCursorRange::two(CCursor::default(), CCursor::new(text.chars().count()))));
                                    TextEdit::store_state(ui.ctx(), edit.id, state);
                                    (self.focused, self.renaming) = (true, Some((row.id, text)));
                                } else {
                                    let (enter, esc, done) = (ui.input(|i| i.key_pressed(Key::Enter)), ui.input(|i| i.key_pressed(Key::Escape)), edit.lost_focus());
                                    if done || enter || esc {
                                        let new_name = text.trim();
                                        if (done || enter) && !esc && !new_name.is_empty() && new_name != row.name {
                                            history.edit("Rename", |doc| { doc.node_mut(row.id)?.name = new_name.into(); Ok(()) })?;
                                        }
                                        (self.renaming, self.focused) = (None, false);
                                    } else {
                                        self.renaming = Some((row.id, text));
                                    }
                                }
                            } else {
                                let mut text = RichText::new(&row.name);
                                if selected {
                                    text = text.color(theme.accent).strong();
                                } else if !row.visible {
                                    text = text.color(theme.muted);
                                }
                                ui.add(egui::Label::new(text).truncate().selectable(false));
                            }
                            Ok::<(), Error>(())
                        }).inner
                    }).inner
                }).inner
            });
            scope.inner?;

            let row_resp = scope.response;
            if !eye_clicked && !lock_clicked && !is_renaming {
                if row_resp.double_clicked() {
                    (self.renaming, self.focused) = (Some((row.id, row.name)), false);
                } else if row_resp.clicked() {
                    if ui.input(|i| i.modifiers.shift) {
                        if let Some(pos) = selection.iter().position(|&id| id == row.id) { selection.remove(pos); } else { selection.push(row.id); }
                    } else {
                        *selection = vec![row.id];
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, Modifiers, PointerButton, pos2, vec2};
    use omavec_engine::{Document, NodeKind};
    use omavec_geom::kurbo::Size;

    struct Harness {
        ctx: egui::Context,
        panel: LayersPanel,
        history: History,
        page: NodeId,
        selection: Vec<NodeId>,
        theme: Theme,
        time: f64,
        modifiers: Modifiers,
        frame_id: NodeId,
        rect1_id: NodeId,
        rect2_id: NodeId,
        ellipse_id: NodeId,
    }

    impl Harness {
        fn new() -> Self {
            let mut doc = Document::default();
            let page = doc.pages[0].id;

            let mut frame = doc.create(NodeKind::Frame { clip: true }, Size::new(200.0, 200.0));
            frame.name = "Frame".into();
            let frame_id = frame.id;
            doc.insert(page, 0, frame).unwrap();

            let mut rect1 = doc.create(NodeKind::Rectangle, Size::new(50.0, 50.0));
            rect1.name = "Rect 1".into();
            let rect1_id = rect1.id;
            doc.insert(frame_id, 0, rect1).unwrap();

            let mut rect2 = doc.create(NodeKind::Rectangle, Size::new(50.0, 50.0));
            rect2.name = "Rect 2".into();
            let rect2_id = rect2.id;
            doc.insert(frame_id, 1, rect2).unwrap();

            let mut ellipse = doc.create(NodeKind::Ellipse, Size::new(60.0, 60.0));
            ellipse.name = "Ellipse".into();
            let ellipse_id = ellipse.id;
            doc.insert(page, 1, ellipse).unwrap();

            let mut h = Self {
                ctx: egui::Context::default(),
                panel: LayersPanel::default(),
                history: History::new(doc),
                page,
                selection: Vec::new(),
                theme: Theme::default(),
                time: 0.0,
                modifiers: Modifiers::NONE,
                frame_id,
                rect1_id,
                rect2_id,
                ellipse_id,
            };
            h.frame(vec![]);
            h
        }

        fn frame(&mut self, events: Vec<Event>) {
            self.time += 0.05;
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(pos2(0.0, 0.0), vec2(300.0, 600.0))),
                time: Some(self.time),
                events: [vec![Event::ModifiersChanged(self.modifiers)], events].concat(),
                ..Default::default()
            };
            let (panel, history, page, selection, theme) = (&mut self.panel, &mut self.history, self.page, &mut self.selection, &self.theme);
            let mut output = self.ctx.run_ui(input, |ui| {
                panel.show(ui, history, page, selection, theme).unwrap();
            });
            output.textures_delta.clear();
        }

        fn row_point(&self, id: NodeId) -> egui::Pos2 {
            let rect = self.ctx.read_response(row_id(id)).unwrap().rect;
            pos2(rect.left() + 20.0, rect.center().y)
        }

        fn eye_point(&self, id: NodeId) -> egui::Pos2 {
            self.ctx.read_response(row_id(id).with("👁")).unwrap().rect.center()
        }

        fn button(&mut self, pos: egui::Pos2, pressed: bool) {
            self.frame(vec![
                Event::PointerMoved(pos),
                Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: self.modifiers },
            ]);
        }

        fn click(&mut self, pos: egui::Pos2) {
            self.time += 1.0;
            self.button(pos, true);
            self.button(pos, false);
        }

        fn shift_click(&mut self, pos: egui::Pos2) {
            self.modifiers = Modifiers::SHIFT;
            self.click(pos);
            self.modifiers = Modifiers::NONE;
        }

        fn double_click(&mut self, pos: egui::Pos2) {
            self.click(pos);
            self.button(pos, true);
            self.button(pos, false);
        }
    }

    #[test]
    fn rows_are_front_most_first_with_children_under_parent() {
        let mut h = Harness::new();
        h.frame(vec![]);
        let y = |id| h.ctx.read_response(row_id(id)).unwrap().rect.min.y;
        assert!(y(h.ellipse_id) < y(h.frame_id));
        assert!(y(h.frame_id) < y(h.rect2_id));
        assert!(y(h.rect2_id) < y(h.rect1_id));
    }

    #[test]
    fn clicking_and_shift_clicking_selects_nodes() {
        let mut h = Harness::new();
        let p_ellipse = h.row_point(h.ellipse_id);
        let p_frame = h.row_point(h.frame_id);
        let p_rect1 = h.row_point(h.rect1_id);

        h.click(p_ellipse);
        assert_eq!(h.selection, vec![h.ellipse_id]);

        h.shift_click(p_frame);
        assert_eq!(h.selection, vec![h.ellipse_id, h.frame_id]);

        h.shift_click(p_ellipse);
        assert_eq!(h.selection, vec![h.frame_id]);

        h.click(p_rect1);
        assert_eq!(h.selection, vec![h.rect1_id]);
    }

    #[test]
    fn eye_toggles_node_visibility() {
        let mut h = Harness::new();
        let p_eye = h.eye_point(h.ellipse_id);

        h.click(p_eye);
        assert!(!h.history.document().node(h.ellipse_id).unwrap().visible);
        assert_eq!(h.history.undo_name(), Some("Show/Hide"));

        h.click(p_eye);
        assert!(h.history.document().node(h.ellipse_id).unwrap().visible);
        assert_eq!(h.history.undo_name(), Some("Show/Hide"));
    }

    #[test]
    fn toggle_visible_and_toggle_locked_behave_as_expected() {
        let mut h = Harness::new();
        let (r1, r2) = (h.rect1_id, h.rect2_id);

        // Prep: r1 hidden, r2 shown
        h.history.edit("prep", |doc| {
            doc.node_mut(r1)?.visible = false;
            Ok(())
        }).unwrap();
        assert!(h.history.document().node(r2).unwrap().visible);
        assert!(!h.history.document().node(r1).unwrap().visible);

        // [shown, hidden] hides both
        let rev_before = h.history.revision();
        toggle_visible(&mut h.history, &[r2, r1]).unwrap();
        assert!(!h.history.document().node(r2).unwrap().visible);
        assert!(!h.history.document().node(r1).unwrap().visible);
        assert_eq!(h.history.revision(), rev_before + 1);
        assert_eq!(h.history.undo_name(), Some("Show/Hide"));

        // [hidden, hidden] shows both
        let rev_before = h.history.revision();
        toggle_visible(&mut h.history, &[r2, r1]).unwrap();
        assert!(h.history.document().node(r2).unwrap().visible);
        assert!(h.history.document().node(r1).unwrap().visible);
        assert_eq!(h.history.revision(), rev_before + 1);
        assert_eq!(h.history.undo_name(), Some("Show/Hide"));

        // Toggle locked: prep r1 locked, r2 unlocked
        h.history.edit("prep", |doc| {
            doc.node_mut(r1)?.locked = true;
            Ok(())
        }).unwrap();
        assert!(!h.history.document().node(r2).unwrap().locked);
        assert!(h.history.document().node(r1).unwrap().locked);

        // [unlocked, locked] locks both
        let rev_before = h.history.revision();
        toggle_locked(&mut h.history, &[r2, r1]).unwrap();
        assert!(h.history.document().node(r2).unwrap().locked);
        assert!(h.history.document().node(r1).unwrap().locked);
        assert_eq!(h.history.revision(), rev_before + 1);
        assert_eq!(h.history.undo_name(), Some("Lock/Unlock"));

        // [locked, locked] unlocks both
        let rev_before = h.history.revision();
        toggle_locked(&mut h.history, &[r2, r1]).unwrap();
        assert!(!h.history.document().node(r2).unwrap().locked);
        assert!(!h.history.document().node(r1).unwrap().locked);
        assert_eq!(h.history.revision(), rev_before + 1);
        assert_eq!(h.history.undo_name(), Some("Lock/Unlock"));

        // No nodes: no step
        let rev_before = h.history.revision();
        toggle_visible(&mut h.history, &[]).unwrap();
        assert_eq!(h.history.revision(), rev_before);
        toggle_locked(&mut h.history, &[]).unwrap();
        assert_eq!(h.history.revision(), rev_before);
    }

    #[test]
    fn double_click_renames_or_cancels() {
        let mut h = Harness::new();
        let p_ellipse = h.row_point(h.ellipse_id);

        // Double click, type new name, Enter -> renamed, one "Rename" step, undo restores old name
        h.double_click(p_ellipse);
        assert!(h.panel.renaming.is_some());
        h.frame(vec![]);
        h.frame(vec![
            Event::Text("Sun".into()),
            Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE },
        ]);
        assert_eq!(h.panel.renaming, None);
        assert_eq!(h.history.document().node(h.ellipse_id).unwrap().name, "Sun");
        assert_eq!(h.history.undo_name(), Some("Rename"));
        assert!(h.history.undo());
        assert_eq!(h.history.document().node(h.ellipse_id).unwrap().name, "Ellipse");

        // Double click, type, Esc -> unchanged, no step
        let rev_before = h.history.revision();
        h.double_click(p_ellipse);
        assert!(h.panel.renaming.is_some());
        h.frame(vec![]);
        h.frame(vec![
            Event::Text("Moon".into()),
            Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE },
        ]);
        assert_eq!(h.panel.renaming, None);
        assert_eq!(h.history.document().node(h.ellipse_id).unwrap().name, "Ellipse");
        assert_eq!(h.history.revision(), rev_before);

        // Double click, clear text, Enter -> unchanged, no step
        let rev_before = h.history.revision();
        h.double_click(p_ellipse);
        assert!(h.panel.renaming.is_some());
        h.frame(vec![]);
        h.panel.renaming = Some((h.ellipse_id, String::new()));
        h.frame(vec![
            Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE },
        ]);
        assert_eq!(h.panel.renaming, None);
        assert_eq!(h.history.document().node(h.ellipse_id).unwrap().name, "Ellipse");
        assert_eq!(h.history.revision(), rev_before);
    }
}
