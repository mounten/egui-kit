//! `/blog`: a nested router with its own layout (with a `load`) and error page.

mod error;
mod layout;
mod page;
mod slug;

use egui_kit::Router;
use egui_kit::page::{Layout, Page};

use crate::state::{AppState, POSTS};

pub fn router() -> Router<AppState> {
    Router::new()
        // Reloaded when a post is published (see the admin page); the index reloads along,
        // since it reads the layout's data with `Parent`.
        .layout(Layout::with_load(layout::load, layout::view).depends(POSTS))
        .error(error::view)
        .page("/", Page::with_load(page::load, page::view))
        .page(
            "/{slug}",
            Page::with_load(slug::page::load, slug::page::view).depends(POSTS),
        )
        .page(
            "/{slug}/print",
            Page::with_load(slug::page::load, slug::print::view).reset_layouts(),
        )
}
