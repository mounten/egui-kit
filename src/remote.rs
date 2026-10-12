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
//! # Example
//!
//! A todo list that fetches the todos from a backend, and adds one with a `POST`:
//!
//! ```
//! use std::sync::{Arc, Mutex};
//!
//! use egui::Ui;
//! use egui_kit::remote::{self, Command, Query, QueryState};
//!
//! /// The backend. A stand-in for an HTTP client like `reqwest::Client`: cheap to clone, and
//! /// `Send`, as queries and commands run on other threads.
//! #[derive(Clone, Default)]
//! struct Api {
//!     todos: Arc<Mutex<Vec<String>>>,
//! }
//!
//! impl Api {
//!     /// `GET /todos`
//!     async fn todos(&self) -> Result<Vec<String>, String> {
//!         Ok(self.todos.lock().unwrap().clone())
//!     }
//!
//!     /// `POST /todos`
//!     async fn add_todo(&self, title: String) -> Result<(), String> {
//!         if title.is_empty() {
//!             return Err("a todo needs a title".into());
//!         }
//!         self.todos.lock().unwrap().push(title);
//!         Ok(())
//!     }
//! }
//!
//! struct TodoList {
//!     api: Api,
//!     todos: Query<Vec<String>, String>,
//!     add: Command<(), String>,
//!     title: String,
//! }
//!
//! impl TodoList {
//!     fn new(api: Api) -> Self {
//!         // The closure is called for every fetch (the first one, refreshes, refetches), and
//!         // returns a new future each time, with its own clone of the client.
//!         let todos = remote::query({
//!             let api = api.clone();
//!             move || {
//!                 let api = api.clone();
//!                 async move { api.todos().await }
//!             }
//!         })
//!         // Every query with this key, anywhere in the app, shares this one's value.
//!         .key("todos")
//!         // Refetch when "todos" is invalidated, e.g. by the command below.
//!         .depends("todos");
//!
//!         Self {
//!             api,
//!             todos,
//!             // Invalidates "todos" once a todo was added.
//!             add: remote::command().refreshes("todos"),
//!             title: String::new(),
//!         }
//!     }
//!
//!     fn ui(&mut self, ui: &mut Ui) {
//!         match self.todos.state() {
//!             QueryState::Loading => {
//!                 ui.spinner();
//!             }
//!             QueryState::Ready(todos) => {
//!                 for todo in todos.iter() {
//!                     ui.label(todo);
//!                 }
//!             }
//!             QueryState::Failed(error) => {
//!                 ui.label(format!("Couldn't load the todos: {error}"));
//!             }
//!         }
//!
//!         ui.horizontal(|ui| {
//!             ui.text_edit_singleline(&mut self.title);
//!             if ui.add_enabled(!self.add.is_pending(), egui::Button::new("Add")).clicked() {
//!                 let api = self.api.clone();
//!                 let title = core::mem::take(&mut self.title);
//!                 self.add.run(async move { api.add_todo(title).await });
//!             }
//!         });
//!         if let Some(error) = self.add.error() {
//!             ui.label(&*error);
//!         }
//!     }
//! }
//! ```
//!
//! When the `POST` succeeds, the command invalidates `"todos"`, the query refetches, and the
//! list shows the new todo. Nothing else has to tell the list that it changed.
//!
//! # Keys, dependencies and refreshes
//!
//! They answer different questions:
//!
//! | | Answers | Set with |
//! |---|---|---|
//! | key | *Which* data is this? Queries with the same key are the same data. | [`Query::key`] |
//! | dependency | *When* is it outdated? It refetches when one of its dependencies is invalidated. | [`Query::depends`], [`Page::depends`] |
//! | refresh | *What* did a mutation change? The dependencies it invalidates when it succeeds. | [`Command::refreshes`] |
//!
//! A **key** identifies a query's value in the cache. Queries with the same key (and value and
//! error types) share one state: they fetch once for all of them, and a query created later
//! finds the value right away, e.g. when a page is opened again. So the key must contain
//! everything the fetch depends on: `("user", id)`, not `"user"`. A query without a key has a
//! value of its own, which is dropped with it.
//!
//! A **dependency** is a name for some data on the backend, like `"todos"` or `"users"`. It is
//! the same kind of name as a load's [`Page::depends`]: a single invalidation reruns the loads
//! and refetches the queries that depend on it, so many queries can depend on one name, and a
//! query can depend on several. Dependencies are coarser than keys: `("user", 1)` and
//! `("user", 2)` are two keys that both depend on `"users"`.
//!
//! A **refresh** invalidates a dependency once a command succeeded: the command says what it
//! changed, and doesn't need to know who shows it. Invalidating a dependency directly does the
//! same, with [`Nav::invalidate`] in a view, or with [`invalidate`] from anywhere on the UI
//! thread.
//!
//! ```
//! # use egui_kit::remote::{self, Command, Query};
//! # #[derive(Clone)]
//! # struct Api;
//! # impl Api {
//! #     async fn user(&self, _id: u32) -> Result<String, String> { Ok("Ferris".into()) }
//! #     async fn rename(&self, _id: u32, _name: String) -> Result<(), String> { Ok(()) }
//! # }
//! fn user(api: &Api, id: u32) -> Query<String, String> {
//!     let api = api.clone();
//!     remote::query(move || {
//!         let api = api.clone();
//!         async move { api.user(id).await }
//!     })
//!     .key(("user", id)) // one cache entry per user
//!     .depends("users") // refetched when any user changes
//! }
//!
//! fn rename(api: &Api, rename: &Command<(), String>, id: u32, name: String) {
//!     let api = api.clone();
//!     // `rename` was created with `remote::command().refreshes("users")`: once it succeeded,
//!     // every user query refetches, and so do the loads with `.depends("users")`.
//!     rename.run(async move { api.rename(id, name).await });
//! }
//! ```
//!
//! The [`RouterApp`] applies invalidations at its next frame. A query that is invalidated keeps
//! its value, and refetches the next time it is read.
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
//! A query fetches on its first read, and keeps its value while it refetches. A keyed value
//! outlives its queries for [a while](Query::cache_for), so that a page opened again shows it
//! right away. A query reading a cached value for the first time refetches it if it is [older
//! than `stale_after`](Query::stale_after), showing the cached value meanwhile.
//!
//! Dropping a query's state (its last clone, and its cache entry if keyed) aborts its fetch,
//! like refreshing it aborts the one in progress. Commands always run to completion.
//!
//! [`RouterApp`]: crate::RouterApp
//! [`RouterApp::ui`]: crate::RouterApp::ui
//! [`Page::depends`]: crate::page::Page::depends
//! [`Nav::invalidate`]: crate::extract::Nav::invalidate

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
