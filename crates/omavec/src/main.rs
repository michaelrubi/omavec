// Shipped code returns errors; only tests may panic.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod app;
mod canvas;
mod cli;
mod commands;
mod layers_panel;
mod properties;
mod rulers;
mod theme;
mod tools;

use eframe::egui_wgpu::WgpuSetup;
use eframe::wgpu::PowerPreference;

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn,omavec=info"))
        .init();

    // `omavec export …` writes files and never opens a window.
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments.first().is_some_and(|first| first == "export") {
        match cli::export(&arguments[1..]) {
            Ok(written) => written.iter().for_each(|path| println!("{}", path.display())),
            Err(error) => {
                eprintln!("omavec export: {error}");
                std::process::exit(2);
            }
        }
        return Ok(());
    }
    // `omavec logo.omavec` opens that document.
    let open = std::env::args_os().nth(1).map(std::path::PathBuf::from);

    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id("omavec")
            .with_title("Omavec")
            .with_inner_size([1400.0, 900.0])
            .with_min_inner_size([480.0, 320.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    // Prefer the integrated GPU so opening Omavec doesn't wake a laptop's
    // discrete GPU and drain the battery.
    if let WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup {
        setup.power_preference = PowerPreference::LowPower;
    }

    eframe::run_native(
        "omavec",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, open)))),
    )
}
