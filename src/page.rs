//! Pages and layouts: what a route loads and shows.
//!
//! A [`Page`] is made of two functions:
//!
//! - an optional **load**: an `async fn` whose arguments are [extractors](crate::extract) and
//!   which returns `Result<D, E>` with `E: Into<Rejection>`. Any such function implements
//!   [`Handler`]. Hooks are handlers too, returning `Result<(), E>`.
//! - a **view**: a function of `&mut egui::Ui` and extractors, called every frame. Any such
//!   function implements [`View`]. Among the extractors, `&mut D` (or `&D`) is what the load
//!   returned; that the types match is checked at compile time.
//!
//! ```
//! use egui::Ui;
//! use egui_kit::Rejection;
//! use egui_kit::extract::{Nav, Path};
//! use egui_kit::page::Page;
//!
//! struct Post {
//!     title: String,
//! }
//!
//! async fn load(Path(id): Path<u32>) -> Result<Post, Rejection> {
//!     Ok(Post { title: format!("Post #{id}") })
//! }
//!
//! fn view(ui: &mut Ui, post: &mut Post, nav: Nav) {
//!     ui.heading(&post.title);
//!     nav.link(ui, "Home", "/");
//! }
//!
//! let page = Page::<()>::with_load(load, view);
//! ```
//!
//! A [`Layout`] wraps all pages of a router. Its view
//! ([`LayoutView`]) gets the [`Outlet`] to draw the page in as its second argument.
//!
//! Views can be closures too, with annotated arguments: `|ui: &mut Ui, nav: Nav| …`.

mod handler;
mod layout;
mod loader;
mod view;

use std::rc::Rc;

use crate::error::Rejection;
pub use handler::Handler;
pub(crate) use handler::{ErasedHandler, LocalBoxFuture, erase, unit};
pub use layout::Layout;
pub(crate) use loader::{ErasedView, LayoutReset, Loader, NodeId};
pub(crate) use view::Frame;
pub use view::{LayoutView, Outlet, View, ViewContext};

/// A page: an optional `load` handler and a [view](View).
///
/// The view extracts the data returned by `load` as `&mut D`, so it can also use it to keep
/// page-local UI state (text inputs, toggles, …) for as long as the page is shown.
pub struct Page<S> {
    pub(crate) loader: Loader<S>,
    pub(crate) view: ErasedView<S>,
    pub(crate) reset: Option<LayoutReset>,
}

impl<S: 'static> Page<S> {
    /// A page without a `load` handler.
    pub fn new<V, U>(view: V) -> Self
    where
        V: View<U, S, ()>,
    {
        Self {
            loader: Loader::new(unit()),
            view: Rc::new(move |ui, cx| view.call(ui, cx)),
            reset: None,
        }
    }

    /// A page whose view can extract the data produced by `load` (as `&mut D` or `&D`).
    pub fn with_load<H, T, D, E, V, U>(load: H, view: V) -> Self
    where
        H: Handler<T, S, Output = Result<D, E>>,
        D: 'static,
        E: Into<Rejection>,
        V: View<U, S, D>,
    {
        Self {
            loader: Loader::new(erase(load)),
            view: Rc::new(move |ui, cx| view.call(ui, cx)),
            reset: None,
        }
    }

    /// Reruns `load` when `key` is [invalidated](crate::extract::Nav::invalidate).
    pub fn depends(mut self, key: impl Into<String>) -> Self {
        self.loader.keys.push(key.into());
        self
    }

    /// Renders the page in the root layout only, skipping the layouts in between.
    /// Hooks and error pages still apply.
    pub fn reset_layouts(mut self) -> Self {
        self.reset = Some(LayoutReset::Root);
        self
    }

    /// Renders the page in the layouts up to the router [named](crate::Router::named) `name`.
    pub fn reset_layouts_to(mut self, name: impl Into<String>) -> Self {
        self.reset = Some(LayoutReset::To(name.into()));
        self
    }
}
