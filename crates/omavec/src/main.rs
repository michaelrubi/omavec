mod app;
mod canvas;
mod commands;
mod rulers;
mod theme;

use eframe::egui_wgpu::WgpuSetup;
use eframe::wgpu::PowerPreference;

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn,omavec=info"))
        .init();

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
        Box::new(move |cc| Ok(Box::new(app::App::new(cc)))),
    )
}
