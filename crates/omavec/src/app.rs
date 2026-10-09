use std::sync::mpsc::Receiver;

use egui::{Button, Ui};

use crate::canvas::Canvas;
use crate::commands::Command;
use crate::theme::{self, Theme};

pub struct App {
    theme: Theme,
    /// New themes from the watcher; `None` in tests.
    theme_rx: Option<Receiver<Theme>>,
    /// Whether the panels around the canvas are shown (Ctrl+\).
    show_ui: bool,
    canvas: Canvas,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
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
        app
    }

    fn with_theme(theme: Theme, blobs: Option<usize>) -> Self {
        let canvas = match blobs {
            Some(count) => {
                let middle = omavec_render::spike::DOCUMENT / 2.0;
                Canvas::new(omavec_render::spike::blobs(count), Some((middle, middle).into()))
            }
            None => Canvas::new(Default::default(), None),
        };
        Self { theme, theme_rx: None, show_ui: true, canvas }
    }

    fn run(&mut self, command: Command, ctx: &egui::Context) {
        match command {
            Command::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Command::ToggleUi => self.show_ui = !self.show_ui,
            Command::ZoomIn => self.canvas.zoom_by(2.0),
            Command::ZoomOut => self.canvas.zoom_by(0.5),
            Command::ZoomTo100 => self.canvas.zoom_by(1.0 / self.canvas.view.zoom),
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
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| self.menu_item(ui, Command::Quit));
            ui.menu_button("View", |ui| {
                for command in [Command::ZoomIn, Command::ZoomOut, Command::ZoomTo100, Command::ToggleUi] {
                    self.menu_item(ui, command);
                }
            });
        });
    }

    fn keys(&mut self, ctx: &egui::Context) {
        // While a text field has focus, keys edit the text.
        if !ctx.egui_wants_keyboard_input() {
            for command in Command::pressed(ctx) {
                self.run(command, ctx);
            }
        }
    }

    /// Lays out the window and returns the rectangle the canvas got.
    fn show(&mut self, ui: &mut Ui) -> egui::Rect {
        let bar = egui::Frame::new()
            .fill(self.theme.dark_background)
            .inner_margin(egui::Margin::symmetric(8, 4));
        if self.show_ui {
            egui::Panel::top("menu").frame(bar).show(ui, |ui| self.menu_bar(ui));
            egui::Panel::left("layers")
                .frame(bar)
                .default_size(240.0)
                .resizable(true)
                .show(ui, |ui| {
                    ui.strong("Layers");
                    ui.separator();
                    ui.take_available_space();
                });
            egui::Panel::right("properties")
                .frame(bar)
                .default_size(240.0)
                .resizable(true)
                .show(ui, |ui| {
                    ui.strong("Design");
                    ui.separator();
                    ui.take_available_space();
                });
        }
        egui::CentralPanel::no_frame().show(ui, |ui| self.canvas.show(ui, self.theme.backdrop())).inner
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(theme) = self.theme_rx.as_ref().and_then(|rx| rx.try_iter().last()) {
            ctx.set_visuals(theme.visuals());
            self.theme = theme;
        }
        self.keys(ctx);
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, Key, Modifiers, pos2, vec2};

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
}
