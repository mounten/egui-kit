//! `/admin`: every route in here is guarded by the `require_login` hook.

mod page;

use egui_kit::Router;
use egui_kit::page::Page;

use crate::hooks;
use crate::state::{AppState, POSTS};

pub fn router() -> Router<AppState> {
    Router::new()
        .hook(hooks::require_login)
        .page("/", Page::with_load(page::load, page::view).depends(POSTS))
}
