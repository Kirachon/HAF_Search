mod database;
mod gpu;
mod gui;
mod match_engine;
mod matcher;
mod opener;
mod reference_loader;
mod scanner;
mod searcher;
mod vectorizer;

use eframe::NativeOptions;
use gui::TiffLocatorApp;

fn main() -> Result<(), eframe::Error> {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .try_init();

    let options = NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1000.0, 700.0])
            .with_min_inner_size([800.0, 600.0])
            .with_icon(eframe::icon_data::from_png_bytes(&[]).unwrap_or_default()),
        ..Default::default()
    };

    eframe::run_native(
        "TiffLocator",
        options,
        Box::new(|cc| Ok(Box::new(TiffLocatorApp::new(cc)))),
    )
}
