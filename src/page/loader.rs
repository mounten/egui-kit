//! What pages and layouts have in common: a `load` and what it depends on.

use core::sync::atomic::{AtomicU64, Ordering};
use std::rc::Rc;

use egui::Ui;

use super::handler::ErasedHandler;
use super::view::{Children, ViewContext};

/// A page or error view; its data is in the [`ViewContext`].
pub(crate) type ErasedView<S> = Rc<dyn Fn(&mut Ui, &ViewContext<'_, S>)>;
pub(crate) type ErasedLayoutView<S> = Rc<dyn Fn(&mut Ui, Children<'_>, &ViewContext<'_, S>)>;

/// Identifies a page or layout, to recognise it across navigations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct NodeId(u64);

impl NodeId {
    fn next() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

/// What pages and layouts have in common: a `load` and what it depends on.
pub(crate) struct Loader<S> {
    pub(crate) id: NodeId,
    pub(crate) load: ErasedHandler<S>,
    /// Rerun the load when one of these is invalidated, see [`Nav::invalidate`](crate::extract::Nav::invalidate).
    pub(crate) keys: Vec<String>,
}

impl<S: 'static> Loader<S> {
    pub(crate) fn new(load: ErasedHandler<S>) -> Self {
        Self {
            id: NodeId::next(),
            load,
            keys: Vec::new(),
        }
    }
}

/// Which parent layouts a page or layout keeps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LayoutReset {
    /// Only the root layout.
    Root,
    /// The layouts up to the router [named](crate::Router::named) so.
    To(String),
}
