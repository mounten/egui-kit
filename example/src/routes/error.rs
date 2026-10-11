//! Root error page, rendered inside the root layout.

use egui::RichText;
use egui_kit::RouteError;
use egui_kit::extract::Nav;

/// Error pages extract their error like pages extract their data.
pub fn view(ui: &mut egui::Ui, error: &RouteError, nav: Nav<'_>) {
    ui.vertical_centered(|ui| {
        ui.add_space(48.0);
        ui.label(RichText::new(error.status.to_string()).size(64.0).strong());
        ui.label(&error.message);
        ui.add_space(16.0);
        ui.horizontal(|ui| {
            if ui.button("Back").clicked() {
                nav.back();
            }
            nav.link(ui, "Home", "/");
        });
    });
}
