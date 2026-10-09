use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};

use egui::{Button, RichText, Ui};
use omavec_engine::display::DisplayList;
use omavec_engine::{Document, History, Node, NodeId, file};
use omavec_geom::kurbo::{Point, Rect, Vec2};

use crate::canvas::{Canvas, Pointer};
use crate::commands::Command;
use crate::properties::Properties;
use crate::theme::{self, Theme};
use crate::tools::{Keys, Tool, Tools};

/// The tools in the tool bar, with the command that picks each.
const TOOLS: [(Tool, Command); 4] = [(Tool::Move, Command::MoveTool), (Tool::Frame, Command::FrameTool), (Tool::Rectangle, Command::RectangleTool), (Tool::Ellipse, Command::EllipseTool)];

pub struct App {
    theme: Theme,
    /// New themes from the watcher; `None` in tests.
    theme_rx: Option<Receiver<Theme>>,
    /// Whether the panels around the canvas are shown (Ctrl+\).
    show_ui: bool,
    canvas: Canvas,
    history: History,
    tools: Tools,
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
        Self { theme, theme_rx: None, show_ui: true, canvas, history: History::new(document), tools: Tools::default(), properties: Properties::default(), page, drawn: None, spike: blobs.is_some(), path: None, dialog: None, status: None, title: String::new() }
    }

    fn say(&mut self, message: impl Into<String>, wrong: bool) {
        let message = message.into();
        if wrong {
            log::warn!("{message}");
        }
        self.status = Some((message, wrong));
    }

    /// Whatever the tools and the engine refuse is said, not fatal.
    fn check<T>(&mut self, result: Result<T, omavec_engine::Error>) {
        if let Err(error) = result {
            self.say(error.to_string(), true);
        }
    }

    /// Puts `document` on the canvas in place of the one that was there.
    fn set_document(&mut self, document: Document, path: Option<PathBuf>) {
        let Some(page) = document.pages.first().map(|page| page.id) else { return };
        (self.history, self.tools, self.page, self.path, self.drawn) = (History::new(document), Tools::default(), page, path, None);
    }

    fn open(&mut self, folder: &Path) {
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
            Err(error) => self.say(format!("Couldn't save: {error}"), true),
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
                Command::Open => dialog.set_title("Open an .omavec folder").pick_folder(),
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
            (_, Some(mut folder)) => {
                if folder.extension().is_none_or(|extension| extension != "omavec") {
                    folder.as_mut_os_string().push(".omavec");
                }
                self.save_to(&folder);
            }
            (_, None) => {}
        }
    }

    fn run(&mut self, command: Command, ctx: &egui::Context) {
        match command {
            Command::New => self.set_document(Document::default(), None),
            Command::Open | Command::SaveAs => self.ask(command, ctx),
            Command::Save => match self.path.clone() {
                Some(folder) => self.save_to(&folder),
                None => self.ask(Command::SaveAs, ctx),
            },
            Command::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Command::Undo => drop(self.history.undo()),
            Command::Redo => drop(self.history.redo()),
            Command::Delete => {
                let deleted = self.tools.delete(&mut self.history);
                self.check(deleted);
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
            Command::MoveTool | Command::FrameTool | Command::RectangleTool | Command::EllipseTool => {
                if let Some((tool, _)) = TOOLS.iter().find(|(_, picks)| *picks == command) {
                    self.tools.tool = *tool;
                }
            }
            Command::ZoomIn => self.canvas.zoom_by(2.0),
            Command::ZoomOut => self.canvas.zoom_by(0.5),
            Command::ZoomTo100 => self.canvas.zoom_by(1.0 / self.canvas.view.zoom),
            Command::ToggleRulers => self.canvas.rulers = !self.canvas.rulers,
            Command::TogglePixelGrid => self.canvas.pixel_grid = !self.canvas.pixel_grid,
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
        const MENUS: [(&str, &[Command]); 3] = [
            ("File", &[Command::New, Command::Open, Command::Save, Command::SaveAs, Command::Quit]),
            ("Edit", &[Command::Undo, Command::Redo, Command::Delete]),
            ("View", &[Command::ZoomIn, Command::ZoomOut, Command::ZoomTo100, Command::ToggleRulers, Command::TogglePixelGrid, Command::ToggleUi]),
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

    /// The page's nodes, front-most first as in Figma; a click selects one.
    fn layers(&mut self, ui: &mut Ui) {
        fn rows(ui: &mut Ui, nodes: &[std::sync::Arc<Node>], depth: usize, selection: &[NodeId], accent: egui::Color32, picked: &mut Option<NodeId>) {
            for node in nodes.iter().rev() {
                ui.horizontal(|ui| {
                    ui.add_space(depth as f32 * 12.0);
                    let mut name = RichText::new(&node.name);
                    if selection.contains(&node.id) {
                        name = name.color(accent).strong();
                    }
                    if ui.add(Button::new(name).frame(false)).clicked() {
                        *picked = Some(node.id);
                    }
                });
                rows(ui, &node.children, depth + 1, selection, accent, picked);
            }
        }
        let mut picked = None;
        if let Some(page) = self.history.document().node(self.page) {
            rows(ui, &page.children, 0, &self.tools.selection, self.theme.accent, &mut picked);
        }
        if let Some(picked) = picked {
            self.tools.selection = vec![picked];
        }
    }

    fn keys(&mut self, ctx: &egui::Context) {
        // While a text field has focus, keys edit the text.
        if !ctx.egui_wants_keyboard_input() {
            for command in Command::pressed(ctx) {
                self.run(command, ctx);
            }
        }
    }

    /// The corners of each selected node's box, on the page.
    fn selected(&self) -> Vec<[Point; 4]> {
        let document = self.history.document();
        let corners = |id: &NodeId| {
            let (to_page, size) = (document.to_page(*id)?, document.node(*id)?.size);
            let outline = Rect::from_origin_size((0.0, 0.0), size);
            Some([(outline.x0, outline.y0), (outline.x1, outline.y0), (outline.x1, outline.y1), (outline.x0, outline.y1)].map(|corner| to_page * Point::from(corner)))
        };
        self.tools.selection.iter().filter_map(corners).collect()
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
                    self.layers(ui);
                    ui.take_available_space();
                });
            egui::Panel::right("properties")
                .frame(bar)
                .default_size(240.0)
                .resizable(true)
                .show(ui, |ui| {
                    ui.strong("Design");
                    ui.separator();
                    let shown = self.properties.show(ui, &mut self.history, &self.tools.selection);
                    self.check(shown);
                    ui.take_available_space();
                });
        }
        let selected = self.selected();
        let (rect, pointer) = egui::CentralPanel::no_frame().show(ui, |ui| self.canvas.show(ui, &self.theme, &selected)).inner;
        let keys = ui.input(|i| Keys { shift: i.modifiers.shift, alt: i.modifiers.alt });
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
            };
            self.check(done);
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
        self.keys(ctx);
        self.update_title(ctx);
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, Key, Modifiers, PointerButton, Pos2, pos2, vec2};
    use omavec_engine::NodeKind;
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
    fn letters_pick_tools_and_escape_goes_back_to_move() {
        let (ctx, mut app, _) = app();
        for (letter, tool) in [(Key::F, Tool::Frame), (Key::R, Tool::Rectangle), (Key::O, Tool::Ellipse), (Key::V, Tool::Move)] {
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
        // Opening something that isn't a document says so and changes nothing.
        other.open(&folder.join("pages"));
        assert!(other.status.as_ref().is_some_and(|(message, wrong)| *wrong && message.starts_with("Couldn't open: ")));
        assert_eq!(other.history.document(), app.history.document());
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
}
