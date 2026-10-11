//! [`Parent`]: the data of the layouts above a load.

use core::any::type_name;
use core::cell::{Cell, RefCell};
use core::future::poll_fn;
use core::task::Poll;
use std::rc::Rc;

use super::FromRequest;
use crate::error::{Rejection, RouteError};
use crate::load::DataCell;
use crate::request::{Deps, Request};

/// The progress of the layouts and page of a route being loaded, by level.
pub(crate) enum Slot {
    /// No layout at this level.
    Empty,
    Waiting,
    Ready(DataCell),
    Failed,
}

pub(crate) type Slots = Rc<RefCell<Vec<Slot>>>;

/// What a load needs to wait for its parents' data.
#[derive(Clone)]
pub(crate) struct ParentHandle {
    slots: Slots,
    /// The loading node's level; its parents are the levels before.
    index: usize,
    deps: Rc<Cell<Deps>>,
}

impl ParentHandle {
    pub(crate) fn new(slots: Slots, index: usize, deps: Rc<Cell<Deps>>) -> Self {
        Self { slots, index, deps }
    }
}

/// Access to the data loaded by the layouts above, in a layout's or page's `load`.
///
/// ```no_run
/// # use egui_kit::Rejection;
/// # use egui_kit::extract::Parent;
/// #[derive(Clone)]
/// struct BlogLayout { posts: Vec<String> }
///
/// async fn load(parent: Parent) -> Result<usize, Rejection> {
///     let blog = parent.get::<BlogLayout>().await?;
///     Ok(blog.posts.len())
/// }
/// ```
///
/// Using it makes the load rerun whenever a parent layout reloads.
pub struct Parent(ParentHandle);

impl<S> FromRequest<S> for Parent {
    fn from_request(req: &Request, _state: &S) -> Result<Self, Rejection> {
        req.parent
            .clone()
            .map(Parent)
            .ok_or_else(|| RouteError::internal("`Parent` is only available in a `load`").into())
    }
}

impl Parent {
    /// Waits for the layouts above and returns a copy of the innermost one's data of type `T`.
    ///
    /// # Errors
    ///
    /// Fails with a 500 if no parent layout loaded a `T`, or if a parent layout failed.
    pub async fn get<T: Clone + 'static>(&self) -> Result<T, Rejection> {
        let handle = &self.0;
        let mut deps = handle.deps.get();
        deps.parent = true;
        handle.deps.set(deps);

        poll_fn(|_| {
            let slots = handle.slots.borrow();
            let parents = &slots[..handle.index];
            if parents.iter().any(|slot| matches!(slot, Slot::Waiting)) {
                // Polled again by the loader once a parent finished.
                return Poll::Pending;
            }
            if parents.iter().any(|slot| matches!(slot, Slot::Failed)) {
                return Poll::Ready(Err(
                    RouteError::internal("a parent layout failed to load").into()
                ));
            }
            let data = parents.iter().rev().find_map(|slot| match slot {
                Slot::Ready(data) => data.borrow().downcast_ref::<T>().cloned(),
                _ => None,
            });
            Poll::Ready(data.ok_or_else(|| {
                RouteError::internal(format!("no parent layout loaded a `{}`", type_name::<T>()))
                    .into()
            }))
        })
        .await
    }
}
