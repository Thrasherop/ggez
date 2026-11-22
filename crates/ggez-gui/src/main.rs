#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("ggez")
            .with_inner_size([980.0, 680.0]),
        ..Default::default()
    };
    eframe::run_simple_native("ggez", options, |ctx, _frame| {
        eframe::egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("ggez");
        });
    })
}
