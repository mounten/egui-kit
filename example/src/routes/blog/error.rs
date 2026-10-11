//! Blog error page: rendered inside the blog layout, so the sidebar stays visible.

use egui::Color32;
use egui_kit::RouteError;
use egui_kit::extract::Nav;

pub fn view(ui: &mut egui::Ui, error: &RouteError, nav: Nav<'_>) {
    ui.heading("Blog error");
    ui.colored_label(
        Color32::LIGHT_RED,
        format!("{} — {}", error.status, error.message),
    );
    ui.add_space(8.0);
    nav.link(ui, "All posts", "/blog");
}
