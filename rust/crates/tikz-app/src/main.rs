#![forbid(unsafe_code)]
//! Desktop entry point: `tikz-app [document.tex]`.

use std::path::PathBuf;

use tikz_app::app::TikzApp;

fn main() -> eframe::Result<()> {
    let initial = std::env::args_os().nth(1).map(PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("TikZ Editor")
            .with_inner_size([1360.0, 860.0])
            .with_min_inner_size([900.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native(
        "tikz-editor",
        options,
        Box::new(move |_cc| Ok(Box::new(TikzApp::new(initial)))),
    )
}