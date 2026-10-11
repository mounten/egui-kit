//! `/gallery`: shallow routing, as in SvelteKit's docs. Clicking a photo opens it in a modal and
//! pushes `/gallery/{id}` *without* navigating; back closes the modal again. Loading
//! `/gallery/{id}` for real (a link, a reload, back from another page) shows the photo's own page.
//!
//! The grid scrolls in the page's `Scroll` area: scroll down, open a photo's page, go back, and the
//! grid is where you left it.

mod id;
mod page;

use egui::{Response, Sense, Ui, Vec2};
use egui_kit::Router;
use egui_kit::page::Page;

use crate::state::{AppState, Photo};

pub fn router() -> Router<AppState> {
    Router::new()
        .page("/", Page::with_load(page::load, page::view))
        .page("/{id}", Page::with_load(id::page::load, id::page::view))
}

fn swatch(ui: &mut Ui, photo: &Photo, size: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let rounding = if response.hovered() { 4.0 } else { 12.0 };
    ui.painter().rect_filled(rect, rounding, photo.color);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
