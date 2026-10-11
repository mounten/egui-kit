//! The route tree, laid out like a SvelteKit `src/routes` directory:
//!
//! ```text
//! routes/
//! ├── layout.rs             +layout.svelte         top bar
//! ├── error.rs              +error.svelte
//! ├── page.rs               +page.svelte           /
//! ├── about/page.rs         [[lang=lang]]/about    optional parameter with a matcher
//! ├── login/page.rs         login                  /login?redirect=…
//! ├── blog/
//! │   ├── layout.rs         +layout.ts/.svelte     sidebar, loaded; depends on "app:posts"
//! │   ├── error.rs          +error.svelte
//! │   ├── page.rs           +page.ts/.svelte       /blog?q=…&tag=…, reads the layout's data
//! │   └── slug/
//! │       ├── page.rs       [slug]/+page.ts/.svelte
//! │       └── print.rs      [slug]/print/+page@    breaks out of the blog layout
//! ├── authors/
//! │   ├── id/page.rs        [id=int]               /authors/1
//! │   └── name/page.rs      [name]                 /authors/ferris
//! ├── gallery/              shallow routing, scroll restoration
//! └── admin/                guarded by a hook; blocks navigation with unsaved drafts;
//!                           publishes with a command (see `remote.rs`)
//! ```
//!
//! Every `page.rs` has a `load` (the `+page.ts`) and a `view` (the `+page.svelte`).

mod about;
mod admin;
mod authors;
mod blog;
mod error;
mod gallery;
mod layout;
mod login;
mod page;

use egui_kit::Router;
use egui_kit::page::{Layout, Page};

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        // SvelteKit's `src/params/*.ts`.
        .matcher("int", |segment| segment.parse::<u32>().is_ok())
        .matcher("lang", |segment| about::page::LANGUAGES.contains(&segment))
        .layout(Layout::new(layout::view))
        .error(error::view)
        .page("/", Page::new(page::view))
        .page(
            "/{?lang=lang}/about",
            Page::with_load(about::page::load, about::page::view),
        )
        .page(
            "/login",
            Page::with_load(login::page::load, login::page::view),
        )
        .page(
            "/authors/{id=int}",
            Page::with_load(authors::id::page::load, authors::id::page::view),
        )
        .page(
            "/authors/{name}",
            Page::with_load(authors::name::page::load, authors::name::page::view),
        )
        .nest("/blog", blog::router())
        .nest("/gallery", gallery::router())
        .nest("/admin", admin::router())
}
