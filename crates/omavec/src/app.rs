use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};

use egui::{Button, RichText, Ui};
use omavec_engine::display::DisplayList;
use omavec_engine::{Document, Error, History, NodeId, NodeKind, Stack, file};
use omavec_geom::kurbo::{Point, Rect, Vec2};

use crate::canvas::{Canvas, Overlay, Pointer};
use crate::clipboard::Clipboard;
use crate::commands::Command;
use crate::layers_panel::{self, LayersPanel};
use crate::properties::Properties;
use crate::theme::{self, Theme};
use crate::tools::{Grab, Keys, Tool, Tools};

/// The tools in the tool bar, with the command that picks each.
const TOOLS: [(Tool, Command); 9] = [
    (Tool::Move, Command::MoveTool),
    (Tool::Hand, Command::HandTool),
    (Tool::Frame, Command::FrameTool),
    (Tool::Rectangle, Command::RectangleTool),
    (Tool::Ellipse, Command::EllipseTool),
    (Tool::Polygon, Command::PolygonTool),
    (Tool::Star, Command::StarTool),
    (Tool::Line, Command::LineTool),
    (Tool::Arrow, Command::ArrowTool),
];

pub struct App {
    theme: Theme,
    /// New themes from the watcher; `None` in tests.
    theme_rx: Option<Receiver<Theme>>,
    /// Whether the panels around the canvas are shown (Ctrl+\).
    show_ui: bool,
    canvas: Canvas,
    history: History,
    tools: Tools,
    layers: LayersPanel,
    properties: Properties,
    /// The page on the canvas.
    page: NodeId,
    /// The revision of the document the canvas is drawing.
    drawn: Option<u64>,
    /// Phase 0's test scene is on the canvas instead of the document.
    spike: bool,
    /// The folder the document is saved in, once it has one.
    path: Option<PathBuf>,
    /// An open file dialog and the command its answer is for.
    dialog: Option<(Command, Receiver<Option<PathBuf>>)>,
    /// The last thing worth saying, and whether it went wrong.
    status: Option<(String, bool)>,
    /// The window title as last set.
    title: String,
    /// A command that would throw away unsaved changes, waiting for an answer.
    confirm: Option<Command>,
    /// What to carry on with once a save asked for by that answer is done.
    after_save: Option<Command>,
    /// The window may close: its changes are saved or given up.
    closing: bool,
    clipboard: Clipboard,
    /// Whether V is held, by which Ctrl+V is told from V (see `Command::pressed`).
    v_down: bool,
}

/// The answers to "save your changes first?".
#[derive(Clone, Copy, Debug, PartialEq)]
enum Choice {
    Save,
    Discard,
    Cancel,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, open: Option<PathBuf>) -> Self {
        let ctx = &cc.egui_ctx;
        // Ctrl+= / Ctrl+- zoom the canvas, not the interface.
        ctx.options_mut(|o| o.zoom_with_keyboard = false);
        theme::install_font(ctx);
        // Phase 0's test scene: `OMAVEC_BLOBS=10000 omavec` scatters that many
        // random paths, to try panning and zooming by hand.
        let blobs = std::env::var("OMAVEC_BLOBS").ok().and_then(|count| count.parse().ok());
        let mut app = Self::with_theme(Theme::load(), blobs);
        ctx.set_visuals(app.theme.visuals());
        app.theme_rx = Some(theme::watch(ctx.clone()));
        app.clipboard.system = true;
        if let Some(path) = open {
            app.open(&path);
        }
        app
    }

    fn with_theme(theme: Theme, blobs: Option<usize>) -> Self {
        let mut canvas = match blobs {
            Some(count) => {
                let middle = omavec_render::spike::DOCUMENT / 2.0;
                Canvas::new(omavec_render::spike::blobs(count), Some((middle, middle).into()))
            }
            None => Canvas::new(Default::default(), None),
        };
        canvas.readout = blobs.is_some();
        let document = Document::default();
        let page = document.pages[0].id;
        Self { theme, theme_rx: None, show_ui: true, canvas, history: History::new(document), tools: Tools::default(), layers: LayersPanel::default(), properties: Properties::default(), page, drawn: None, spike: blobs.is_some(), path: None, dialog: None, status: None, title: String::new(), confirm: None, after_save: None, closing: false, clipboard: Clipboard::default(), v_down: false }
    }

    fn say(&mut self, message: impl Into<String>, wrong: bool) {
        let message = message.into();
        if wrong {
            log::warn!("{message}");
        }
        self.status = Some((message, wrong));
    }

    /// Whatever the tools and the engine refuse is said, not fatal.
    fn check<T>(&mut self, result: Result<T, Error>) {
        if let Err(error) = result {
            self.say(error.to_string(), true);
        }
    }

    /// Changes the selected nodes as one undo step called `name`, and selects
    /// what the change returns. With nothing selected there's nothing to do.
    fn arrange(&mut self, name: &str, change: impl FnOnce(&mut Document, &[NodeId]) -> Result<Vec<NodeId>, Error>) {
        if self.tools.selection.is_empty() {
            return;
        }
        let selection = &self.tools.selection;
        match self.history.edit(name, |document| change(document, selection)) {
            Ok(selected) => self.tools.selection = selected,
            Err(error) => self.say(error.to_string(), true),
        }
    }

    /// Puts `document` on the canvas in place of the one that was there.
    fn set_document(&mut self, document: Document, path: Option<PathBuf>) {
        let Some(page) = document.pages.first().map(|page| page.id) else { return };
        (self.history, self.tools, self.page, self.path, self.drawn) = (History::new(document), Tools::default(), page, path, None);
    }

    fn open(&mut self, path: &Path) {
        // The file dialog can't pick a folder and a file both, so a folder
        // is opened by the `document.json` in it.
        let folder = if path.file_name().is_some_and(|name| name == "document.json") { path.parent().unwrap_or(path) } else { path };
        match file::open(folder) {
            Ok(document) => {
                self.set_document(document, Some(folder.into()));
                self.status = None;
            }
            Err(error) => self.say(format!("Couldn't open: {error}"), true),
        }
    }

    fn save_to(&mut self, folder: &Path) {
        match file::save(self.history.document(), folder) {
            Ok(()) => {
                self.history.mark_saved();
                self.path = Some(folder.into());
                self.say(format!("Saved {}", name_of(folder)), false);
            }
            Err(error) => {
                // Whatever was waiting on this save doesn't happen.
                self.after_save = None;
                self.say(format!("Couldn't save: {error}"), true);
            }
        }
    }

    /// What Export exports: the selection, or with nothing selected every
    /// top-level frame on the page.
    fn exported(&self) -> Vec<NodeId> {
        let document = self.history.document();
        if !self.tools.selection.is_empty() {
            return document.roots(&self.tools.selection);
        }
        let frames = document.node(self.page).into_iter().flat_map(|page| &page.children).filter(|node| matches!(node.kind, NodeKind::Frame { .. }));
        frames.map(|node| node.id).collect()
    }

    /// Writes what Export exports into `folder`, each node as its own
    /// export settings say.
    fn export_to(&mut self, folder: &Path) {
        match crate::export::write(self.history.document(), &self.exported(), &[], folder) {
            Ok(files) => self.say(format!("Exported {} file{} into {}", files.len(), if files.len() == 1 { "" } else { "s" }, folder.display()), false),
            Err(error) => self.say(format!("Couldn't export: {error}"), true),
        }
    }

    /// Opens a file dialog off the UI thread; `answer` takes what it returns.
    fn ask(&mut self, command: Command, ctx: &egui::Context) {
        if self.dialog.is_some() {
            return;
        }
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        let start = self.path.as_deref().and_then(Path::parent).map(Path::to_path_buf);
        std::thread::spawn(move || {
            let mut dialog = rfd::FileDialog::new();
            if let Some(start) = start {
                dialog = dialog.set_directory(start);
            }
            let picked = match command {
                Command::Open => dialog.set_title("Open a .omavecz, or the document.json in a .omavec folder").add_filter("Omavec documents", &["omavecz", "json"]).pick_file(),
                Command::Export => dialog.set_title("Export into").pick_folder(),
                _ => dialog.set_title("Save As").set_file_name("Untitled.omavec").save_file(),
            };
            let _ = tx.send(picked);
            ctx.request_repaint();
        });
        self.dialog = Some((command, rx));
    }

    fn answer(&mut self) {
        let Some((command, rx)) = &self.dialog else { return };
        let Ok(picked) = rx.try_recv() else { return };
        let command = *command;
        self.dialog = None;
        match (command, picked) {
            (Command::Open, Some(folder)) => self.open(&folder),
            (Command::Export, Some(folder)) => self.export_to(&folder),
            (Command::Export, None) => {}
            (_, Some(mut folder)) => {
                // A folder unless it's named as the zipped kind.
                if folder.extension().is_none_or(|extension| extension != "omavec" && extension != "omavecz") {
                    folder.as_mut_os_string().push(".omavec");
                }
                self.save_to(&folder);
            }
            (_, None) => self.after_save = None,
        }
    }

    /// Does what New, Open and Quit do, with nothing left to lose.
    fn proceed(&mut self, command: Command, ctx: &egui::Context) {
        match command {
            Command::New => self.set_document(Document::default(), None),
            Command::Open => self.ask(command, ctx),
            _ => {
                self.closing = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    /// The answer to the question `confirm` put.
    fn decide(&mut self, choice: Choice, ctx: &egui::Context) {
        let Some(command) = self.confirm.take() else { return };
        match choice {
            Choice::Save => {
                self.after_save = Some(command);
                self.run(Command::Save, ctx);
            }
            Choice::Discard => self.proceed(command, ctx),
            Choice::Cancel => {}
        }
    }

    fn run(&mut self, command: Command, ctx: &egui::Context) {
        match command {
            // These replace the document or close it: ask first if it has
            // changes that aren't saved.
            Command::New | Command::Open | Command::Quit if self.history.is_dirty() => self.confirm = Some(command),
            Command::New | Command::Open | Command::Quit => self.proceed(command, ctx),
            Command::SaveAs => self.ask(command, ctx),
            Command::Export if self.exported().is_empty() => self.say("Nothing to export: select something, or draw a frame", true),
            Command::Export => self.ask(command, ctx),
            Command::Save => match self.path.clone() {
                Some(folder) => self.save_to(&folder),
                None => self.ask(Command::SaveAs, ctx),
            },
            Command::Undo => drop(self.history.undo()),
            Command::Redo => drop(self.history.redo()),
            Command::Delete => {
                let deleted = self.tools.delete(&mut self.history);
                self.check(deleted);
            }
            Command::Copy | Command::Cut => {
                if self.clipboard.copy(self.history.document(), &self.tools.selection) && command == Command::Cut {
                    self.run(Command::Delete, ctx);
                }
            }
            Command::Paste => {
                let nodes = self.clipboard.nodes().to_vec();
                let document = self.history.document();
                // Into the frame or group that is selected, or beside whatever
                // else is, or onto the page.
                let parent = match self.tools.selection[..] {
                    [one] if document.node(one).is_some_and(|node| node.kind.is_container()) => one,
                    [first, ..] => document.parent(first).unwrap_or(self.page),
                    [] => self.page,
                };
                if !nodes.is_empty() {
                    match self.history.edit("Paste", |document| document.paste(parent, &nodes)) {
                        Ok(pasted) => self.tools.selection = pasted,
                        Err(error) => self.say(error.to_string(), true),
                    }
                }
            }
            Command::Duplicate => self.arrange("Duplicate", |document, selection| document.duplicate(selection)),
            Command::Group => self.arrange("Group", |document, selection| Ok(vec![document.group(selection, NodeKind::Group)?])),
            Command::FrameSelection => self.arrange("Frame Selection", |document, selection| Ok(vec![document.group(selection, NodeKind::Frame { clip: false })?])),
            Command::Ungroup => self.arrange("Ungroup", |document, selection| {
                // What isn't a group or a frame stays as it is, and selected.
                let mut freed = Vec::new();
                for id in document.roots(selection) {
                    match document.ungroup(id) {
                        Ok(inside) => freed.extend(inside),
                        Err(_) => freed.push(id),
                    }
                }
                Ok(freed)
            }),
            Command::BringToFront | Command::BringForward | Command::SendBackward | Command::SendToBack => {
                let to = match command {
                    Command::BringToFront => Stack::Front,
                    Command::BringForward => Stack::Forward,
                    Command::SendBackward => Stack::Backward,
                    _ => Stack::Back,
                };
                self.arrange(command.label(), |document, selection| document.restack(selection, to).map(|()| selection.to_vec()));
            }
            Command::SelectAll | Command::SelectChildren | Command::SelectParent => {
                let document = self.history.document();
                let selection = &self.tools.selection;
                // What can be clicked can be selected.
                let inside = |id: NodeId| document.node(id).into_iter().flat_map(|node| &node.children).filter(|child| child.visible && !child.locked).map(|child| child.id).collect::<Vec<_>>();
                let found: Vec<NodeId> = match command {
                    // Everything beside what is selected, or on the page.
                    Command::SelectAll => inside(selection.first().and_then(|first| document.parent(*first)).unwrap_or(self.page)),
                    Command::SelectChildren => selection.iter().flat_map(|id| inside(*id)).collect(),
                    _ => {
                        let mut parents: Vec<NodeId> = selection.iter().filter_map(|id| document.parent(*id)).filter(|parent| *parent != self.page).collect();
                        parents.dedup();
                        parents
                    }
                };
                // Nothing further in, or further out: stay.
                if !found.is_empty() {
                    self.tools.selection = found;
                }
            }
            Command::ToggleVisible => {
                let toggled = layers_panel::toggle_visible(&mut self.history, &self.tools.selection);
                self.check(toggled);
            }
            Command::ToggleLocked => {
                let toggled = layers_panel::toggle_locked(&mut self.history, &self.tools.selection);
                self.check(toggled);
            }
            Command::Cancel => self.tools.cancel(&mut self.history),
            Command::NudgeLeft | Command::NudgeRight | Command::NudgeUp | Command::NudgeDown => {
                // One unit, or ten with Shift, as in Figma.
                let step = if ctx.input(|i| i.modifiers.shift) { 10.0 } else { 1.0 };
                let by = match command {
                    Command::NudgeLeft => Vec2::new(-step, 0.0),
                    Command::NudgeRight => Vec2::new(step, 0.0),
                    Command::NudgeUp => Vec2::new(0.0, -step),
                    _ => Vec2::new(0.0, step),
                };
                let nudged = self.tools.nudge(&mut self.history, by);
                self.check(nudged);
            }
            Command::MoveTool | Command::HandTool | Command::FrameTool | Command::RectangleTool | Command::EllipseTool | Command::PolygonTool | Command::StarTool | Command::LineTool | Command::ArrowTool => {
                if let Some((tool, _)) = TOOLS.iter().find(|(_, picks)| *picks == command) {
                    self.tools.tool = *tool;
                }
            }
            Command::ZoomIn => self.canvas.zoom_by(2.0),
            Command::ZoomOut => self.canvas.zoom_by(0.5),
            Command::ZoomTo100 => self.canvas.zoom_by(1.0 / self.canvas.view.zoom),
            Command::ZoomToFit | Command::ZoomToSelection => {
                // Everything on the page, or what is selected.
                let document = self.history.document();
                let all = || document.node(self.page).map(|page| page.children.iter().map(|node| node.id).collect()).unwrap_or_default();
                let nodes: Vec<NodeId> = if command == Command::ZoomToFit { all() } else { self.tools.selection.clone() };
                let area = self.corners(&nodes).into_iter().flatten().fold(None, |area: Option<Rect>, corner| {
                    Some(area.map_or(Rect::from_points(corner, corner), |area| area.union_pt(corner)))
                });
                if let Some(area) = area {
                    self.canvas.fit(area);
                }
            }
            Command::ToggleRulers => self.canvas.rulers = !self.canvas.rulers,
            Command::TogglePixelGrid => self.canvas.pixel_grid = !self.canvas.pixel_grid,
            Command::ToggleSnap => {
                self.tools.snap = !self.tools.snap;
                self.say(if self.tools.snap { "Snapping on" } else { "Snapping off" }, false);
            }
            Command::ToggleUi => self.show_ui = !self.show_ui,
        }
    }

    fn menu_item(&mut self, ui: &mut Ui, command: Command) {
        let mut button = Button::new(command.label());
        if let Some(shortcut) = command.shortcut() {
            button = button.shortcut_text(ui.ctx().format_shortcut(&shortcut));
        }
        if ui.add(button).clicked() {
            ui.close();
            self.run(command, ui.ctx());
        }
    }

    fn menu_bar(&mut self, ui: &mut Ui) {
        const MENUS: [(&str, &[Command]); 4] = [
            ("File", &[Command::New, Command::Open, Command::Save, Command::SaveAs, Command::Export, Command::Quit]),
            ("Edit", &[Command::Undo, Command::Redo, Command::Cut, Command::Copy, Command::Paste, Command::Duplicate, Command::Delete, Command::SelectAll, Command::SelectChildren, Command::SelectParent]),
            ("Object", &[Command::Group, Command::Ungroup, Command::FrameSelection, Command::BringToFront, Command::BringForward, Command::SendBackward, Command::SendToBack, Command::ToggleVisible, Command::ToggleLocked]),
            ("View", &[Command::ZoomIn, Command::ZoomOut, Command::ZoomTo100, Command::ZoomToFit, Command::ZoomToSelection, Command::ToggleRulers, Command::TogglePixelGrid, Command::ToggleSnap, Command::ToggleUi]),
        ];
        egui::MenuBar::new().ui(ui, |ui| {
            for (menu, commands) in MENUS {
                ui.menu_button(menu, |ui| {
                    for command in commands {
                        self.menu_item(ui, *command);
                    }
                });
            }
        });
    }

    /// The tools in a row, the one in use picked out, each with its letter.
    fn tool_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            for (tool, command) in TOOLS {
                let letter = command.shortcut().map(|shortcut| ui.ctx().format_shortcut(&shortcut)).unwrap_or_default();
                let colour = if self.tools.tool == tool { self.theme.accent } else { self.theme.dark_foreground };
                let button = Button::new(RichText::new(format!("{} {letter}", command.label())).color(colour)).frame(false);
                if ui.add(button).clicked() {
                    self.run(command, ui.ctx());
                }
                ui.add_space(8.0);
            }
        });
    }

    /// "Save your changes?", over everything else until it's answered.
    fn confirm(&mut self, ctx: &egui::Context) {
        if self.confirm.is_none() {
            return;
        }
        let name = self.path.as_deref().map_or_else(|| "Untitled".into(), name_of);
        let mut choice = None;
        let modal = egui::Modal::new(egui::Id::new("confirm")).show(ctx, |ui| {
            ui.label(format!("Save the changes to {name}?"));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                for (label, answer) in [("Save", Choice::Save), ("Don't Save", Choice::Discard), ("Cancel", Choice::Cancel)] {
                    if ui.button(label).clicked() {
                        choice = Some(answer);
                    }
                }
            });
        });
        // Esc, or a click outside it, is Cancel.
        if modal.should_close() {
            choice = choice.or(Some(Choice::Cancel));
        }
        if let Some(choice) = choice {
            self.decide(choice, ctx);
        }
    }

    fn keys(&mut self, ctx: &egui::Context) {
        // While a text field has focus, keys edit the text; while a question
        // is up, it is answered first.
        if !ctx.egui_wants_keyboard_input() && self.confirm.is_none() {
            for command in Command::pressed(ctx, &mut self.v_down) {
                self.run(command, ctx);
            }
        }
    }

    /// The corners of each node's box, on the page.
    fn corners(&self, nodes: &[NodeId]) -> Vec<[Point; 4]> {
        let document = self.history.document();
        let corners = |id: &NodeId| {
            let (to_page, outline) = (document.to_page(*id)?, document.node(*id)?.bounds());
            Some([(outline.x0, outline.y0), (outline.x1, outline.y0), (outline.x1, outline.y1), (outline.x0, outline.y1)].map(|corner| to_page * Point::from(corner)))
        };
        nodes.iter().filter_map(corners).collect()
    }

    /// The pointer to show at `at` on the canvas: what a press there would do.
    /// `corners` are those of the selection's box, on the page.
    fn cursor(&self, at: Point, corners: Option<[Point; 4]>) -> egui::CursorIcon {
        use egui::CursorIcon;
        let grab = match self.tools.tool {
            Tool::Hand => return CursorIcon::Grab,
            Tool::Move => self.tools.grab_at(self.history.document(), at),
            _ => return CursorIcon::Crosshair,
        };
        match (grab, corners) {
            (Some(Grab::Resize(handle)), Some([a, b, _, d])) => {
                // Which way the handle pulls on screen, in eighths of a turn.
                let pull = (b - a).normalize() * (handle.0 - 0.5) + (d - a).normalize() * (handle.1 - 0.5);
                match (pull.atan2() / std::f64::consts::FRAC_PI_4).round().rem_euclid(4.0) as u8 {
                    0 => CursorIcon::ResizeHorizontal,
                    1 => CursorIcon::ResizeNwSe,
                    2 => CursorIcon::ResizeVertical,
                    _ => CursorIcon::ResizeNeSw,
                }
            }
            (Some(Grab::Rotate), _) => CursorIcon::Alias,
            _ => CursorIcon::Default,
        }
    }

    /// Lays out the window and returns the rectangle the canvas got.
    fn show(&mut self, ui: &mut Ui) -> egui::Rect {
        // Hand the canvas the document whenever it has changed.
        if !self.spike && self.drawn != Some(self.history.revision()) {
            let document = self.history.document();
            self.tools.forget_missing(document);
            if let Some(page) = document.node(self.page) {
                self.canvas.set_list(DisplayList::of(page));
            }
            self.drawn = Some(self.history.revision());
        }
        // A command a panel's button asked for, run once the panels are laid out.
        let mut wanted = None;
        let bar = egui::Frame::new()
            .fill(self.theme.dark_background)
            .inner_margin(egui::Margin::symmetric(8, 4));
        if self.show_ui {
            egui::Panel::top("menu").frame(bar).show(ui, |ui| self.menu_bar(ui));
            egui::Panel::top("tools").frame(bar).show(ui, |ui| self.tool_bar(ui));
            if let Some((message, wrong)) = &self.status {
                let colour = if *wrong { self.theme.red } else { self.theme.dark_foreground };
                egui::Panel::bottom("status").frame(bar).show(ui, |ui| ui.colored_label(colour, message));
            }
            egui::Panel::left("layers")
                .frame(bar)
                .default_size(240.0)
                .resizable(true)
                .show(ui, |ui| {
                    ui.strong("Layers");
                    ui.separator();
                    let shown = self.layers.show(ui, &mut self.history, self.page, &mut self.tools.selection, &self.theme);
                    self.check(shown);
                    ui.take_available_space();
                });
            egui::Panel::right("properties")
                .frame(bar)
                .default_size(240.0)
                .resizable(true)
                .show(ui, |ui| {
                    ui.strong("Design");
                    ui.separator();
                    match self.properties.show(ui, &mut self.history, &self.tools.selection) {
                        Ok(asked) => wanted = asked,
                        Err(error) => self.say(error.to_string(), true),
                    }
                    ui.take_available_space();
                });
        }
        if let Some(command) = wanted {
            self.run(command, ui.ctx());
        }
        let document = self.history.document();
        let handles = self.tools.frame(document).map(|(to_page, area)| [(area.x0, area.y0), (area.x1, area.y0), (area.x1, area.y1), (area.x0, area.y1)].map(|corner| to_page * Point::from(corner)));
        let overlay = Overlay { outlines: self.corners(&self.tools.selection), handles, marquee: self.tools.marquee(), guides: self.tools.guides.clone() };
        self.canvas.hand = self.tools.tool == Tool::Hand;
        let (rect, pointer) = egui::CentralPanel::no_frame().show(ui, |ui| self.canvas.show(ui, &self.theme, &overlay)).inner;
        let keys = ui.input(|i| Keys { shift: i.modifiers.shift, alt: i.modifiers.alt, ctrl: i.modifiers.command });
        // A handle is taken from six points away, whatever the zoom.
        self.tools.grab = 6.0 / self.canvas.view.zoom;
        for event in pointer {
            let done = match event {
                Pointer::Press(at) => {
                    self.tools.press(&self.history, self.page, at, keys);
                    Ok(())
                }
                Pointer::Drag(at) => self.tools.drag(&mut self.history, at, keys),
                Pointer::Release => self.tools.release(&mut self.history),
                Pointer::Double(at) => {
                    self.tools.enter(&self.history, self.page, at);
                    Ok(())
                }
            };
            self.check(done);
        }
        if let Some(at) = self.canvas.hover {
            ui.ctx().set_cursor_icon(self.cursor(at, handles));
        }
        rect
    }

    /// Keeps the window title on the document's name, with a dot while it
    /// has changes that aren't saved.
    fn update_title(&mut self, ctx: &egui::Context) {
        let name = self.path.as_deref().map_or_else(|| "Untitled".into(), name_of);
        let title = format!("{}{name} — Omavec", if self.history.is_dirty() { "● " } else { "" });
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }
}

/// A document's name: its folder's, without `.omavec`.
fn name_of(folder: &Path) -> String {
    folder.file_stem().unwrap_or(folder.as_os_str()).to_string_lossy().into_owned()
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(theme) = self.theme_rx.as_ref().and_then(|rx| rx.try_iter().last()) {
            ctx.set_visuals(theme.visuals());
            self.theme = theme;
        }
        self.answer();
        // A save that an answer asked for has finished: carry on with what
        // was being done.
        if !self.history.is_dirty()
            && self.dialog.is_none()
            && let Some(command) = self.after_save.take()
        {
            self.proceed(command, ctx);
        }
        // The compositor closing the window is Quit by another road.
        if ctx.input(|i| i.viewport().close_requested()) && !self.closing && self.history.is_dirty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm = Some(Command::Quit);
        }
        self.keys(ctx);
        self.update_title(ctx);
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
        self.confirm(ui.ctx());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, Key, Modifiers, PointerButton, Pos2, pos2, vec2};
    use omavec_engine::Export;
    use omavec_geom::kurbo::Vec2;

    /// One headless frame of the whole window with `events`. Returns the
    /// rectangle left for the canvas.
    fn frame(ctx: &egui::Context, app: &mut App, events: Vec<Event>) -> egui::Rect {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 600.0))),
            events,
            ..Default::default()
        };
        let mut canvas = egui::Rect::NOTHING;
        let mut output = ctx.run_ui(input, |ui| {
            app.keys(ui.ctx());
            canvas = app.show(ui);
        });
        // There's no renderer to upload textures to.
        output.textures_delta.clear();
        canvas
    }

    fn key(key: Key, modifiers: Modifiers) -> Vec<Event> {
        let press = |pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers };
        vec![Event::ModifiersChanged(modifiers), press(true), press(false)]
    }

    /// Presses the primary button at `from`, drags to `to` and lets go.
    fn drag(ctx: &egui::Context, app: &mut App, from: Pos2, to: Pos2) {
        let button = |pressed, pos| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        frame(ctx, app, vec![Event::PointerMoved(from)]);
        frame(ctx, app, vec![button(true, from)]);
        frame(ctx, app, vec![Event::PointerMoved(from + (to - from) / 2.0)]);
        frame(ctx, app, vec![Event::PointerMoved(to)]);
        frame(ctx, app, vec![button(false, to)]);
    }

    /// The boxes of the page's children on the page, back to front.
    fn boxes(app: &App) -> Vec<(NodeKind, Rect)> {
        let document = app.history.document();
        let page = document.node(app.page).unwrap();
        page.children.iter().map(|node| (node.kind.clone(), document.to_page(node.id).unwrap().transform_rect_bbox(Rect::from_origin_size((0.0, 0.0), node.size)))).collect()
    }

    fn app() -> (egui::Context, App, egui::Rect) {
        let ctx = egui::Context::default();
        let mut app = App::with_theme(Theme::default(), None);
        let canvas = frame(&ctx, &mut app, vec![]);
        (ctx, app, canvas)
    }

    #[test]
    fn r_and_a_drag_draw_a_rectangle_that_undo_takes_away() {
        let (ctx, mut app, canvas) = app();
        frame(&ctx, &mut app, key(Key::R, Modifiers::NONE));
        assert_eq!(app.tools.tool, Tool::Rectangle);
        drag(&ctx, &mut app, canvas.min + vec2(50.0, 60.0), canvas.min + vec2(250.0, 160.0));
        let drawn = [(NodeKind::Rectangle, Rect::new(50.0, 60.0, 250.0, 160.0))];
        assert_eq!(boxes(&app), drawn);
        assert_eq!(app.tools.tool, Tool::Move);
        assert_eq!(app.tools.selection.len(), 1);
        assert!(app.history.is_dirty());

        frame(&ctx, &mut app, key(Key::Z, Modifiers::COMMAND));
        assert!(boxes(&app).is_empty());
        // The selection doesn't go on naming a node that's gone.
        frame(&ctx, &mut app, vec![]);
        assert!(app.tools.selection.is_empty());
        frame(&ctx, &mut app, key(Key::Z, Modifiers::COMMAND | Modifiers::SHIFT));
        assert_eq!(boxes(&app), drawn);
    }

    #[test]
    fn a_shape_is_drawn_where_the_view_shows_the_document() {
        let (ctx, mut app, canvas) = app();
        app.canvas.view = crate::canvas::View { origin: Vec2::new(100.0, 50.0), zoom: 2.0 };
        frame(&ctx, &mut app, key(Key::O, Modifiers::NONE));
        drag(&ctx, &mut app, canvas.min + vec2(100.0, 50.0), canvas.min + vec2(300.0, 250.0));
        assert_eq!(boxes(&app), [(NodeKind::Ellipse, Rect::new(0.0, 0.0, 100.0, 100.0))]);
    }

    #[test]
    fn a_drag_with_the_move_tool_moves_and_delete_deletes() {
        let (ctx, mut app, canvas) = app();
        frame(&ctx, &mut app, key(Key::F, Modifiers::NONE));
        drag(&ctx, &mut app, canvas.min + vec2(100.0, 100.0), canvas.min + vec2(200.0, 200.0));
        // Click off it, then drag it by (30, 40).
        drag(&ctx, &mut app, canvas.min + vec2(400.0, 400.0), canvas.min + vec2(400.0, 400.0));
        assert!(app.tools.selection.is_empty());
        drag(&ctx, &mut app, canvas.min + vec2(150.0, 150.0), canvas.min + vec2(180.0, 190.0));
        assert_eq!(boxes(&app), [(NodeKind::Frame { clip: true }, Rect::new(130.0, 140.0, 230.0, 240.0))]);
        assert_eq!(app.history.undo_name(), Some("Move"));

        frame(&ctx, &mut app, key(Key::Delete, Modifiers::NONE));
        assert!(boxes(&app).is_empty());
        frame(&ctx, &mut app, key(Key::Z, Modifiers::COMMAND));
        frame(&ctx, &mut app, key(Key::Backspace, Modifiers::NONE));
        assert_eq!(boxes(&app).len(), 1, "nothing was selected after the undo");
    }

    #[test]
    fn a_corner_resizes_at_any_zoom_and_arrows_nudge() {
        let (ctx, mut app, canvas) = app();
        app.canvas.view = crate::canvas::View { origin: Vec2::new(0.0, 0.0), zoom: 4.0 };
        frame(&ctx, &mut app, key(Key::R, Modifiers::NONE));
        // On screen 40..200 by 40..120: in the document 10..50 by 10..30.
        drag(&ctx, &mut app, canvas.min + vec2(40.0, 40.0), canvas.min + vec2(200.0, 120.0));
        // Take the bottom-right corner from four points off it.
        drag(&ctx, &mut app, canvas.min + vec2(204.0, 123.0), canvas.min + vec2(284.0, 203.0));
        assert_eq!(boxes(&app), [(NodeKind::Rectangle, Rect::new(10.0, 10.0, 70.0, 50.0))]);
        assert_eq!(app.history.undo_name(), Some("Resize"));

        frame(&ctx, &mut app, key(Key::ArrowRight, Modifiers::NONE));
        frame(&ctx, &mut app, key(Key::ArrowUp, Modifiers::SHIFT));
        assert_eq!(boxes(&app), [(NodeKind::Rectangle, Rect::new(11.0, 0.0, 71.0, 40.0))]);
    }

    #[test]
    fn shift_1_and_shift_2_fit_the_page_and_the_selection() {
        let (ctx, mut app, canvas) = app();
        let on_screen = |app: &App, x: f64, y: f64| app.canvas.view.origin + Vec2::new(x, y) * app.canvas.view.zoom;
        let middle = Vec2::new(f64::from(canvas.width()), f64::from(canvas.height())) / 2.0;
        // Nothing on the page: nothing to fit.
        frame(&ctx, &mut app, key(Key::Num1, Modifiers::SHIFT));
        assert_eq!(app.canvas.view, crate::canvas::View::default());

        for (from, to) in [((10.0, 10.0), (50.0, 30.0)), ((300.0, 200.0), (340.0, 260.0))] {
            frame(&ctx, &mut app, key(Key::R, Modifiers::NONE));
            drag(&ctx, &mut app, canvas.min + vec2(from.0, from.1), canvas.min + vec2(to.0, to.1));
        }
        // Both rectangles: 10..340 by 10..260, so its middle is (175, 135).
        frame(&ctx, &mut app, key(Key::Num1, Modifiers::SHIFT));
        assert!((on_screen(&app, 175.0, 135.0) - middle).hypot() < 1e-6);
        let fit = app.canvas.view.zoom;
        // The selection is the second one alone: closer in, on its middle.
        frame(&ctx, &mut app, key(Key::Num2, Modifiers::SHIFT));
        assert!(app.canvas.view.zoom > fit * 2.0);
        assert!((on_screen(&app, 320.0, 230.0) - middle).hypot() < 1e-6);
    }

    #[test]
    fn the_selection_is_grouped_copied_restacked_and_ungrouped_by_key() {
        let (ctx, mut app, canvas) = app();
        for (from, to) in [((10.0, 10.0), (50.0, 30.0)), ((300.0, 200.0), (340.0, 260.0))] {
            frame(&ctx, &mut app, key(Key::R, Modifiers::NONE));
            drag(&ctx, &mut app, canvas.min + vec2(from.0, from.1), canvas.min + vec2(to.0, to.1));
        }
        let kinds = |app: &App| boxes(app).into_iter().map(|(kind, _)| kind).collect::<Vec<_>>();
        let [first, second] = app.history.document().node(app.page).unwrap().children.iter().map(|node| node.id).collect::<Vec<_>>()[..] else { panic!() };

        frame(&ctx, &mut app, key(Key::A, Modifiers::COMMAND));
        assert_eq!(app.tools.selection, [first, second]);
        frame(&ctx, &mut app, key(Key::G, Modifiers::COMMAND));
        assert_eq!(boxes(&app), [(NodeKind::Group, Rect::new(10.0, 10.0, 340.0, 260.0))]);
        assert_eq!(app.history.undo_name(), Some("Group"));
        let group = app.tools.selection[0];
        // Enter goes into the group, and Shift+Enter back out to it.
        frame(&ctx, &mut app, key(Key::Enter, Modifiers::NONE));
        assert_eq!(app.tools.selection, [first, second]);
        frame(&ctx, &mut app, key(Key::Enter, Modifiers::NONE));
        assert_eq!(app.tools.selection, [first, second], "nothing further in");
        frame(&ctx, &mut app, key(Key::Enter, Modifiers::SHIFT));
        assert_eq!(app.tools.selection, [group]);
        frame(&ctx, &mut app, key(Key::Enter, Modifiers::SHIFT));
        assert_eq!(app.tools.selection, [group], "nothing further out");

        // A duplicate goes in front and is what is selected; [ sends it back.
        frame(&ctx, &mut app, key(Key::D, Modifiers::COMMAND));
        let copy = app.tools.selection[0];
        let order = |app: &App| app.history.document().node(app.page).unwrap().children.iter().map(|node| node.id).collect::<Vec<_>>();
        assert_eq!(order(&app), [group, copy]);
        frame(&ctx, &mut app, key(Key::OpenBracket, Modifiers::NONE));
        assert_eq!(order(&app), [copy, group]);
        assert_eq!(app.history.undo_name(), Some("Send to Back"));

        frame(&ctx, &mut app, key(Key::G, Modifiers::COMMAND | Modifiers::SHIFT));
        assert_eq!(kinds(&app), [NodeKind::Rectangle, NodeKind::Rectangle, NodeKind::Group]);
        assert_eq!(app.tools.selection.len(), 2);

        // Cut takes them away; paste puts them back, on top, where they were.
        frame(&ctx, &mut app, vec![Event::Cut]);
        assert_eq!(kinds(&app), [NodeKind::Group]);
        let v_up = Event::Key { key: Key::V, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::COMMAND };
        frame(&ctx, &mut app, vec![v_up.clone()]);
        assert_eq!(boxes(&app)[1..], [(NodeKind::Rectangle, Rect::new(10.0, 10.0, 50.0, 30.0)), (NodeKind::Rectangle, Rect::new(300.0, 200.0, 340.0, 260.0))]);
        assert_eq!(app.history.undo_name(), Some("Paste"));
        // With a group selected, the paste goes into it.
        frame(&ctx, &mut app, vec![Event::Copy]);
        app.tools.selection = vec![group];
        frame(&ctx, &mut app, vec![v_up.clone()]);
        assert_eq!(app.history.document().node(group).unwrap().children.len(), 4);

        // With nothing selected none of them does anything.
        app.tools.selection.clear();
        let before = app.history.revision();
        for (letter, modifiers) in [(Key::G, Modifiers::COMMAND), (Key::D, Modifiers::COMMAND), (Key::CloseBracket, Modifiers::NONE), (Key::G, Modifiers::COMMAND | Modifiers::ALT)] {
            frame(&ctx, &mut app, key(letter, modifiers));
        }
        assert_eq!((app.history.revision(), &app.status), (before, &None));
    }

    #[test]
    fn a_double_click_goes_into_a_group_and_a_drag_from_nothing_is_a_marquee() {
        let (ctx, mut app, canvas) = app();
        for (from, to) in [((10.0, 10.0), (50.0, 30.0)), ((300.0, 200.0), (340.0, 260.0))] {
            frame(&ctx, &mut app, key(Key::R, Modifiers::NONE));
            drag(&ctx, &mut app, canvas.min + vec2(from.0, from.1), canvas.min + vec2(to.0, to.1));
        }
        let [first, second] = app.history.document().node(app.page).unwrap().children.iter().map(|node| node.id).collect::<Vec<_>>()[..] else { panic!() };
        // A marquee from the bare canvas over both.
        drag(&ctx, &mut app, canvas.min + vec2(400.0, 300.0), canvas.min + vec2(5.0, 5.0));
        assert_eq!(app.tools.selection, [first, second]);
        assert_eq!(app.history.undo_name(), Some("Draw"), "selecting isn't an edit");
        frame(&ctx, &mut app, key(Key::G, Modifiers::COMMAND));
        let group = app.tools.selection[0];
        // Ctrl is let go: with it held, a click goes straight to the deepest.
        frame(&ctx, &mut app, vec![Event::ModifiersChanged(Modifiers::NONE)]);

        // Two clicks in quick succession on the first rectangle.
        let at = canvas.min + vec2(30.0, 20.0);
        let button = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        frame(&ctx, &mut app, vec![Event::PointerMoved(at)]);
        frame(&ctx, &mut app, vec![button(true)]);
        frame(&ctx, &mut app, vec![button(false)]);
        assert_eq!(app.tools.selection, [group]);
        frame(&ctx, &mut app, vec![button(true)]);
        frame(&ctx, &mut app, vec![button(false)]);
        assert_eq!(app.tools.selection, [first]);
    }

    #[test]
    fn every_tool_draws_its_shape_and_the_panels_show_it() {
        let (ctx, mut app, canvas) = app();
        let mut kinds = Vec::new();
        for (index, (tool, command)) in TOOLS.into_iter().enumerate().skip(2) {
            app.run(command, &ctx);
            assert_eq!(app.tools.tool, tool);
            let from = canvas.min + vec2(20.0 + 45.0 * index as f32, 40.0 + 40.0 * (index % 2) as f32);
            drag(&ctx, &mut app, from, from + vec2(40.0, 30.0));
            // A frame with the new node selected: the Design panel has its fields.
            frame(&ctx, &mut app, vec![]);
            let [id] = app.tools.selection[..] else { panic!("{tool:?} drew nothing") };
            kinds.push(app.history.document().node(id).unwrap().kind.clone());
            assert_eq!((app.tools.tool, &app.status), (Tool::Move, &None));
        }
        let expected = [NodeKind::Frame { clip: true }, NodeKind::Rectangle, NodeKind::Ellipse, NodeKind::Polygon { sides: 3 }, NodeKind::Star { points: 5, ratio: 0.382 }, NodeKind::Line, NodeKind::Line];
        assert_eq!(kinds, expected);
        // L and Shift+L are the line and the arrow.
        frame(&ctx, &mut app, key(Key::L, Modifiers::NONE));
        assert_eq!(app.tools.tool, Tool::Line);
        frame(&ctx, &mut app, key(Key::L, Modifiers::SHIFT));
        assert_eq!(app.tools.tool, Tool::Arrow);
    }

    #[test]
    fn export_writes_the_selection_or_every_frame_as_each_is_set_to() {
        let folder = std::env::temp_dir().join(format!("omavec-export-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let (ctx, mut app, canvas) = app();
        // Nothing to export yet: say so, and open no dialog.
        frame(&ctx, &mut app, key(Key::E, Modifiers::COMMAND | Modifiers::SHIFT));
        assert!(app.dialog.is_none() && app.status.as_ref().is_some_and(|(message, wrong)| *wrong && message.starts_with("Nothing to export")));

        frame(&ctx, &mut app, key(Key::F, Modifiers::NONE));
        drag(&ctx, &mut app, canvas.min + vec2(20.0, 20.0), canvas.min + vec2(220.0, 120.0));
        frame(&ctx, &mut app, key(Key::O, Modifiers::NONE));
        drag(&ctx, &mut app, canvas.min + vec2(40.0, 40.0), canvas.min + vec2(100.0, 80.0));
        let ellipse = app.tools.selection[0];
        let names = |files: &Path| {
            let mut names: Vec<String> = std::fs::read_dir(files).unwrap().map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned()).collect();
            names.sort();
            names
        };
        // The selection, as a PNG since it says nothing else.
        app.export_to(&folder.join("one"));
        assert_eq!(names(&folder.join("one")), ["Ellipse.png"]);
        assert_eq!(app.status, Some((format!("Exported 1 file into {}", folder.join("one").display()), false)));
        // As its own settings say, once it has some.
        app.history.edit("Export Settings", |document| document.node_mut(ellipse).map(|node| node.exports = vec![Export::Svg, Export::Png { scale: 2.0 }])).unwrap();
        app.export_to(&folder.join("two"));
        assert_eq!(names(&folder.join("two")), ["Ellipse.svg", "Ellipse@2x.png"]);
        let svg = std::fs::read_to_string(folder.join("two/Ellipse.svg")).unwrap();
        assert!(svg.contains(r#"width="60" height="40""#) && svg.contains("<ellipse"), "{svg}");
        // With nothing selected, every frame on the page.
        app.tools.selection.clear();
        app.export_to(&folder.join("all"));
        assert_eq!(names(&folder.join("all")), ["Frame.png"]);
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn letters_pick_tools_and_escape_goes_back_to_move() {
        let (ctx, mut app, _) = app();
        for (letter, tool) in [(Key::F, Tool::Frame), (Key::R, Tool::Rectangle), (Key::O, Tool::Ellipse), (Key::H, Tool::Hand), (Key::V, Tool::Move)] {
            frame(&ctx, &mut app, key(letter, Modifiers::NONE));
            assert_eq!(app.tools.tool, tool);
        }
        // Shift+R is the rulers, not the Rectangle tool.
        frame(&ctx, &mut app, key(Key::R, Modifiers::SHIFT));
        assert_eq!((app.tools.tool, app.canvas.rulers), (Tool::Move, true));
        frame(&ctx, &mut app, key(Key::R, Modifiers::NONE));
        assert_eq!((app.tools.tool, app.canvas.rulers), (Tool::Rectangle, true));
        frame(&ctx, &mut app, key(Key::Escape, Modifiers::NONE));
        assert_eq!(app.tools.tool, Tool::Move);
    }

    #[test]
    fn a_document_saves_and_opens_again() {
        let folder = std::env::temp_dir().join(format!("omavec-app-test-{}", std::process::id())).join("Logo.omavec");
        let _ = std::fs::remove_dir_all(&folder);
        let (ctx, mut app, canvas) = app();
        frame(&ctx, &mut app, key(Key::R, Modifiers::NONE));
        drag(&ctx, &mut app, canvas.min + vec2(50.0, 60.0), canvas.min + vec2(250.0, 160.0));
        app.update_title(&ctx);
        assert_eq!(app.title, "● Untitled — Omavec");

        app.save_to(&folder);
        app.update_title(&ctx);
        assert_eq!(app.title, "Logo — Omavec");
        assert_eq!(app.status, Some(("Saved Logo".into(), false)));

        let (_, mut other, _) = self::app();
        other.open(&folder);
        assert_eq!(other.history.document(), app.history.document());
        assert_eq!((other.path.as_deref(), other.history.is_dirty(), &other.status), (Some(folder.as_path()), false, &None));
        // By the document.json inside it, as the file dialog picks it.
        let (_, mut by_file, _) = self::app();
        by_file.open(&folder.join("document.json"));
        assert_eq!((by_file.history.document(), by_file.path.as_deref()), (app.history.document(), Some(folder.as_path())));
        // And as one zipped file.
        let zipped = folder.with_extension("omavecz");
        app.save_to(&zipped);
        by_file.open(&zipped);
        by_file.update_title(&ctx);
        assert_eq!((by_file.history.document(), by_file.title.as_str()), (app.history.document(), "Logo — Omavec"));
        // Opening something that isn't a document says so and changes nothing.
        other.open(&folder.join("pages"));
        assert!(other.status.as_ref().is_some_and(|(message, wrong)| *wrong && message.starts_with("Couldn't open: ")));
        assert_eq!(other.history.document(), app.history.document());
        let _ = std::fs::remove_dir_all(folder.parent().unwrap());
    }

    #[test]
    fn unsaved_changes_are_asked_about_before_they_are_thrown_away() {
        let folder = std::env::temp_dir().join(format!("omavec-confirm-test-{}", std::process::id())).join("Kept.omavec");
        let _ = std::fs::remove_dir_all(&folder);
        let (ctx, mut app, canvas) = app();
        // Nothing to lose: New just happens.
        frame(&ctx, &mut app, key(Key::N, Modifiers::COMMAND));
        assert_eq!(app.confirm, None);

        let draw = |app: &mut App| {
            frame(&ctx, app, key(Key::R, Modifiers::NONE));
            drag(&ctx, app, canvas.min + vec2(50.0, 60.0), canvas.min + vec2(250.0, 160.0));
        };
        draw(&mut app);
        frame(&ctx, &mut app, key(Key::N, Modifiers::COMMAND));
        assert_eq!((app.confirm, boxes(&app).len()), (Some(Command::New), 1));
        // While it's asking, keys don't reach the document.
        frame(&ctx, &mut app, key(Key::Delete, Modifiers::NONE));
        frame(&ctx, &mut app, key(Key::Z, Modifiers::COMMAND));
        assert_eq!(boxes(&app).len(), 1);

        app.decide(Choice::Cancel, &ctx);
        assert_eq!((app.confirm, boxes(&app).len()), (None, 1));

        frame(&ctx, &mut app, key(Key::N, Modifiers::COMMAND));
        app.decide(Choice::Discard, &ctx);
        assert_eq!((app.confirm, boxes(&app).len(), app.history.is_dirty()), (None, 0, false));

        // Save, for a document that has somewhere to go: saved, then New.
        draw(&mut app);
        app.save_to(&folder);
        frame(&ctx, &mut app, key(Key::ArrowRight, Modifiers::NONE));
        frame(&ctx, &mut app, key(Key::N, Modifiers::COMMAND));
        assert_eq!(app.confirm, Some(Command::New));
        app.decide(Choice::Save, &ctx);
        eframe::App::logic(&mut app, &ctx, &mut eframe::Frame::_new_kittest());
        assert_eq!((boxes(&app).len(), &app.path, app.history.is_dirty()), (0, &None, false));
        let saved = file::open(&folder).unwrap();
        assert_eq!(saved.pages[0].children[0].transform.translation().x, 51.0);
        let _ = std::fs::remove_dir_all(folder.parent().unwrap());
    }

    #[test]
    fn ctrl_backslash_hides_and_shows_the_panels() {
        let ctx = egui::Context::default();
        let mut app = App::with_theme(Theme::default(), None);
        let screen = vec2(1000.0, 600.0);
        let with_panels = frame(&ctx, &mut app, vec![]);
        assert!(with_panels.width() < screen.x - 400.0, "panels on both sides: {with_panels:?}");
        assert!(with_panels.height() < screen.y, "menu bar on top: {with_panels:?}");

        let hidden = frame(&ctx, &mut app, key(Key::Backslash, Modifiers::COMMAND));
        assert_eq!(hidden.size(), screen);

        let shown = frame(&ctx, &mut app, key(Key::Backslash, Modifiers::COMMAND));
        assert_eq!(shown, with_panels);
    }

    #[test]
    fn zoom_shortcuts_zoom_about_the_middle_of_the_canvas() {
        let ctx = egui::Context::default();
        let mut app = App::with_theme(Theme::default(), None);
        let canvas = frame(&ctx, &mut app, vec![]);
        let middle = omavec_geom::kurbo::Vec2::new(f64::from(canvas.width()), f64::from(canvas.height())) / 2.0;
        let in_the_middle = |app: &App| (middle - app.canvas.view.origin) / app.canvas.view.zoom;
        let before = in_the_middle(&app);

        frame(&ctx, &mut app, key(Key::Equals, Modifiers::COMMAND));
        frame(&ctx, &mut app, key(Key::Equals, Modifiers::COMMAND));
        assert_eq!(app.canvas.view.zoom, 4.0);
        assert_eq!(in_the_middle(&app), before);

        frame(&ctx, &mut app, key(Key::Minus, Modifiers::COMMAND));
        assert_eq!(app.canvas.view.zoom, 2.0);

        frame(&ctx, &mut app, key(Key::Num0, Modifiers::SHIFT));
        assert_eq!(app.canvas.view.zoom, 1.0);
        assert_eq!(in_the_middle(&app), before);
    }

    #[test]
    fn shift_r_toggles_rulers() {
        let ctx = egui::Context::default();
        let mut app = App::with_theme(Theme::default(), None);
        assert!(!app.canvas.rulers);

        frame(&ctx, &mut app, key(Key::R, Modifiers::SHIFT));
        assert!(app.canvas.rulers);

        frame(&ctx, &mut app, key(Key::R, Modifiers::SHIFT));
        assert!(!app.canvas.rulers);
    }

    #[test]
    fn shift_quote_toggles_pixel_grid() {
        let ctx = egui::Context::default();
        let mut app = App::with_theme(Theme::default(), None);
        assert!(app.canvas.pixel_grid);

        frame(&ctx, &mut app, key(Key::Quote, Modifiers::SHIFT));
        assert!(!app.canvas.pixel_grid);

        frame(&ctx, &mut app, key(Key::Quote, Modifiers::SHIFT));
        assert!(app.canvas.pixel_grid);
    }

    #[test]
    fn ctrl_shift_h_and_ctrl_shift_l_toggle_visible_and_locked() {
        let (ctx, mut app, canvas) = app();
        frame(&ctx, &mut app, key(Key::R, Modifiers::NONE));
        drag(&ctx, &mut app, canvas.min + vec2(50.0, 60.0), canvas.min + vec2(250.0, 160.0));
        assert_eq!(app.tools.selection.len(), 1);
        let id = app.tools.selection[0];
        assert!(app.history.document().node(id).unwrap().visible);
        assert!(!app.history.document().node(id).unwrap().locked);

        frame(&ctx, &mut app, key(Key::H, Modifiers::COMMAND | Modifiers::SHIFT));
        assert!(!app.history.document().node(id).unwrap().visible);

        frame(&ctx, &mut app, key(Key::H, Modifiers::COMMAND | Modifiers::SHIFT));
        assert!(app.history.document().node(id).unwrap().visible);

        frame(&ctx, &mut app, key(Key::L, Modifiers::COMMAND | Modifiers::SHIFT));
        assert!(app.history.document().node(id).unwrap().locked);

        frame(&ctx, &mut app, key(Key::L, Modifiers::COMMAND | Modifiers::SHIFT));
        assert!(!app.history.document().node(id).unwrap().locked);
    }

    #[test]
    fn while_renaming_typed_letters_and_backspace_reach_the_text_and_nothing_else() {
        let (ctx, mut app, canvas) = app();
        frame(&ctx, &mut app, key(Key::R, Modifiers::NONE));
        drag(&ctx, &mut app, canvas.min + vec2(50.0, 60.0), canvas.min + vec2(250.0, 160.0));
        assert_eq!(app.tools.tool, Tool::Move);
        assert_eq!(boxes(&app).len(), 1);
        let id = app.tools.selection[0];

        let rect = ctx.read_response(crate::layers_panel::row_id(id)).unwrap().rect;
        let pos = pos2(rect.left() + 20.0, rect.center().y);
        let button = |pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        frame(&ctx, &mut app, vec![Event::PointerMoved(pos), button(true)]);
        frame(&ctx, &mut app, vec![button(false)]);
        frame(&ctx, &mut app, vec![button(true)]);
        frame(&ctx, &mut app, vec![button(false)]);
        assert!(app.layers.renaming.is_some());

        // First frame: text edit requests focus and selects all text.
        frame(&ctx, &mut app, vec![]);

        // Type 'r': should not pick Rectangle tool.
        frame(&ctx, &mut app, [vec![Event::Text("r".into())], key(Key::R, Modifiers::NONE)].concat());
        assert_eq!(app.tools.tool, Tool::Move);

        // Type 'v': should not pick Move tool.
        frame(&ctx, &mut app, [vec![Event::Text("v".into())], key(Key::V, Modifiers::NONE)].concat());
        assert_eq!(app.tools.tool, Tool::Move);

        // Type 'o': should not pick Ellipse tool.
        frame(&ctx, &mut app, [vec![Event::Text("o".into())], key(Key::O, Modifiers::NONE)].concat());
        assert_eq!(app.tools.tool, Tool::Move);

        // Backspace: should not delete the node.
        frame(&ctx, &mut app, key(Key::Backspace, Modifiers::NONE));
        assert_eq!(boxes(&app).len(), 1);

        // Press Enter to submit rename.
        frame(&ctx, &mut app, key(Key::Enter, Modifiers::NONE));
        assert_eq!(app.layers.renaming, None);
        assert_eq!(app.tools.tool, Tool::Move);
        assert_eq!(boxes(&app).len(), 1);
        assert_eq!(app.history.document().node(id).unwrap().name, "rv");
    }
}
