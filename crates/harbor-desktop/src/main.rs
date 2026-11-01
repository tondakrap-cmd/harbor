mod ui;
use eframe::egui;
fn main() -> eframe::Result {
    if std::env::args().any(|a| a == "--version") {
        println!("harbor {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let demo = std::env::args().any(|a| a == "--demo");
    eframe::run_native(
        "Harbor • správce MariaDB",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1440.0, 960.0])
                .with_min_inner_size([1024.0, 720.0]),
            ..Default::default()
        },
        Box::new(move |cc| Ok(Box::new(ui::Harbor::new(&cc.egui_ctx, demo)))),
    )
}
