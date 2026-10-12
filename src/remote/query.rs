//! [`Query`]: async data.

use core::any::{Any, TypeId};
use core::cell::{Cell, Ref, RefCell};
use core::fmt;
use core::future::Future;
use core::time::Duration;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use egui::{AsId, Id};
use web_time::Instant;

use super::runtime::{self, Abort, BoxFuture, MaybeSend};
use super::{CACHE_FOR, Entry, context, lock, with_registry};
use crate::load::Invalidation;

/// Where a fetch puts its result.
type Slot<T, E> = Arc<Mutex<Option<Result<T, E>>>>;

/// A fetch in progress, aborted when dropped.
struct Run<T, E> {
    slot: Slot<T, E>,
    _task: Abort,
}

/// The state of a query, shared by its clones and by the queries with the same key.
struct State<T, E> {
    value: Option<Rc<T>>,
    /// Why the last fetch failed.
    error: Option<Rc<E>>,
    version: u64,
    fetched_at: Option<Instant>,
    run: Option<Run<T, E>>,
    /// Refetch on the next read.
    stale: bool,
    depends: Vec<String>,
    read_at: Instant,
    cache_for: Duration,
}

impl<T: 'static, E: 'static> Entry for RefCell<State<T, E>> {
    fn invalidate(&self, invalidation: &Invalidation) {
        let mut state = self.borrow_mut();
        if invalidation.hits(&state.depends) {
            state.stale = true;
            state.run = None;
        }
    }

    fn expired(&self, now: Instant) -> bool {
        let state = self.borrow();
        now.saturating_duration_since(state.read_at) >= state.cache_for
    }

    fn as_any(self: Rc<Self>) -> Rc<dyn Any> {
        self
    }
}

/// What a [`Query`] has to show.
#[derive(Debug)]
pub enum QueryState<T, E> {
    /// The first fetch is in progress.
    Loading,
    /// The last fetch succeeded. A refetch may be in progress, see [`Query::is_fetching`].
    Ready(Rc<T>),
    /// The last fetch failed. [`Query::data`] still has the value before, if any.
    Failed(Rc<E>),
}

/// Async data: fetches on its first read, then keeps its value until it refetches.
///
/// Create one with [`query`], from a function that returns the future to fetch the data: it is
/// called for every fetch. Keep it where the views can read it (the app state, a
/// [`PageState`](crate::extract::PageState)…) and read it every frame you show it. Clones share
/// its state. See the [module docs](crate::remote).
///
/// ```
/// # use egui_kit::remote::{self, Query};
/// # #[derive(Clone)]
/// # struct Api;
/// # impl Api { async fn user(&self, _id: u32) -> Result<String, String> { Ok("Ferris".into()) } }
/// fn user(api: &Api, id: u32) -> Query<String, String> {
///     let api = api.clone();
///     remote::query(move || {
///         let api = api.clone();
///         async move { api.user(id).await }
///     })
///     .key(("user", id))
/// }
/// ```
pub struct Query<T, E> {
    state: Rc<RefCell<State<T, E>>>,
    fetch: Rc<dyn Fn() -> BoxFuture<Result<T, E>>>,
    refetch_every: Option<Duration>,
    stale_after: Duration,
    cache_for: Option<Duration>,
    /// Whether it was read: until then, it refetches a value older than `stale_after`.
    read: Cell<bool>,
}

/// A [`Query`] that fetches with `fetch`.
pub fn query<T, E, F, Fut>(fetch: F) -> Query<T, E>
where
    T: MaybeSend + 'static,
    E: MaybeSend + 'static,
    F: Fn() -> Fut + 'static,
    Fut: Future<Output = Result<T, E>> + MaybeSend + 'static,
{
    Query::new(fetch)
}

impl<T: MaybeSend + 'static, E: MaybeSend + 'static> Query<T, E> {
    /// See [`query`].
    pub fn new<F, Fut>(fetch: F) -> Self
    where
        F: Fn() -> Fut + 'static,
        Fut: Future<Output = Result<T, E>> + MaybeSend + 'static,
    {
        let state = Rc::new(RefCell::new(State {
            value: None,
            error: None,
            version: 0,
            fetched_at: None,
            run: None,
            stale: false,
            depends: Vec::new(),
            read_at: Instant::now(),
            cache_for: CACHE_FOR,
        }));
        let entry: Rc<dyn Entry> = Rc::clone(&state) as _;
        with_registry(|registry| registry.entries.push(Rc::downgrade(&entry)));
        Self {
            state,
            fetch: Rc::new(move || Box::pin(fetch())),
            refetch_every: None,
            stale_after: Duration::ZERO,
            cache_for: None,
            read: Cell::new(false),
        }
    }

    /// Shares the state of the queries with the same key (and value and error types): they
    /// fetch once for all of them, and the value is [cached](Self::cache_for). The first
    /// query with a key fetches with its function, the others use its value.
    ///
    /// The key identifies the data, so it must contain everything the fetch uses, like
    /// `("user", id)`. See [Keys, dependencies and refreshes](crate::remote#keys-dependencies-and-refreshes).
    #[must_use]
    pub fn key(mut self, key: impl AsId) -> Self {
        let id = (TypeId::of::<(T, E)>(), Id::new(key));
        let shared = with_registry(|registry| {
            if let Some(entry) = registry.cache.get(&id) {
                return Rc::clone(entry).as_any().downcast().ok();
            }
            registry
                .cache
                .insert(id, Rc::clone(&self.state) as Rc<dyn Entry>);
            None
        });
        if let Some(shared) = shared {
            let depends = core::mem::take(&mut self.state.borrow_mut().depends);
            self.state = shared;
            for key in depends {
                self = self.depends(key);
            }
        }
        self.apply_cache_for();
        self
    }

    /// Refetches when the dependency `key` is invalidated: with
    /// [`Nav::invalidate`](crate::extract::Nav::invalidate), [`invalidate`](super::invalidate), or
    /// by a command that [refreshes](super::Command::refreshes) it. Loads with the same
    /// [dependency](crate::page::Page::depends) rerun along. Call it once per dependency.
    ///
    /// A dependency names data on the backend (`"users"`), unlike the [key](Self::key) that
    /// names one value (`("user", 1)`). See [Keys, dependencies and refreshes](crate::remote#keys-dependencies-and-refreshes).
    #[must_use]
    pub fn depends(self, key: impl Into<String>) -> Self {
        let key = key.into();
        let mut state = self.state.borrow_mut();
        if !state.depends.contains(&key) {
            state.depends.push(key);
        }
        drop(state);
        self
    }

    /// Refetches this long after the last fetch finished, for as long as it is read: for data
    /// that changes elsewhere, like an inbox.
    #[must_use]
    pub fn refetch_every(mut self, every: Duration) -> Self {
        self.refetch_every = Some(every);
        self
    }

    /// How old a [cached](Self::key) value may be for this query to use it without refetching,
    /// when it first reads it. The cached value is shown while it refetches. Zero by default:
    /// a new query always refetches.
    #[must_use]
    pub fn stale_after(mut self, duration: Duration) -> Self {
        self.stale_after = duration;
        self
    }

    /// How long a [keyed](Self::key) value is kept once no query uses it. 5 minutes by default.
    #[must_use]
    pub fn cache_for(mut self, duration: Duration) -> Self {
        self.cache_for = Some(duration);
        self.apply_cache_for();
        self
    }

    fn apply_cache_for(&self) {
        if let Some(duration) = self.cache_for {
            self.state.borrow_mut().cache_for = duration;
        }
    }

    /// What to show: loading, the value, or why the last fetch failed.
    pub fn state(&self) -> QueryState<T, E> {
        let state = self.read();
        if let Some(error) = &state.error {
            QueryState::Failed(Rc::clone(error))
        } else if let Some(value) = &state.value {
            QueryState::Ready(Rc::clone(value))
        } else {
            QueryState::Loading
        }
    }

    /// The last value fetched: while it refetches, or if that failed, the previous one.
    pub fn data(&self) -> Option<Rc<T>> {
        self.read().value.clone()
    }

    /// Why the last fetch failed.
    pub fn error(&self) -> Option<Rc<E>> {
        self.read().error.clone()
    }

    /// Whether the first fetch is in progress: there is no [`data`](Self::data) yet.
    pub fn is_loading(&self) -> bool {
        let state = self.read();
        state.value.is_none() && state.run.is_some()
    }

    /// Whether a fetch is in progress, the first one or a refetch.
    pub fn is_fetching(&self) -> bool {
        self.read().run.is_some()
    }

    /// Identifies the [data](Self::data): it changes whenever a fetch succeeded, so that a view
    /// can rebuild what it derives from the value only then. `0` while there is no value.
    pub fn version(&self) -> u64 {
        self.read().version
    }

    /// Refetches, aborting the fetch in progress. The value stays until the new one arrives.
    pub fn refresh(&self) {
        let mut state = self.state.borrow_mut();
        state.stale = true;
        state.run = None;
        drop(state);
        if let Some(ctx) = context() {
            ctx.request_repaint();
        }
    }

    /// Replaces the value, e.g. with what a command returned, or optimistically before a
    /// command confirms it. A fetch in progress still replaces it when done.
    pub fn set_data(&self, value: T) {
        let mut state = self.state.borrow_mut();
        state.value = Some(Rc::new(value));
        state.error = None;
        state.version += 1;
    }

    /// Picks up a finished fetch, and starts one if due.
    fn read(&self) -> Ref<'_, State<T, E>> {
        let now = Instant::now();
        let mut state = self.state.borrow_mut();
        state.read_at = now;
        if let Some(result) = state.run.as_ref().and_then(|run| lock(&run.slot).take()) {
            state.run = None;
            state.fetched_at = Some(now);
            match result {
                Ok(value) => {
                    state.value = Some(Rc::new(value));
                    state.error = None;
                    state.version += 1;
                }
                Err(error) => state.error = Some(Rc::new(error)),
            }
        }
        if state.run.is_none() && self.due(&state, now) {
            state.stale = false;
            state.run = Some(self.start());
        }
        drop(state);
        self.read.set(true);
        self.state.borrow()
    }

    fn due(&self, state: &State<T, E>, now: Instant) -> bool {
        let Some(fetched_at) = state.fetched_at else {
            return true;
        };
        let age = now.saturating_duration_since(fetched_at);
        if state.stale || (!self.read.get() && age >= self.stale_after) {
            return true;
        }
        match self.refetch_every {
            Some(every) if age >= every => true,
            Some(every) => {
                if let Some(ctx) = context() {
                    ctx.request_repaint_after(every.saturating_sub(age));
                }
                false
            }
            None => false,
        }
    }

    fn start(&self) -> Run<T, E> {
        let slot: Slot<T, E> = Arc::default();
        let future = (self.fetch)();
        let ctx = context();
        let task = runtime::spawn({
            let slot = Arc::clone(&slot);
            async move {
                let result = future.await;
                *lock(&slot) = Some(result);
                if let Some(ctx) = ctx {
                    ctx.request_repaint();
                }
            }
        });
        Run { slot, _task: task }
    }
}

impl<T, E> Clone for Query<T, E> {
    fn clone(&self) -> Self {
        Self {
            state: Rc::clone(&self.state),
            fetch: Rc::clone(&self.fetch),
            refetch_every: self.refetch_every,
            stale_after: self.stale_after,
            cache_for: self.cache_for,
            read: self.read.clone(),
        }
    }
}

impl<T: fmt::Debug, E: fmt::Debug> fmt::Debug for Query<T, E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.state.borrow();
        f.debug_struct("Query")
            .field("value", &state.value)
            .field("error", &state.error)
            .field("version", &state.version)
            .field("fetching", &state.run.is_some())
            .finish_non_exhaustive()
    }
}
