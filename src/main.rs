mod app;
mod camera;
mod config;
mod hymns;
mod overlay;
mod visca;

use app::PTZApp;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([540.0, 820.0])
            .with_min_inner_size([480.0, 720.0])
            .with_title("ViscaCaster — Universal PTZ & Broadcast Director"),
        ..Default::default()
    };

    eframe::run_native(
        "ViscaCaster",
        options,
        Box::new(|cc| Ok(Box::new(PTZApp::new(cc)))),
    )
}
