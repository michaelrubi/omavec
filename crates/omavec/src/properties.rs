//! The Design panel: the selected node's position, size and rotation as
//! numbers that can be dragged or typed.

use egui::{DragValue, Ui};
use omavec_engine::{Align, Cap, Color, Error, Export, History, Join, Node, NodeId, NodeKind, Paint, PaintKind, Stop};

use crate::commands::Command;
use omavec_geom::kurbo::Affine;

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

    /// X and Y are where the corner of the node's box sits in its parent.
    /// Rotation is in degrees, anticlockwise on screen, as Figma shows it.
    fn get(self, node: &Node) -> f64 {
        let [a, b, ..] = node.transform.as_coeffs();
        let (area, corner) = (node.bounds(), node.transform * node.bounds().origin());
        match self {
            Field::X => corner.x,
            Field::Y => corner.y,
            Field::Width => area.width(),
            Field::Height => area.height(),
            // Adding zero turns the -0.0 of an unrotated node into 0.0.
            Field::Rotation => -b.atan2(a).to_degrees() + 0.0,
        }
    }

    fn set(self, node: &mut Node, value: f64) {
        let (area, was) = (node.bounds(), self.get(node));
        match self {
            Field::X => node.transform = Affine::translate((value - was, 0.0)) * node.transform,
            Field::Y => node.transform = Affine::translate((0.0, value - was)) * node.transform,
            // A group has no size of its own: what is in it is resized,
            // from the corner of its box.
            Field::Width | Field::Height if node.kind == NodeKind::Group => {
                let factor = if was > 0.0 { value.max(0.0) / was } else { 1.0 };
                let scale = if self == Field::Width { Affine::scale_non_uniform(factor, 1.0) } else { Affine::scale_non_uniform(1.0, factor) };
                let corner = area.origin().to_vec2();
                let within = Affine::translate(corner) * scale * Affine::translate(-corner);
                if node.transform.determinant() != 0.0 {
                    node.stretch(node.transform * within * node.transform.inverse());
                }
            }
            Field::Width => node.size.width = value.max(0.0),
            Field::Height => node.size.height = value.max(0.0),
            Field::Rotation => {
                // About the middle of the node's box, which stays where it
                // is. (This drops any scale or skew; nothing makes those yet.)
                let middle = area.center();
                let stays = node.transform * middle;
                node.transform = Affine::translate(stays.to_vec2()) * Affine::rotate(-value.to_radians()) * Affine::translate(-middle.to_vec2());
            }
        }
    }
}

/// A number that only some kinds of node have, as the panel shows it:
/// angles in degrees, and fractions as percentages.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Param {
    Opacity,
    /// All four corners at once.
    Radius,
    /// The angle an ellipse's arc starts at, clockwise from three o'clock.
    Start,
    /// How much of the ellipse there is: 100 is all of it.
    Sweep,
    /// The hole in an ellipse, or how far out a star's inner corners are.
    Ratio,
    /// A polygon's sides, or a star's points.
    Count,
}

/// An ellipse's arc: where it starts, how far it goes, and its hole.
fn arc(node: &Node) -> (f64, f64, f64) {
    match node.kind {
        NodeKind::Arc { start, sweep, ratio } => (start, sweep, ratio),
        _ => (0.0, std::f64::consts::TAU, 0.0),
    }
}

impl Param {
    /// The ones `node` has, in the order the panel shows them.
    fn of(node: &Node) -> Vec<Param> {
        let own: &[Param] = match node.kind {
            NodeKind::Frame { .. } | NodeKind::Rectangle => &[Param::Radius],
            NodeKind::Ellipse | NodeKind::Arc { .. } => &[Param::Start, Param::Sweep, Param::Ratio],
            NodeKind::Polygon { .. } => &[Param::Count],
            NodeKind::Star { .. } => &[Param::Count, Param::Ratio],
            NodeKind::Page | NodeKind::Group | NodeKind::Line => &[],
        };
        [Param::Opacity].into_iter().chain(own.iter().copied()).collect()
    }

    /// Its label, the unit after it, and the values it can take.
    fn looks(self) -> (&'static str, &'static str, std::ops::RangeInclusive<f64>) {
        match self {
            Param::Opacity => ("Opacity ", "%", 0.0..=100.0),
            Param::Radius => ("Radius ", "", 0.0..=f64::MAX),
            Param::Start => ("Start ", "°", -360.0..=360.0),
            Param::Sweep => ("Sweep ", "%", -100.0..=100.0),
            Param::Ratio => ("Ratio ", "%", 0.0..=100.0),
            Param::Count => ("Count ", "", 3.0..=100.0),
        }
    }

    fn get(self, node: &Node) -> f64 {
        let (start, sweep, hole) = arc(node);
        match (self, &node.kind) {
            (Param::Opacity, _) => node.opacity * 100.0,
            (Param::Radius, _) => node.radii[0],
            (Param::Start, _) => start.to_degrees(),
            (Param::Sweep, _) => sweep / std::f64::consts::TAU * 100.0,
            (Param::Ratio, NodeKind::Star { ratio, .. }) => ratio * 100.0,
            (Param::Ratio, _) => hole * 100.0,
            (Param::Count, NodeKind::Polygon { sides: count } | NodeKind::Star { points: count, .. }) => f64::from(*count),
            (Param::Count, _) => 0.0,
        }
    }

    fn set(self, node: &mut Node, value: f64) {
        let value = value.clamp(*self.looks().2.start(), *self.looks().2.end());
        let (mut start, mut sweep, mut hole) = arc(node);
        match (self, &mut node.kind) {
            (Param::Opacity, _) => node.opacity = value / 100.0,
            (Param::Radius, _) => node.radii = [value; 4],
            (Param::Ratio, NodeKind::Star { ratio, .. }) => *ratio = value / 100.0,
            (Param::Count, NodeKind::Polygon { sides: count } | NodeKind::Star { points: count, .. }) => *count = value.round() as u32,
            (Param::Count, _) => {}
            (Param::Start | Param::Sweep | Param::Ratio, kind) => {
                match self {
                    Param::Start => start = value.to_radians(),
                    Param::Sweep => sweep = value / 100.0 * std::f64::consts::TAU,
                    _ => hole = value / 100.0,
                }
                // All of it, with no hole, is a plain ellipse again.
                let whole = sweep.abs() >= std::f64::consts::TAU && hole == 0.0;
                *kind = if whole { NodeKind::Ellipse } else { NodeKind::Arc { start, sweep, ratio: hole } };
            }
        }
    }
}

/// A change to one of a stack of paints: a node's fills, or its stroke's.
#[derive(Clone, Debug, PartialEq)]
enum PaintEdit {
    /// What the paint is: one colour, or a gradient and everything about it.
    Kind(usize, PaintKind),
    Opacity(usize, f64),
    Visible(usize, bool),
    Remove(usize),
    /// A new paint on top.
    Add,
}

impl PaintEdit {
    /// Makes the change to `paints`; `new` is the colour Add starts with.
    fn apply(self, paints: &mut Vec<Paint>, new: Color) {
        let paint = |paints: &mut Vec<Paint>, index: usize, change: &dyn Fn(&mut Paint)| {
            if let Some(paint) = paints.get_mut(index) {
                change(paint);
            }
        };
        match self {
            PaintEdit::Kind(index, kind) => paint(paints, index, &|paint| paint.kind = kind.clone()),
            PaintEdit::Opacity(index, opacity) => paint(paints, index, &|paint| paint.opacity = opacity.clamp(0.0, 1.0)),
            PaintEdit::Visible(index, visible) => paint(paints, index, &|paint| paint.visible = visible),
            PaintEdit::Remove(index) if index < paints.len() => drop(paints.remove(index)),
            PaintEdit::Remove(_) => {}
            PaintEdit::Add => paints.push(Paint::solid(new)),
        }
    }

    /// Whether the widget that makes this edit is one that's dragged.
    fn dragged(&self) -> bool {
        matches!(self, PaintEdit::Kind(..) | PaintEdit::Opacity(..))
    }
}

/// Figma's grey for a new fill, and its black for a new stroke.
const NEW_FILL: Color = Color::rgb(0xd9, 0xd9, 0xd9);
const NEW_STROKE: Color = Color::rgb(0, 0, 0);

/// The kinds of paint, as the drop-down names them.
const KINDS: [&str; 3] = ["Solid", "Linear", "Radial"];

fn kind_name(kind: &PaintKind) -> &'static str {
    match kind {
        PaintKind::Solid { .. } => KINDS[0],
        PaintKind::Linear { .. } => KINDS[1],
        PaintKind::Radial { .. } => KINDS[2],
    }
}

/// `kind` as the kind of paint called `name`, with what colours it had: a
/// gradient made from one colour fades it out, as Figma's does, and one
/// colour made from a gradient is its first.
fn converted(kind: &PaintKind, name: &str) -> PaintKind {
    let fade = |color| vec![Stop { at: 0.0, color, opacity: 1.0 }, Stop { at: 1.0, color, opacity: 0.0 }];
    let stops = kind.stops().unwrap_or_else(|| fade(kind.color()));
    match name {
        "Linear" => PaintKind::Linear { from: (0.5, 0.0), to: (0.5, 1.0), stops },
        "Radial" => PaintKind::Radial { from: (0.5, 0.5), to: (1.0, 0.5), stops },
        _ => PaintKind::Solid { color: kind.color() },
    }
}

/// A linear gradient's angle in degrees, clockwise from pointing right, and
/// the line through the middle of the box at an angle.
fn angle_of(from: (f64, f64), to: (f64, f64)) -> f64 {
    (to.1 - from.1).atan2(to.0 - from.0).to_degrees()
}

fn at_angle(degrees: f64) -> ((f64, f64), (f64, f64)) {
    let (sin, cos) = degrees.to_radians().sin_cos();
    ((0.5 - cos / 2.0, 0.5 - sin / 2.0), (0.5 + cos / 2.0, 0.5 + sin / 2.0))
}

/// A button that shows `color` and opens a picker for it.
fn color_button(ui: &mut Ui, color: &mut Color) -> bool {
    let mut rgb = [color.r, color.g, color.b];
    let changed = ui.color_edit_button_srgb(&mut rgb).changed();
    *color = Color::rgb(rgb[0], rgb[1], rgb[2]);
    changed
}

/// A stack of paints under `title`, the top one first as in Figma, and the
/// edit the user made to it this frame, if any.
fn paints(ui: &mut Ui, title: &str, paints: &[Paint]) -> Option<PaintEdit> {
    let mut edit = None;
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.strong(title);
        if ui.small_button("+").on_hover_text("Add one").clicked() {
            edit = Some(PaintEdit::Add);
        }
    });
    for (index, paint) in paints.iter().enumerate().rev() {
        let mut kind = paint.kind.clone();
        // Wrapped, so a long row takes a second line and not a wider panel.
        ui.horizontal_wrapped(|ui| {
            let mut name = kind_name(&kind);
            egui::ComboBox::from_id_salt((title, index)).width(56.0).selected_text(name).show_ui(ui, |ui| {
                for other in KINDS {
                    ui.selectable_value(&mut name, other, other);
                }
            });
            if name != kind_name(&kind) {
                kind = converted(&kind, name);
            }
            match &mut kind {
                PaintKind::Solid { color } => {
                    color_button(ui, color);
                    ui.monospace(String::from(*color));
                }
                PaintKind::Linear { from, to, .. } => {
                    let mut degrees = angle_of(*from, *to).round();
                    if ui.add(DragValue::new(&mut degrees).suffix("°")).changed() {
                        (*from, *to) = at_angle(degrees);
                    }
                }
                PaintKind::Radial { .. } => {}
            }
            let mut percent = (paint.opacity * 100.0).round();
            if ui.add(DragValue::new(&mut percent).range(0.0..=100.0).suffix("%")).changed() {
                edit = Some(PaintEdit::Opacity(index, percent / 100.0));
            }
            let mut visible = paint.visible;
            if ui.checkbox(&mut visible, "").on_hover_text("Show it").changed() {
                edit = Some(PaintEdit::Visible(index, visible));
            }
            if ui.small_button("−").on_hover_text("Remove it").clicked() {
                edit = Some(PaintEdit::Remove(index));
            }
        });
        // A gradient's stops, one to a row: colour, place along it, opacity.
        if let PaintKind::Linear { stops, .. } | PaintKind::Radial { stops, .. } = &mut kind {
            let mut removed = None;
            let several = stops.len() > 2;
            for (at, stop) in stops.iter_mut().enumerate() {
                ui.horizontal_wrapped(|ui| {
                    ui.add_space(16.0);
                    color_button(ui, &mut stop.color);
                    let (mut place, mut opacity) = ((stop.at * 100.0).round(), (stop.opacity * 100.0).round());
                    ui.add(DragValue::new(&mut place).range(0.0..=100.0).prefix("at ").suffix("%"));
                    ui.add(DragValue::new(&mut opacity).range(0.0..=100.0).suffix("%"));
                    (stop.at, stop.opacity) = (place / 100.0, opacity / 100.0);
                    if several && ui.small_button("−").on_hover_text("Remove this stop").clicked() {
                        removed = Some(at);
                    }
                });
            }
            if let Some(at) = removed {
                stops.remove(at);
            }
            ui.horizontal(|ui| {
                ui.add_space(16.0);
                if ui.small_button("+ stop").clicked() {
                    // Halfway along, in the colour of the first.
                    let color = stops.first().map_or(NEW_FILL, |stop| stop.color);
                    stops.push(Stop { at: 0.5, color, opacity: 1.0 });
                }
            });
        }
        if kind != paint.kind {
            edit = Some(PaintEdit::Kind(index, kind));
        }
    }
    edit
}

/// The node's export settings, one to a row, and what the user did to them
/// this frame: the settings as they should now be, or a click on Export.
fn exports(ui: &mut Ui, exports: &[Export]) -> (Option<Vec<Export>>, bool) {
    let (mut edited, mut go) = (exports.to_vec(), false);
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.strong("Export");
        if ui.small_button("+").on_hover_text("Add one").clicked() {
            // As Figma does: 1x, then 2x, then 3x.
            let scale = 1.0 + exports.iter().filter(|export| matches!(export, Export::Png { .. })).count() as f64;
            edited.push(Export::Png { scale });
        }
    });
    let mut removed = None;
    for (index, export) in edited.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            let mut svg = *export == Export::Svg;
            egui::ComboBox::from_id_salt(("export", index)).selected_text(if svg { "SVG" } else { "PNG" }).show_ui(ui, |ui| {
                ui.selectable_value(&mut svg, false, "PNG");
                ui.selectable_value(&mut svg, true, "SVG");
            });
            match (svg, &mut *export) {
                (true, _) => *export = Export::Svg,
                (false, Export::Png { scale }) => drop(ui.add(DragValue::new(scale).speed(0.05).range(0.1..=16.0).max_decimals(2).suffix("x"))),
                (false, Export::Svg) => *export = Export::PNG,
            }
            if ui.small_button("−").on_hover_text("Remove it").clicked() {
                removed = Some(index);
            }
        });
    }
    if let Some(index) = removed {
        edited.remove(index);
    }
    if ui.button("Export…").clicked() {
        go = true;
    }
    ((edited != exports).then_some(edited), go)
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

    /// Lays the panel out for the selection, and returns a command if one of
    /// its buttons asked for one.
    pub fn show(&mut self, ui: &mut Ui, history: &mut History, selection: &[NodeId]) -> Result<Option<Command>, Error> {
        let pointer_down = ui.input(|i| i.pointer.any_down());
        self.settle(history, pointer_down);
        let [id] = selection else {
            if selection.len() > 1 {
                ui.weak(format!("{} selected", selection.len()));
            }
            return Ok(None);
        };
        let Some(node) = history.document().node(*id) else { return Ok(None) };
        let exported = node.exports.clone();
        let (values, fills, stroke) = (Field::ALL.map(|field| field.get(node)), node.fills.clone(), node.stroke.clone());
        let params: Vec<(Param, f64)> = Param::of(node).into_iter().map(|param| (param, param.get(node))).collect();
        let (mut clip, mut clip_changed, line) = (if let NodeKind::Frame { clip } = node.kind { Some(clip) } else { None }, false, node.kind == NodeKind::Line);
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
        let mut set = None;
        ui.horizontal_wrapped(|ui| {
            for (param, was) in params {
                let (label, suffix, range) = param.looks();
                let mut value = was;
                ui.add(DragValue::new(&mut value).speed(1.0).range(range).max_decimals(2).prefix(label).suffix(suffix));
                if value != was {
                    set = Some((param, value));
                }
            }
            if let Some(mut clips) = clip
                && ui.checkbox(&mut clips, "Clip content").changed()
            {
                clip = Some(clips);
                clip_changed = true;
            }
        });
        if let Some((param, value)) = set {
            self.change(history, *id, "Shape", pointer_down, |node| param.set(node, value))?;
        }
        if let (true, Some(clip)) = (clip_changed, clip) {
            self.change(history, *id, "Clip Content", false, |node| node.kind = NodeKind::Frame { clip })?;
        }

        if let Some(edit) = paints(ui, "Fill", &fills) {
            let dragged = pointer_down && edit.dragged();
            self.change(history, *id, "Fill", dragged, |node| edit.apply(&mut node.fills, NEW_FILL))?;
        }
        if let Some(edit) = paints(ui, "Stroke", &stroke.paints) {
            let dragged = pointer_down && edit.dragged();
            self.change(history, *id, "Stroke", dragged, |node| edit.apply(&mut node.stroke.paints, NEW_STROKE))?;
        }
        if !stroke.paints.is_empty() {
            let mut edited = stroke.clone();
            // Each choice of a kind, as a drop-down that says which it is.
            fn choice<T: Copy + PartialEq + std::fmt::Debug>(ui: &mut Ui, name: &str, value: &mut T, all: &[T]) {
                egui::ComboBox::from_id_salt(name).selected_text(format!("{value:?}")).show_ui(ui, |ui| {
                    for one in all {
                        ui.selectable_value(value, *one, format!("{one:?}"));
                    }
                });
            }
            ui.horizontal_wrapped(|ui| {
                ui.add(DragValue::new(&mut edited.weight).speed(0.1).range(0.0..=f64::MAX).max_decimals(2).prefix("Weight "));
                // A line has no inside or outside, but it has ends.
                if line {
                    let caps = [Cap::None, Cap::Round, Cap::Square, Cap::Arrow, Cap::Triangle];
                    choice(ui, "stroke start", &mut edited.start_cap, &caps);
                    choice(ui, "stroke end", &mut edited.end_cap, &caps);
                } else {
                    choice(ui, "stroke align", &mut edited.align, &[Align::Inside, Align::Center, Align::Outside]);
                    choice(ui, "stroke join", &mut edited.join, &[Join::Miter, Join::Bevel, Join::Round]);
                }
            });
            if edited != stroke {
                // Only the weight is dragged; the rest is picked from a list.
                self.change(history, *id, "Stroke", pointer_down && edited.weight != stroke.weight, |node| node.stroke = edited)?;
            }
        }
        let (edited, go) = exports(ui, &exported);
        if let Some(edited) = edited {
            // Only a scale is dragged.
            self.change(history, *id, "Export Settings", pointer_down && edited.len() == exported.len(), |node| node.exports = edited)?;
        }
        Ok(go.then_some(Command::Export))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use omavec_engine::{Document, NodeKind};
    use omavec_geom::kurbo::{Point, Size};

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
    fn a_groups_fields_are_those_of_the_box_round_what_is_in_it() {
        let (mut history, id) = rectangle();
        let group = history
            .edit("Group", |document| {
                let page = document.pages[0].id;
                let mut other = document.create(NodeKind::Ellipse, Size::new(50.0, 50.0));
                other.transform = Affine::translate((280.0, 190.0));
                let other = (other.id, document.insert(page, usize::MAX, other)?).0;
                let group = document.group(&[id, other], NodeKind::Group)?;
                // The rectangle leaves the group's corner behind it.
                document.node_mut(id)?.transform = Affine::translate((-20.0, 0.0));
                Ok(group)
            })
            .unwrap();
        assert_eq!(values(&history, group), [10.0, 40.0, 320.0, 200.0, 0.0]);
        type_in(&mut history, group, Field::X, 100.0);
        type_in(&mut history, group, Field::Width, 160.0);
        type_in(&mut history, group, Field::Height, 400.0);
        assert_eq!(values(&history, group), [100.0, 40.0, 160.0, 400.0, 0.0]);
        // The rectangle is half as wide and twice as high, still at the corner.
        assert_eq!(values(&history, id), [-20.0, 0.0, 100.0, 200.0, 0.0]);
        let middle = Point::new(180.0, 240.0);
        type_in(&mut history, group, Field::Rotation, 90.0);
        let node = history.document().node(group).unwrap();
        assert!((node.transform * node.bounds().center() - middle).hypot() < 1e-9, "the middle moved");
    }

    #[test]
    fn each_kind_of_node_has_its_own_numbers() {
        let (mut history, id) = rectangle();
        let kinds = |history: &History| Param::of(history.document().node(id).unwrap());
        let get = |history: &History, param: Param| param.get(history.document().node(id).unwrap());
        let set = |history: &mut History, param: Param, value: f64| Properties::default().change(history, id, "Shape", false, |node| param.set(node, value)).unwrap();
        let become_a = |history: &mut History, kind: NodeKind| history.edit("Kind", |document| document.node_mut(id).map(|node| node.kind = kind)).unwrap();

        assert_eq!(kinds(&history), [Param::Opacity, Param::Radius]);
        set(&mut history, Param::Radius, 12.0);
        set(&mut history, Param::Opacity, 250.0);
        let node = history.document().node(id).unwrap();
        assert_eq!((node.radii, node.opacity), ([12.0; 4], 1.0));
        set(&mut history, Param::Opacity, 40.0);
        assert_eq!(get(&history, Param::Opacity), 40.0);

        // An ellipse becomes an arc when part of it goes, and an ellipse
        // again when all of it is back.
        become_a(&mut history, NodeKind::Ellipse);
        assert_eq!(kinds(&history), [Param::Opacity, Param::Start, Param::Sweep, Param::Ratio]);
        assert_eq!([Param::Start, Param::Sweep, Param::Ratio].map(|param| get(&history, param)), [0.0, 100.0, 0.0]);
        set(&mut history, Param::Sweep, 25.0);
        set(&mut history, Param::Start, 90.0);
        let NodeKind::Arc { start, sweep, ratio } = history.document().node(id).unwrap().kind else { panic!() };
        assert!((start - std::f64::consts::FRAC_PI_2).abs() < 1e-12 && (sweep - std::f64::consts::FRAC_PI_2).abs() < 1e-12 && ratio == 0.0);
        set(&mut history, Param::Ratio, 50.0);
        set(&mut history, Param::Sweep, 100.0);
        assert_eq!(history.document().node(id).unwrap().kind, NodeKind::Arc { start, sweep: std::f64::consts::TAU, ratio: 0.5 }, "a ring");
        set(&mut history, Param::Ratio, 0.0);
        assert_eq!(history.document().node(id).unwrap().kind, NodeKind::Ellipse);

        become_a(&mut history, NodeKind::Star { points: 5, ratio: 0.382 });
        assert_eq!(kinds(&history), [Param::Opacity, Param::Count, Param::Ratio]);
        set(&mut history, Param::Count, 7.4);
        set(&mut history, Param::Ratio, 60.0);
        assert_eq!(history.document().node(id).unwrap().kind, NodeKind::Star { points: 7, ratio: 0.6 });
        become_a(&mut history, NodeKind::Polygon { sides: 3 });
        set(&mut history, Param::Count, 1.0);
        assert_eq!(history.document().node(id).unwrap().kind, NodeKind::Polygon { sides: 3 }, "no fewer than three");
        set(&mut history, Param::Count, 8.0);
        assert_eq!((get(&history, Param::Count), kinds(&history)), (8.0, vec![Param::Opacity, Param::Count]));
    }

    #[test]
    fn a_colour_becomes_a_gradient_and_back_keeping_what_it_can() {
        let red = Color::rgb(255, 0, 0);
        let solid = PaintKind::Solid { color: red };
        // It fades the colour out, top to bottom, as Figma's does.
        let linear = converted(&solid, "Linear");
        let fade = vec![Stop { at: 0.0, color: red, opacity: 1.0 }, Stop { at: 1.0, color: red, opacity: 0.0 }];
        assert_eq!(linear, PaintKind::Linear { from: (0.5, 0.0), to: (0.5, 1.0), stops: fade.clone() });
        assert_eq!(converted(&linear, "Radial"), PaintKind::Radial { from: (0.5, 0.5), to: (1.0, 0.5), stops: fade });
        assert_eq!((converted(&linear, "Solid"), kind_name(&linear)), (solid, "Linear"));
        // Top to bottom is 90°; a line at an angle goes through the middle.
        assert_eq!(angle_of((0.5, 0.0), (0.5, 1.0)), 90.0);
        let (from, to) = at_angle(0.0);
        assert_eq!((from, to), ((0.0, 0.5), (1.0, 0.5)));
        let (from, to) = at_angle(135.0);
        assert!((angle_of(from, to) - 135.0).abs() < 1e-9 && ((from.0 + to.0) / 2.0 - 0.5).abs() < 1e-12);
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
    fn a_stroke_starts_black_and_dragging_its_weight_is_one_step() {
        let (mut history, id) = rectangle();
        let mut panel = Properties::default();
        panel.change(&mut history, id, "Stroke", false, |node| PaintEdit::Add.apply(&mut node.stroke.paints, NEW_STROKE)).unwrap();
        for weight in [1.5, 2.0, 4.0] {
            panel.change(&mut history, id, "Stroke", true, |node| node.stroke.weight = weight).unwrap();
        }
        panel.settle(&mut history, false);
        panel.change(&mut history, id, "Stroke", false, |node| node.stroke.align = Align::Outside).unwrap();
        let stroke = |history: &History| history.document().node(id).unwrap().stroke.clone();
        assert_eq!(stroke(&history), omavec_engine::Stroke { paints: vec![Paint::solid(NEW_STROKE)], weight: 4.0, align: Align::Outside, ..Default::default() });
        history.undo();
        assert_eq!(stroke(&history).align, Align::Inside);
        history.undo();
        assert_eq!(stroke(&history).weight, 1.0);
        history.undo();
        assert!(stroke(&history).paints.is_empty());
    }

    #[test]
    fn fills_are_edited_one_at_a_time() {
        let (mut history, id) = rectangle();
        let mut panel = Properties::default();
        let fills = |history: &History| history.document().node(id).unwrap().fills.clone();
        let mut edit = |history: &mut History, edit: PaintEdit| panel.change(history, id, "Fill", false, |node| edit.apply(&mut node.fills, NEW_FILL)).unwrap();
        let (grey, red) = (Color::rgb(0xd9, 0xd9, 0xd9), Color::rgb(255, 0, 0));
        assert_eq!(fills(&history), [Paint::solid(grey)]);

        edit(&mut history, PaintEdit::Add);
        edit(&mut history, PaintEdit::Kind(1, PaintKind::Solid { color: red }));
        edit(&mut history, PaintEdit::Opacity(1, 0.4));
        edit(&mut history, PaintEdit::Visible(0, false));
        let expected = [Paint { visible: false, ..Paint::solid(grey) }, Paint { opacity: 0.4, ..Paint::solid(red) }];
        assert_eq!(fills(&history), expected);
        // Opacity stays between nothing and everything.
        edit(&mut history, PaintEdit::Opacity(1, 7.0));
        assert_eq!(fills(&history)[1].opacity, 1.0);
        history.undo();

        edit(&mut history, PaintEdit::Remove(0));
        assert_eq!(fills(&history), expected[1..]);
        // A fill that isn't there: nothing happens, and nothing to undo.
        let steps = history.undo_name().map(str::to_owned);
        edit(&mut history, PaintEdit::Remove(5));
        edit(&mut history, PaintEdit::Kind(5, PaintKind::Solid { color: grey }));
        assert_eq!(fills(&history), expected[1..]);
        assert_eq!(history.undo_name().map(str::to_owned), steps);
        history.undo();
        assert_eq!(fills(&history), expected);
    }
}
