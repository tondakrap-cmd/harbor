mod logo;
mod ui;
use eframe::egui;
fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // `harbor backup …` etc. run without opening a window (cron, systemd timers).
    if let Some(code) = harbor_core::cli::run(&args) {
        std::process::exit(code);
    }
    if args.iter().any(|a| a == "--help") {
        println!("harbor [--demo]\n\n{}", harbor_core::cli::USAGE);
        return Ok(());
    }
    if std::env::args().any(|a| a == "--version") {
        println!("harbor {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let demo = std::env::args().any(|a| a == "--demo");
    #[cfg(debug_assertions)]
    if let Some(dir) = std::env::var_os("HARBOR_TOUR_DIR") {
        let socket = std::env::var("HARBOR_TOUR_SOCKET").ok();
        let size = std::env::var("HARBOR_TOUR_SIZE").unwrap_or_else(|_| "1440x960".into());
        let (w, h) = size
            .split_once('x')
            .map(|(w, h)| (w.parse().unwrap_or(1440.0), h.parse().unwrap_or(960.0)))
            .unwrap_or((1440.0, 960.0));
        return eframe::run_native(
            "Harbor tour",
            eframe::NativeOptions {
                viewport: egui::ViewportBuilder::default().with_inner_size([w, h]),
                ..Default::default()
            },
            Box::new(move |cc| {
                Ok(Box::new(ui::tour::Tour::new(
                    &cc.egui_ctx,
                    dir.into(),
                    socket,
                )))
            }),
        );
    }
    eframe::run_native(
        "Harbor • správce MariaDB",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1440.0, 960.0])
                .with_min_inner_size([1024.0, 720.0])
                // X11 shows this icon; Wayland takes it from harbor.desktop by app id.
                .with_icon(logo::icon(128))
                .with_app_id("harbor"),
            ..Default::default()
        },
        Box::new(move |cc| Ok(Box::new(ui::Harbor::new(&cc.egui_ctx, demo)))),
    )
}
