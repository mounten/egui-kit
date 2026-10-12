//! Queries and commands: async data and mutations, as easy to start as `tokio::spawn`, inspired
//! by SvelteKit's [remote functions](https://svelte.dev/docs/kit/remote-functions) and `TanStack`
//! Query.
//!
//! A route's `load` fetches what its page needs before it is shown. Some data doesn't belong to
//! a route, though: a badge in a layout, an autocomplete, a count that several pages show. And
//! changing data is often async too (a `POST` to a backend), which a view can't `await`.
//!
//! - A [`Query`] is async data: it fetches on its first read, and can be
//!   [refreshed](Query::refresh), [refetched periodically](Query::refetch_every) and
//!   [cached](Query::key), so that queries with the same key share one result.
//! - A [`Command`] is an async mutation: [run](Command::run) it, and see whether it's pending
//!   and how it ended. Once it succeeded, it [refreshes](Command::refreshes) the loads and
//!   queries that depend on what it changed.
//!
//! Both are plain values: keep them in the app state, a [`PageState`](crate::extract::PageState)
//! or any struct of yours. Clones share their state.
//!
//! ```
//! use egui::Ui;
//! use egui_kit::remote::{self, Command, Query, QueryState};
//!
//! # async fn fetch_todos() -> Result<Vec<String>, String> { Ok(vec![]) }
//! # async fn post_todo(_title: String) -> Result<(), String> { Ok(()) }
//! struct Todos {
//!     list: Query<Vec<String>, String>,
//!     add: Command<(), String>,
//! }
//!
//! impl Todos {
//!     fn new() -> Self {
//!         Self {
//!             list: remote::query(fetch_todos).key("todos").depends("todos"),
//!             add: remote::command().refreshes("todos"),
//!         }
//!     }
//!
//!     fn ui(&self, ui: &mut Ui) {
//!         match self.list.state() {
//!             QueryState::Loading => {
//!                 ui.spinner();
//!             }
//!             QueryState::Ready(todos) => {
//!                 for todo in todos.iter() {
//!                     ui.label(todo);
//!                 }
//!             }
//!             QueryState::Failed(error) => {
//!                 ui.label(format!("Failed: {error}"));
//!             }
//!         }
//!
//!         if ui.add_enabled(!self.add.is_pending(), egui::Button::new("Add")).clicked() {
//!             self.add.run(post_todo("Buy milk".into()));
//!         }
//!         if let Some(error) = self.add.error() {
//!             ui.label(&*error);
//!         }
//!     }
//! }
//! ```
//!
//! # Runtimes
//!
//! Natively, queries and commands run on a Tokio runtime, so their futures must be `Send` (see
//! [`MaybeSend`]). It is the one the app entered if any (a multi-threaded one, as nothing drives
//! a current-thread runtime between frames), else one that egui-kit starts. [`RouterApp::ui`]
//! enters it too, so that hooks and loads can use Tokio-based clients like `reqwest`. On the
//! web, futures run on the browser's event loop and needn't be `Send`.
//!
//! A query or command wakes egui up when it finishes, once a [`RouterApp`] drew a frame on
//! this thread. Its result is picked up when it is read.
//!
//! # Caching
//!
//! A query fetches on its first read, and keeps its value while it refetches. With a
//! [key](Query::key), queries share their state: one fetch for all of them, and a value that
//! outlives them for [a while](Query::cache_for), so that a page opened again shows it right
//! away. A query reading a cached value for the first time refetches it if it is [older than
//! `stale_after`](Query::stale_after).
//!
//! Dropping a query's state (its last clone, and its cache entry if keyed) aborts its fetch,
//! like refreshing it aborts the one in progress. Commands always run to completion.
//!
//! [`RouterApp`]: crate::RouterApp
//! [`RouterApp::ui`]: crate::RouterApp::ui

mod command;
mod query;
mod runtime;

use core::any::{Any, TypeId};
use core::cell::RefCell;
use core::time::Duration;
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use egui::Id;
use web_time::Instant;

pub use command::{Command, command};
pub use query::{Query, QueryState, query};
pub use runtime::MaybeSend;
pub(crate) use runtime::enter;

use crate::load::Invalidation;

/// A query's state, as the registry sees it.
trait Entry {
    /// Marks it for a refetch if `invalidation` hits what it depends on.
    fn invalidate(&self, invalidation: &Invalidation);

    /// Whether a cache entry no query uses should be dropped.
    fn expired(&self, now: Instant) -> bool;

    fn as_any(self: Rc<Self>) -> Rc<dyn Any>;
}

/// The queries and commands of this thread.
#[derive(Default)]
struct Registry {
    /// Every query's state, to invalidate them.
    entries: Vec<Weak<dyn Entry>>,
    /// The state of keyed queries, by value type and key.
    cache: HashMap<(TypeId, Id), Rc<dyn Entry>>,
    /// The app's context, to wake it up when a query or command finished.
    ctx: Option<egui::Context>,
    /// Keys to invalidate, from [`invalidate`] and from the commands that succeeded.
    invalidated: Arc<Mutex<Vec<String>>>,
}

thread_local! {
    static REGISTRY: RefCell<Registry> = RefCell::default();
}

fn with_registry<R>(f: impl FnOnce(&mut Registry) -> R) -> R {
    REGISTRY.with(|registry| f(&mut registry.borrow_mut()))
}

fn context() -> Option<egui::Context> {
    with_registry(|registry| registry.ctx.clone())
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Pushes `keys` to the invalidations and wakes the app up.
fn push_invalidated(
    invalidated: &Mutex<Vec<String>>,
    ctx: Option<&egui::Context>,
    keys: impl IntoIterator<Item = String>,
) {
    lock(invalidated).extend(keys);
    if let Some(ctx) = ctx {
        ctx.request_repaint();
    }
}

/// Reruns the loads and queries that depend on `key`, like
/// [`Nav::invalidate`](crate::extract::Nav::invalidate), from anywhere on the UI thread. The
/// router applies it at its next frame.
pub fn invalidate(key: impl Into<String>) {
    let (invalidated, ctx) =
        with_registry(|registry| (Arc::clone(&registry.invalidated), registry.ctx.clone()));
    push_invalidated(&invalidated, ctx.as_ref(), [key.into()]);
}

/// Remembers the app's context, at the start of its frame.
pub(crate) fn begin_frame(ctx: &egui::Context) {
    with_registry(|registry| registry.ctx = Some(ctx.clone()));
}

/// The keys [invalidated](invalidate) since the last call.
pub(crate) fn take_invalidated() -> Vec<String> {
    let invalidated = with_registry(|registry| Arc::clone(&registry.invalidated));
    core::mem::take(&mut *lock(&invalidated))
}

/// Marks the queries that depend on what `invalidation` hits for a refetch.
pub(crate) fn invalidate_queries(invalidation: &Invalidation) {
    let entries: Vec<_> = with_registry(|registry| {
        registry.entries.retain(|entry| entry.strong_count() > 0);
        registry.entries.iter().filter_map(Weak::upgrade).collect()
    });
    for entry in entries {
        entry.invalidate(invalidation);
    }
}

/// Drops the cache entries that no query used for long enough.
pub(crate) fn end_frame() {
    let now = Instant::now();
    // Dropped outside of the registry: dropping a query's state aborts its fetch.
    let _expired: Vec<_> = with_registry(|registry| {
        let keys: Vec<_> = registry
            .cache
            .iter()
            .filter(|(_, entry)| Rc::strong_count(entry) == 1 && entry.expired(now))
            .map(|(key, _)| *key)
            .collect();
        keys.into_iter()
            .filter_map(|key| registry.cache.remove(&key))
            .collect()
    });
}

/// How long a keyed query's value is kept by default once no query uses it.
const CACHE_FOR: Duration = Duration::from_mins(5);
