//! [`Layout`]: what wraps the pages of a router.

use core::cell::Cell;
use std::rc::Rc;

use super::handler::{Handler, erase, unit};
use super::loader::{ErasedLayoutView, LayoutReset, Loader};
use super::view::LayoutView;
use crate::error::Rejection;

/// A layout wrapping every page of a [`Router`](crate::Router),
/// including those of nested routers.
///
/// Its [view](LayoutView) gets the [`Children`](super::Children) after the `Ui`, then extractors. Render the child
/// with [`Children::show`](super::Children::show), or animated with a [`ViewTransition`](crate::ViewTransition).
pub struct Layout<S> {
    pub(crate) loader: Loader<S>,
    pub(crate) view: ErasedLayoutView<S>,
    pub(crate) reset: Option<LayoutReset>,
    /// Set once the layout rendered its children with a `ViewTransition`.
    pub(crate) transitions: Cell<bool>,
    /// Whether the view takes its data as `&mut D`, which keeps the views below from reading it.
    pub(crate) data_mut: bool,
}

impl<S: 'static> Layout<S> {
    /// A layout without a `load` handler.
    pub fn new<V, U>(view: V) -> Self
    where
        V: LayoutView<U, S, ()>,
    {
        Self {
            loader: Loader::new(unit()),
            data_mut: view.borrows_data_mut(),
            view: Rc::new(move |ui, children, cx| view.call(ui, children, cx)),
            reset: None,
            transitions: Cell::new(false),
        }
    }

    /// A layout whose view can extract the data produced by `load` (as `&mut D` or `&D`).
    /// The loads below can read it with [`Parent`](crate::extract::Parent), and the views below
    /// with [`LayoutData`](crate::extract::LayoutData) unless this view takes it as `&mut D`.
    pub fn with_load<H, T, D, E, V, U>(load: H, view: V) -> Self
    where
        H: Handler<T, S, Output = Result<D, E>>,
        D: 'static,
        E: Into<Rejection>,
        V: LayoutView<U, S, D>,
    {
        Self {
            loader: Loader::new(erase(load)),
            data_mut: view.borrows_data_mut(),
            view: Rc::new(move |ui, children, cx| view.call(ui, children, cx)),
            reset: None,
            transitions: Cell::new(false),
        }
    }

    /// See [`Page::depends`](super::Page::depends).
    pub fn depends(mut self, key: impl Into<String>) -> Self {
        self.loader.keys.push(key.into());
        self
    }

    /// Wraps this layout in the root layout only, skipping the layouts in between.
    pub fn reset_layouts(mut self) -> Self {
        self.reset = Some(LayoutReset::Root);
        self
    }

    /// Wraps this layout in the layouts up to the router [named](crate::Router::named) `name`.
    pub fn reset_layouts_to(mut self, name: impl Into<String>) -> Self {
        self.reset = Some(LayoutReset::To(name.into()));
        self
    }
}
