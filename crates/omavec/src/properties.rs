//! The Design panel: the selected node's position, size and rotation as
//! numbers that can be dragged or typed.

use egui::{DragValue, Ui};
use omavec_engine::{Error, History, Node, NodeId};
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

/// Turns what a field's widget did this frame into edits: a drag is one
/// undo step from where it `started` to where it `stopped`, and a typed
/// value is a step of its own.
fn apply(history: &mut History, id: NodeId, field: Field, started: bool, value: Option<f64>, stopped: bool) -> Result<(), Error> {
    if started {
        history.begin(field.step());
    }
    if let Some(value) = value {
        history.edit(field.step(), |document| {
            field.set(document.node_mut(id)?, value);
            Ok(())
        })?;
    }
    if stopped {
        history.commit();
    }
    Ok(())
}

pub fn show(ui: &mut Ui, history: &mut History, selection: &[NodeId]) -> Result<(), Error> {
    let [id] = selection else {
        if selection.len() > 1 {
            ui.weak(format!("{} selected", selection.len()));
        }
        return Ok(());
    };
    let Some(node) = history.document().node(*id) else { return Ok(()) };
    let values = Field::ALL.map(|field| field.get(node));
    ui.label(&node.name);
    ui.add_space(4.0);
    let mut result = Ok(());
    ui.horizontal_wrapped(|ui| {
        for (field, was) in Field::ALL.into_iter().zip(values) {
            let mut value = was;
            let suffix = if field == Field::Rotation { "°" } else { "" };
            let response = ui.add(DragValue::new(&mut value).speed(1.0).max_decimals(2).prefix(field.label()).suffix(suffix));
            let applied = apply(history, *id, field, response.drag_started(), (value != was).then_some(value), response.drag_stopped());
            result = std::mem::replace(&mut result, Ok(())).and(applied);
        }
    });
    result
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

    #[test]
    fn fields_read_and_write_the_node() {
        let (mut history, id) = rectangle();
        assert_eq!(values(&history, id), [30.0, 40.0, 200.0, 100.0, 0.0]);
        for (field, value) in [(Field::X, -5.0), (Field::Y, 60.5), (Field::Width, 80.0), (Field::Height, 20.0)] {
            apply(&mut history, id, field, false, Some(value), false).unwrap();
        }
        assert_eq!(values(&history, id), [-5.0, 60.5, 80.0, 20.0, 0.0]);
        // A size can't go below nothing.
        apply(&mut history, id, Field::Width, false, Some(-10.0), false).unwrap();
        assert_eq!(values(&history, id)[2], 0.0);
    }

    #[test]
    fn rotation_is_anticlockwise_about_the_middle() {
        let (mut history, id) = rectangle();
        let on_page = |history: &History, x: f64, y: f64| history.document().node(id).unwrap().transform * Point::new(x, y);
        let middle = on_page(&history, 100.0, 50.0);
        apply(&mut history, id, Field::Rotation, false, Some(90.0), false).unwrap();
        assert!((values(&history, id)[4] - 90.0).abs() < 1e-9);
        assert!((on_page(&history, 100.0, 50.0) - middle).hypot() < 1e-9, "the middle moved");
        // Its right-hand edge's middle is now straight above its middle on screen.
        let right = on_page(&history, 200.0, 50.0);
        assert!((right - Point::new(middle.x, middle.y - 100.0)).hypot() < 1e-9, "{right:?}");
        // And back.
        apply(&mut history, id, Field::Rotation, false, Some(0.0), false).unwrap();
        let back = values(&history, id);
        assert!((back[0] - 30.0).abs() < 1e-9 && (back[1] - 40.0).abs() < 1e-9 && back[4].abs() < 1e-9, "{back:?}");
    }

    #[test]
    fn a_drag_on_a_field_is_one_step_and_a_typed_value_is_another() {
        let (mut history, id) = rectangle();
        apply(&mut history, id, Field::X, true, None, false).unwrap();
        for x in [31.0, 35.0, 50.0] {
            apply(&mut history, id, Field::X, false, Some(x), false).unwrap();
        }
        apply(&mut history, id, Field::X, false, None, true).unwrap();
        apply(&mut history, id, Field::Height, false, Some(10.0), false).unwrap();
        assert_eq!((history.undo_name(), values(&history, id)), (Some("Resize"), [50.0, 40.0, 200.0, 10.0, 0.0]));
        history.undo();
        assert_eq!((history.undo_name(), values(&history, id)), (Some("Move"), [50.0, 40.0, 200.0, 100.0, 0.0]));
        history.undo();
        assert_eq!((history.undo_name(), values(&history, id)), (None, [30.0, 40.0, 200.0, 100.0, 0.0]));
        // A field left alone leaves nothing behind.
        apply(&mut history, id, Field::Y, false, None, false).unwrap();
        apply(&mut history, id, Field::Y, true, None, true).unwrap();
        assert_eq!(history.undo_name(), None);
    }
}
