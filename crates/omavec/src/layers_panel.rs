//! The Layers panel: the page's node tree with selection, rename, hide, lock,
//! and reordering by drag.

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

struct Row {
    id: NodeId,
    depth: usize,
    name: String,
    visible: bool,
    locked: bool,
    parent: NodeId,
    /// The sibling just in front of it, which is the row above among its own.
    front: Option<NodeId>,
    /// Whether things can be dropped into it, and whether it has any yet.
    holds: Option<bool>,
}

fn collect(parent: &Node, depth: usize, rows: &mut Vec<Row>) {
    for (index, node) in parent.children.iter().enumerate().rev() {
        let (front, holds) = (parent.children.get(index + 1).map(|front| front.id), node.kind.is_container().then_some(!node.children.is_empty()));
        rows.push(Row { id: node.id, depth, name: node.name.clone(), visible: node.visible, locked: node.locked, parent: parent.id, front, holds });
        collect(node, depth + 1, rows);
    }
}

/// Where rows dragged to `at` would go: into `parent`, just behind `behind`
/// (or to the front), with the line or box that shows it. `None` on one of
/// the rows being dragged.
fn drop_at(rows: &[(Row, egui::Rect)], at: egui::Pos2, dragged: &[NodeId]) -> Option<(NodeId, Option<NodeId>, egui::Rect)> {
    // Below the last row is behind everything on the page.
    let (row, rect) = rows.iter().find(|(_, rect)| at.y < rect.bottom()).or(rows.last())?;
    if dragged.contains(&row.id) {
        return None;
    }
    let within = ((at.y - rect.top()) / rect.height()).clamp(0.0, 1.0);
    let indent = egui::vec2(row.depth as f32 * 12.0, 0.0);
    let line = |y: f32, indent: egui::Vec2| egui::Rect::from_min_max(egui::pos2(rect.left(), y) + indent, egui::pos2(rect.right(), y));
    match row.holds {
        // The middle of a container's row, or the lower half of one whose
        // children are listed under it: into it, at the front.
        Some(false) if within > 0.25 && within < 0.75 => Some((row.id, None, *rect)),
        Some(true) if within >= 0.5 => Some((row.id, None, line(rect.bottom(), indent + egui::vec2(12.0, 0.0)))),
        // Just above the row that is already just below it: where it is.
        _ if within < 0.5 && row.front.is_some_and(|front| dragged.contains(&front)) => None,
        _ if within < 0.5 => Some((row.parent, row.front, line(rect.top(), indent))),
        _ => Some((row.parent, Some(row.id), line(rect.bottom(), indent))),
    }
}

#[derive(Default)]
pub struct LayersPanel {
    /// The node being renamed and the text typed so far.
    pub(crate) renaming: Option<(NodeId, String)>,
    focused: bool,
    /// Rows are being dragged to a new place.
    dragging: bool,
}

impl LayersPanel {
    /// Draws the rows of `page`, front-most first, and makes the changes the user asks for.
    pub fn show(&mut self, ui: &mut Ui, history: &mut History, page: NodeId, selection: &mut Vec<NodeId>, theme: &Theme) -> Result<(), Error> {
        let mut rows = Vec::new();
        if let Some(page) = history.document().node(page) { collect(page, 0, &mut rows); }
        let (mut placed, mut dropped) = (Vec::new(), false);
        for row in rows {
            let selected = selection.contains(&row.id);
            let is_renaming = self.renaming.as_ref().is_some_and(|(id, _)| *id == row.id);
            let (mut eye_clicked, mut lock_clicked) = (false, false);

            // Senses clicks beneath child widgets so the eye and lock buttons get their own clicks.
            let builder = UiBuilder::new().id(row_id(row.id)).sense(Sense::click_and_drag());
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
                // A drag takes the whole selection, or the row alone if it isn't in it.
                if row_resp.drag_started() {
                    self.dragging = true;
                    if !selected { *selection = vec![row.id]; }
                }
                dropped |= row_resp.drag_stopped();
                if row_resp.double_clicked() {
                    (self.renaming, self.focused) = (Some((row.id, row.name.clone())), false);
                } else if row_resp.clicked() {
                    if ui.input(|i| i.modifiers.shift) {
                        if let Some(pos) = selection.iter().position(|&id| id == row.id) { selection.remove(pos); } else { selection.push(row.id); }
                    } else {
                        *selection = vec![row.id];
                    }
                }
            }
            placed.push((row, row_resp.rect));
        }
        if self.dragging {
            let target = ui.input(|i| i.pointer.interact_pos()).and_then(|at| drop_at(&placed, at, selection));
            if let Some((_, _, mark)) = target {
                // A line between two rows, or a box round the one to go into.
                ui.painter().rect_stroke(mark, 2.0, egui::Stroke::new(2.0, theme.accent), egui::StrokeKind::Inside);
            }
            if dropped {
                self.dragging = false;
                if let Some((parent, behind, _)) = target {
                    // Dropped on something inside itself, it stays where it is.
                    match history.edit("Reorder", |doc| doc.rehome(selection, parent, behind)) {
                        Ok(()) | Err(Error::IntoItself(_)) => {}
                        Err(error) => return Err(error),
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

            /// Presses on `from`, moves to `to` by way of halfway, and lets go.
        fn drag(&mut self, from: egui::Pos2, to: egui::Pos2) {
            self.time += 1.0;
            self.button(from, true);
            self.frame(vec![Event::PointerMoved(from + (to - from) / 2.0)]);
            self.frame(vec![Event::PointerMoved(to)]);
            self.button(to, false);
            // The rows are where the drop put them from the next frame on,
            // and egui knows where that is the frame after.
            self.frame(vec![]);
            self.frame(vec![]);
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

    #[test]
    fn dragging_a_row_moves_the_node_to_where_it_is_dropped() {
        let mut h = Harness::new();
        let children = |h: &Harness, id: NodeId| h.history.document().node(id).unwrap().children.iter().map(|node| node.id).collect::<Vec<_>>();
        let rect = |h: &Harness, id: NodeId| h.ctx.read_response(row_id(id)).unwrap().rect;
        let (frame, rect1, rect2, ellipse) = (h.frame_id, h.rect1_id, h.rect2_id, h.ellipse_id);
        // The rows are Ellipse, Frame, Rect 2, Rect 1. Onto the lower half of
        // the last: behind it, in the frame, and now what is selected.
        let on_page = h.history.document().to_page(ellipse).unwrap();
        let (from, onto) = (h.row_point(ellipse), rect(&h, rect1));
        h.drag(from, pos2(from.x, onto.bottom() - 2.0));
        assert_eq!((children(&h, h.page), children(&h, frame)), (vec![frame], vec![ellipse, rect1, rect2]));
        assert_eq!((h.selection.clone(), h.history.undo_name()), (vec![ellipse], Some("Reorder")));
        assert_eq!(h.history.document().to_page(ellipse).unwrap(), on_page, "it hasn't moved on the page");
        // Onto the upper half of the first row: in front of the frame, on the page.
        let (from, onto) = (h.row_point(rect1), rect(&h, frame));
        h.drag(from, pos2(from.x, onto.top() + 2.0));
        assert_eq!((children(&h, h.page), children(&h, frame)), (vec![frame, rect1], vec![ellipse, rect2]));
        // Onto the lower half of the frame's row, with its children under it:
        // into it, at the front. Both selected rows go, in their order.
        h.selection = vec![rect1];
        let (from, onto) = (h.row_point(rect1), rect(&h, frame));
        h.drag(from, pos2(from.x, onto.bottom() - 2.0));
        assert_eq!((children(&h, h.page), children(&h, frame)), (vec![frame], vec![ellipse, rect2, rect1]));
        // Dropped on itself, or the frame on something in it: nothing happens.
        let before = h.history.revision();
        let from = h.row_point(rect2);
        h.drag(from, from + vec2(0.0, 7.0));
        // Or just above the row below it, which is where it is already.
        let onto = rect(&h, ellipse);
        h.drag(from, pos2(from.x, onto.top() + 2.0));
        let (from, onto) = (h.row_point(frame), rect(&h, ellipse));
        h.drag(from, pos2(from.x, onto.bottom() - 2.0));
        assert_eq!((h.history.revision(), children(&h, frame)), (before, vec![ellipse, rect2, rect1]));
        assert!(!h.panel.dragging);
    }
}
