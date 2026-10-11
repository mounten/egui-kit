//! Hooks run before any `load` of the routes they are attached to (like SvelteKit's `handle`).

use egui_kit::extract::State;
use egui_kit::{Rejection, Request, redirect};

use crate::state::Session;

/// Attached to the `/admin` router: sends anonymous users to the login page.
pub async fn require_login(State(session): State<Session>, req: Request) -> Result<(), Rejection> {
    if session.user().is_some() {
        return Ok(());
    }
    let query = serde_urlencoded::to_string([("redirect", req.url())]).unwrap_or_default();
    Err(redirect(format!("/login?{query}")))
}
