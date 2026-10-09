//! The Design panel: the selected node's position, size and rotation as
//! numbers that can be dragged or typed.

use egui::{DragValue, Ui};
use omavec_engine::{Color, Error, History, Node, NodeId, Paint, PaintKind};
use omavec_geom::kurbo::{Affine, Point};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Field {
    X,
    Y,
    Width,
    Height,
    Rotation,
}

impl Field {
    const ALL: [Field; 5] = [Field::X, Field::Y, Field::Width, Field::Height, Field::Rotation];

    fn label(self) -> &'static str {
        match self {
            Field::X => "X ",
            Field::Y => "Y ",
            Field::Width => "W ",
            Field::Height => "H ",
            Field::Rotation => "∠ ",
        }
    }

    /// What changing the field is called in the undo history.
    fn step(self) -> &'static str {
        match self {
            Field::X | Field::Y => "Move",
            Field::Width | Field::Height => "Resize",
            Field::Rotation => "Rotate",
        }
    }

    /// X and Y are where the node's own corner sits in its parent. Rotation
    /// is in degrees, anticlockwise on screen, as Figma shows it.
    fn get(self, node: &Node) -> f64 {
        let [a, b, _, _, x, y] = node.transform.as_coeffs();
        match self {
            Field::X => x,
            Field::Y => y,
            Field::Width => node.size.width,
            Field::Height => node.size.height,
            // Adding zero turns the -0.0 of an unrotated node into 0.0.
            Field::Rotation => -b.atan2(a).to_degrees() + 0.0,
        }
    }

    fn set(self, node: &mut Node, value: f64) {
        let [a, b, c, d, x, y] = node.transform.as_coeffs();
        match self {
            Field::X => node.transform = Affine::new([a, b, c, d, value, y]),
            Field::Y => node.transform = Affine::new([a, b, c, d, x, value]),
            Field::Width => node.size.width = value.max(0.0),
            Field::Height => node.size.height = value.max(0.0),
            Field::Rotation => {
                // About the node's middle, which stays where it is. (This
                // drops any scale or skew; nothing makes those yet.)
                let middle = Point::new(node.size.width / 2.0, node.size.height / 2.0);
                let stays = node.transform * middle;
                node.transform = Affine::translate(stays.to_vec2()) * Affine::rotate(-value.to_radians()) * Affine::translate(-middle.to_vec2());
            }
        }
    }
}

/// A change to one of a node's fills.
#[derive(Clone, Copy, Debug, PartialEq)]
enum FillEdit {
    Color(usize, Color),
    Opacity(usize, f64),
    Visible(usize, bool),
    Remove(usize),
    /// A new fill on top, in Figma's grey.
    Add,
}

impl FillEdit {
    fn apply(self, node: &mut Node) {
        let fill = |node: &mut Node, index: usize, change: &dyn Fn(&mut Paint)| {
            if let Some(paint) = node.fills.get_mut(index) {
                change(paint);
            }
        };
        match self {
            FillEdit::Color(index, color) => fill(node, index, &|paint| paint.kind = PaintKind::Solid { color }),
            FillEdit::Opacity(index, opacity) => fill(node, index, &|paint| paint.opacity = opacity.clamp(0.0, 1.0)),
            FillEdit::Visible(index, visible) => fill(node, index, &|paint| paint.visible = visible),
            FillEdit::Remove(index) if index < node.fills.len() => drop(node.fills.remove(index)),
            FillEdit::Remove(_) => {}
            FillEdit::Add => node.fills.push(Paint::solid(Color::rgb(0xd9, 0xd9, 0xd9))),
        }
    }
}

#[derive(Default)]
pub struct Properties {
    /// A widget is being dragged, and its changes are one undo step so far.
    live: bool,
}

impl Properties {
    /// Makes one change from a widget. While the pointer is down every
    /// change joins one undo step named `name`; a value typed in, or a
    /// button, is a step of its own.
    fn change(&mut self, history: &mut History, id: NodeId, name: &str, pointer_down: bool, change: impl FnOnce(&mut Node)) -> Result<(), Error> {
        if pointer_down && !self.live {
            history.begin(name);
            self.live = true;
        }
        history.edit(name, |document| {
            change(document.node_mut(id)?);
            Ok(())
        })
    }

    /// Ends the step once the pointer has let go.
    fn settle(&mut self, history: &mut History, pointer_down: bool) {
        if self.live && !pointer_down {
            history.commit();
            self.live = false;
        }
    }

    pub fn show(&mut self, ui: &mut Ui, history: &mut History, selection: &[NodeId]) -> Result<(), Error> {
        let pointer_down = ui.input(|i| i.pointer.any_down());
        self.settle(history, pointer_down);
        let [id] = selection else {
            if selection.len() > 1 {
                ui.weak(format!("{} selected", selection.len()));
            }
            return Ok(());
        };
        let Some(node) = history.document().node(*id) else { return Ok(()) };
        let (values, fills) = (Field::ALL.map(|field| field.get(node)), node.fills.clone());
        ui.label(&node.name);
        ui.add_space(4.0);

        let mut moved = None;
        ui.horizontal_wrapped(|ui| {
            for (field, was) in Field::ALL.into_iter().zip(values) {
                let mut value = was;
                let suffix = if field == Field::Rotation { "°" } else { "" };
                ui.add(DragValue::new(&mut value).speed(1.0).max_decimals(2).prefix(field.label()).suffix(suffix));
                if value != was {
                    moved = Some((field, value));
                }
            }
        });
        if let Some((field, value)) = moved {
            self.change(history, *id, field.step(), pointer_down, |node| field.set(node, value))?;
        }

        ui.add_space(8.0);
        let mut edit = None;
        ui.horizontal(|ui| {
            ui.strong("Fill");
            if ui.small_button("+").on_hover_text("Add a fill").clicked() {
                edit = Some(FillEdit::Add);
            }
        });
        // The top fill first, as in Figma.
        for (index, paint) in fills.iter().enumerate().rev() {
            let PaintKind::Solid { color } = paint.kind;
            ui.horizontal(|ui| {
                let mut rgb = [color.r, color.g, color.b];
                if ui.color_edit_button_srgb(&mut rgb).changed() {
                    edit = Some(FillEdit::Color(index, Color::rgb(rgb[0], rgb[1], rgb[2])));
                }
                ui.monospace(String::from(color));
                let mut percent = (paint.opacity * 100.0).round();
                if ui.add(DragValue::new(&mut percent).range(0.0..=100.0).suffix("%")).changed() {
                    edit = Some(FillEdit::Opacity(index, percent / 100.0));
                }
                let mut visible = paint.visible;
                if ui.checkbox(&mut visible, "").on_hover_text("Show this fill").changed() {
                    edit = Some(FillEdit::Visible(index, visible));
                }
                if ui.small_button("−").on_hover_text("Remove this fill").clicked() {
                    edit = Some(FillEdit::Remove(index));
                }
            });
        }
        if let Some(edit) = edit {
            // Only a colour or an opacity is dragged; the rest are clicks.
            let dragged = pointer_down && matches!(edit, FillEdit::Color(..) | FillEdit::Opacity(..));
            self.change(history, *id, "Fill", dragged, |node| edit.apply(node))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use omavec_engine::{Document, NodeKind};
    use omavec_geom::kurbo::Size;

    /// A 200 × 100 rectangle at (30, 40) on the page.
    fn rectangle() -> (History, NodeId) {
        let mut document = Document::default();
        let page = document.pages[0].id;
        let mut node = document.create(NodeKind::Rectangle, Size::new(200.0, 100.0));
        node.transform = Affine::translate((30.0, 40.0));
        let id = node.id;
        document.insert(page, 0, node).unwrap();
        (History::new(document), id)
    }

    fn values(history: &History, id: NodeId) -> [f64; 5] {
        Field::ALL.map(|field| field.get(history.document().node(id).unwrap()))
    }

    /// Sets `field` as a typed value would.
    fn type_in(history: &mut History, id: NodeId, field: Field, value: f64) {
        Properties::default().change(history, id, field.step(), false, |node| field.set(node, value)).unwrap();
    }

    #[test]
    fn fields_read_and_write_the_node() {
        let (mut history, id) = rectangle();
        assert_eq!(values(&history, id), [30.0, 40.0, 200.0, 100.0, 0.0]);
        for (field, value) in [(Field::X, -5.0), (Field::Y, 60.5), (Field::Width, 80.0), (Field::Height, 20.0)] {
            type_in(&mut history, id, field, value);
        }
        assert_eq!(values(&history, id), [-5.0, 60.5, 80.0, 20.0, 0.0]);
        // A size can't go below nothing.
        type_in(&mut history, id, Field::Width, -10.0);
        assert_eq!(values(&history, id)[2], 0.0);
    }

    #[test]
    fn rotation_is_anticlockwise_about_the_middle() {
        let (mut history, id) = rectangle();
        let on_page = |history: &History, x: f64, y: f64| history.document().node(id).unwrap().transform * Point::new(x, y);
        let middle = on_page(&history, 100.0, 50.0);
        type_in(&mut history, id, Field::Rotation, 90.0);
        assert!((values(&history, id)[4] - 90.0).abs() < 1e-9);
        assert!((on_page(&history, 100.0, 50.0) - middle).hypot() < 1e-9, "the middle moved");
        // Its right-hand edge's middle is now straight above its middle on screen.
        let right = on_page(&history, 200.0, 50.0);
        assert!((right - Point::new(middle.x, middle.y - 100.0)).hypot() < 1e-9, "{right:?}");
        // And back.
        type_in(&mut history, id, Field::Rotation, 0.0);
        let back = values(&history, id);
        assert!((back[0] - 30.0).abs() < 1e-9 && (back[1] - 40.0).abs() < 1e-9 && back[4].abs() < 1e-9, "{back:?}");
    }

    #[test]
    fn a_drag_on_a_widget_is_one_step_and_a_typed_value_is_another() {
        let (mut history, id) = rectangle();
        let mut panel = Properties::default();
        // Dragged: three changes with the pointer down, then it lets go.
        for x in [31.0, 35.0, 50.0] {
            panel.change(&mut history, id, "Move", true, |node| Field::X.set(node, x)).unwrap();
            panel.settle(&mut history, true);
        }
        panel.settle(&mut history, false);
        panel.change(&mut history, id, "Resize", false, |node| Field::Height.set(node, 10.0)).unwrap();
        panel.settle(&mut history, false);
        assert_eq!((history.undo_name(), values(&history, id)), (Some("Resize"), [50.0, 40.0, 200.0, 10.0, 0.0]));
        history.undo();
        assert_eq!((history.undo_name(), values(&history, id)), (Some("Move"), [50.0, 40.0, 200.0, 100.0, 0.0]));
        history.undo();
        assert_eq!((history.undo_name(), values(&history, id)), (None, [30.0, 40.0, 200.0, 100.0, 0.0]));
        // A pointer that comes and goes without changing anything leaves nothing.
        panel.settle(&mut history, true);
        panel.settle(&mut history, false);
        assert_eq!(history.undo_name(), None);
    }

    #[test]
    fn fills_are_edited_one_at_a_time() {
        let (mut history, id) = rectangle();
        let mut panel = Properties::default();
        let fills = |history: &History| history.document().node(id).unwrap().fills.clone();
        let mut edit = |history: &mut History, edit: FillEdit| panel.change(history, id, "Fill", false, |node| edit.apply(node)).unwrap();
        let (grey, red) = (Color::rgb(0xd9, 0xd9, 0xd9), Color::rgb(255, 0, 0));
        assert_eq!(fills(&history), [Paint::solid(grey)]);

        edit(&mut history, FillEdit::Add);
        edit(&mut history, FillEdit::Color(1, red));
        edit(&mut history, FillEdit::Opacity(1, 0.4));
        edit(&mut history, FillEdit::Visible(0, false));
        let expected = [Paint { visible: false, ..Paint::solid(grey) }, Paint { opacity: 0.4, ..Paint::solid(red) }];
        assert_eq!(fills(&history), expected);
        // Opacity stays between nothing and everything.
        edit(&mut history, FillEdit::Opacity(1, 7.0));
        assert_eq!(fills(&history)[1].opacity, 1.0);
        history.undo();

        edit(&mut history, FillEdit::Remove(0));
        assert_eq!(fills(&history), expected[1..]);
        // A fill that isn't there: nothing happens, and nothing to undo.
        let steps = history.undo_name().map(str::to_owned);
        edit(&mut history, FillEdit::Remove(5));
        edit(&mut history, FillEdit::Color(5, grey));
        assert_eq!(fills(&history), expected[1..]);
        assert_eq!(history.undo_name().map(str::to_owned), steps);
        history.undo();
        assert_eq!(fills(&history), expected);
    }
}
