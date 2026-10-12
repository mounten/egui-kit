//! Queries and commands: data and mutations that aren't tied to a route, inspired by SvelteKit's
//! [remote functions](https://svelte.dev/docs/kit/remote-functions).
//!
//! A route's `load` fetches what its page needs before it is shown. Some data doesn't belong to
//! a route, though: a badge in a layout, an autocomplete, a count that several pages show. And
//! changing data is often async too (a `POST` to a backend), which a view can't `await`.
//!
//! - A [`Query`] is async data that any view can read with [`Remote::query`]. The value is the
//!   query's input, and also its cache key: views reading equal queries share one result.
//! - A [`Command`] is an async mutation, started from a view with [`CommandHandle::run`]. Its
//!   state (pending, output, error) belongs to the view, and when it succeeds it
//!   [refreshes](Command::refreshes) the loads and queries that depend on what it changed.
//!
//! ```
//! use egui::Ui;
//! use egui_kit::RouteError;
//! use egui_kit::extract::Remote;
//! use egui_kit::remote::{Command, Query};
//!
//! #[derive(Clone, Default)]
//! struct AppState;
//!
//! /// How many todos there are.
//! #[derive(Clone, PartialEq)]
//! struct TodoCount;
//!
//! impl Query<AppState> for TodoCount {
//!     type Output = usize;
//!
//!     async fn run(self, _state: AppState) -> Result<usize, RouteError> {
//!         Ok(3) // e.g. a `GET /todos/count`
//!     }
//!
//!     fn depends(&self) -> Vec<String> {
//!         vec!["todos".into()]
//!     }
//! }
//!
//! struct AddTodo(String);
//!
//! impl Command<AppState> for AddTodo {
//!     type Output = ();
//!
//!     async fn run(self, _state: AppState) -> Result<(), RouteError> {
//!         Ok(()) // e.g. a `POST /todos`
//!     }
//!
//!     fn refreshes(&self) -> Vec<String> {
//!         vec!["todos".into()]
//!     }
//! }
//!
//! fn view(ui: &mut Ui, remote: Remote<'_, AppState>) {
//!     match remote.query(TodoCount).current() {
//!         Some(count) => ui.label(format!("{count} todos")),
//!         None => ui.spinner(),
//!     };
//!
//!     let add = remote.command::<AddTodo>();
//!     if ui.add_enabled(!add.is_pending(), egui::Button::new("Add")).clicked() {
//!         add.run(AddTodo("Buy milk".into()));
//!     }
//!     if let Some(error) = add.error() {
//!         ui.label(error.message);
//!     }
//! }
//! ```
//!
//! Like `load`s, queries and commands are polled by the router, so no async runtime is needed,
//! and their futures don't have to be `Send`. Natively, clients built on Tokio need its runtime
//! entered around [`RouterApp::ui`](crate::RouterApp::ui); see
//! [Async runtimes](crate#async-runtimes).
//!
//! # Caching
//!
//! A query runs the first time a view reads it, and its result is kept for as long as views keep
//! reading it: it is dropped after a frame in which no view did, or [a while
//! later](Query::keep_unread). While it reruns (after an
//! [invalidation](crate::extract::Nav::invalidate) or a [refresh](Remote::refresh)),
//! [`QueryState::current`] still has the previous value. A query that [refetches
//! periodically](Query::refetch_every) reruns the same way.

use core::any::{Any, TypeId};
use core::cell::{Cell, RefCell};
use core::future::Future;
use core::marker::PhantomData;
use core::task::Poll;
use core::time::Duration;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use crate::error::RouteError;
use crate::exec;
use crate::extract::{FromView, ViewStore};
use crate::load::Invalidation;
use crate::page::{Frame, LocalBoxFuture, ViewContext};

/// Async data that views read with [`Remote::query`], cached by value.
///
/// The type is the query and its value the input: `SearchAuthors("fe".into())` and
/// `SearchAuthors("fer".into())` are two entries of the same query, and every view reading an
/// equal value shares one result.
pub trait Query<S>: Clone + PartialEq + 'static {
    type Output: 'static;

    /// Fetches the data, with a clone of the app state.
    fn run(self, state: S) -> impl Future<Output = Result<Self::Output, RouteError>> + 'static;

    /// Reruns the query when one of these keys is [invalidated](crate::extract::Nav::invalidate),
    /// like [`Page::depends`](crate::page::Page::depends).
    fn depends(&self) -> Vec<String> {
        Vec::new()
    }

    /// Reruns the query this long after its last run finished, for as long as views read it:
    /// for data that changes elsewhere, like an inbox. `None` (the default) only reruns it when
    /// it is invalidated or [refreshed](Remote::refresh).
    fn refetch_every(&self) -> Option<Duration> {
        None
    }

    /// How long the result is kept once no view reads it, so that a view reading it again (after
    /// switching tabs, or reopening a popup) finds it, and doesn't wait for a rerun. By default
    /// it is dropped after the first frame in which no view read it.
    ///
    /// A query isn't polled while no view reads it: a run in progress continues once one does.
    fn keep_unread(&self) -> Duration {
        Duration::ZERO
    }
}

/// An async mutation that views start with [`CommandHandle::run`].
pub trait Command<S>: 'static {
    type Output: 'static;

    /// Carries out the mutation, with a clone of the app state.
    fn run(self, state: S) -> impl Future<Output = Result<Self::Output, RouteError>> + 'static;

    /// The keys to [invalidate](crate::extract::Nav::invalidate) once the command succeeded,
    /// rerunning the loads and queries that depend on them.
    fn refreshes(&self) -> Vec<String> {
        Vec::new()
    }
}

type QueryFuture = LocalBoxFuture<Result<Rc<dyn Any>, RouteError>>;

/// A query's cached result.
struct QueryEntry {
    /// The query, to compare with the ones views read.
    query: Box<dyn Any>,
    keys: Vec<String>,
    refetch_every: Option<Duration>,
    /// When the last run finished, in egui's time.
    finished_at: Option<f64>,
    /// The last value it fetched.
    value: Option<Rc<dyn Any>>,
    /// Identifies `value`.
    version: u64,
    /// Whether the last run failed.
    error: Option<RouteError>,
    future: Option<QueryFuture>,
    /// Rerun on the next read.
    stale: bool,
    /// Whether a view read it during this frame.
    read: bool,
    keep_unread: Duration,
    /// When a view last read it, in egui's time.
    read_at: f64,
}

/// The queries' cache and the commands in flight, owned by the app.
#[derive(Default)]
pub(crate) struct RemoteStore {
    queries: RefCell<Vec<QueryEntry>>,
    /// The last version given to a value, so that versions are never reused.
    version: Cell<u64>,
    /// Yield the keys to invalidate.
    commands: RefCell<Vec<LocalBoxFuture<Vec<String>>>>,
}

impl RemoteStore {
    fn query<S: Clone, Q: Query<S>>(
        &self,
        query: &Q,
        state: &S,
        ctx: &egui::Context,
    ) -> QueryState<Q::Output> {
        let mut queries = self.queries.borrow_mut();
        let index = queries
            .iter()
            .position(|entry| entry.query.downcast_ref::<Q>() == Some(query))
            .unwrap_or_else(|| {
                queries.push(QueryEntry {
                    query: Box::new(query.clone()),
                    keys: query.depends(),
                    refetch_every: query.refetch_every(),
                    finished_at: None,
                    value: None,
                    version: 0,
                    error: None,
                    future: None,
                    stale: true,
                    read: false,
                    keep_unread: query.keep_unread(),
                    read_at: 0.0,
                });
                queries.len() - 1
            });
        let entry = &mut queries[index];
        entry.read = true;
        let now = ctx.input(|i| i.time);
        if let (Some(every), Some(finished_at), None) =
            (entry.refetch_every, entry.finished_at, &entry.future)
        {
            let due = finished_at + every.as_secs_f64();
            if now >= due {
                entry.stale = true;
            } else {
                ctx.request_repaint_after_secs((due - now) as f32);
            }
        }
        entry.read_at = now;
        if entry.stale {
            entry.stale = false;
            let future = query.clone().run(state.clone());
            entry.future = Some(Box::pin(async move {
                future.await.map(|value| Rc::new(value) as Rc<dyn Any>)
            }));
        }
        // Polled when read, so a query that is ready right away never shows a loading state.
        if let Some(future) = &mut entry.future
            && let Poll::Ready(result) = exec::poll_once(ctx, future.as_mut())
        {
            entry.future = None;
            entry.finished_at = Some(now);
            if let Some(every) = entry.refetch_every {
                ctx.request_repaint_after(every);
            }
            match result {
                Ok(value) => {
                    entry.value = Some(value);
                    self.version.set(self.version.get() + 1);
                    entry.version = self.version.get();
                    entry.error = None;
                }
                Err(error) => entry.error = Some(error),
            }
        }
        QueryState {
            value: entry
                .value
                .clone()
                .map(|value| value.downcast().expect("stored by type")),
            version: entry.version,
            error: entry.error.clone(),
            loading: entry.future.is_some(),
        }
    }

    /// Reruns `query` on its next read.
    fn refresh<Q: 'static + PartialEq>(&self, query: &Q) {
        for entry in self.queries.borrow_mut().iter_mut() {
            if entry.query.downcast_ref::<Q>() == Some(query) {
                entry.stale = true;
                entry.future = None;
            }
        }
    }

    /// Reruns the queries that depend on what `invalidation` hits, on their next read.
    pub(crate) fn invalidate(&self, invalidation: &Invalidation) {
        for entry in self.queries.borrow_mut().iter_mut() {
            if invalidation.hits(&entry.keys) {
                entry.stale = true;
                entry.future = None;
            }
        }
    }

    /// Drops the queries no view read during the frame, once they were [kept](Query::keep_unread)
    /// long enough.
    pub(crate) fn end_frame(&self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        self.queries.borrow_mut().retain_mut(|entry| {
            let read = entry.read;
            entry.read = false;
            read || now - entry.read_at < entry.keep_unread.as_secs_f64()
        });
    }

    /// Drives the commands in flight. Returns the keys that those that succeeded refresh.
    pub(crate) fn poll_commands(&self, ctx: &egui::Context) -> Vec<String> {
        let mut refreshes = Vec::new();
        self.commands.borrow_mut().retain_mut(|future| {
            match exec::poll_once(ctx, future.as_mut()) {
                Poll::Ready(keys) => {
                    refreshes.extend(keys);
                    false
                }
                Poll::Pending => true,
            }
        });
        refreshes
    }
}

/// A query's result, as seen by a view: see [`Remote::query`].
pub struct QueryState<T> {
    value: Option<Rc<T>>,
    version: u64,
    error: Option<RouteError>,
    loading: bool,
}

impl<T> QueryState<T> {
    /// The last value the query fetched. While it reruns, or if the rerun failed, this is the
    /// previous value.
    pub fn current(&self) -> Option<&T> {
        self.value.as_deref()
    }

    /// The current value, shared with the cache: cheap to keep beyond the frame.
    pub fn rc(&self) -> Option<Rc<T>> {
        self.value.clone()
    }

    /// Identifies the [current](Self::current) value: it changes whenever the query fetched a
    /// new one, so that a view can rebuild what it derives from the value only then. `0` while
    /// there is no value.
    ///
    /// ```
    /// # use egui_kit::remote::QueryState;
    /// #[derive(Default)]
    /// struct Calendar {
    ///     version: u64,
    ///     items: Vec<String>,
    /// }
    ///
    /// fn update(calendar: &mut Calendar, events: &QueryState<Vec<String>>) {
    ///     if events.version() != calendar.version {
    ///         calendar.version = events.version();
    ///         calendar.items = events.current().cloned().unwrap_or_default();
    ///     }
    /// }
    /// ```
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Why the last run failed.
    pub fn error(&self) -> Option<&RouteError> {
        self.error.as_ref()
    }

    /// Whether the query is running: for the first time if [`current`](Self::current) is
    /// `None`, else rerunning.
    pub fn is_loading(&self) -> bool {
        self.loading
    }
}

impl<T: fmt::Debug> fmt::Debug for QueryState<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("QueryState")
            .field("current", &self.value)
            .field("version", &self.version)
            .field("error", &self.error)
            .field("loading", &self.loading)
            .finish()
    }
}

/// Queries and commands, in views: see the [module docs](crate::remote).
///
/// An extractor: add a `remote: Remote<'_, AppState>` argument to any view. It is `Copy`.
pub struct Remote<'f, S> {
    frame: &'f Frame<'f>,
    store: &'f ViewStore,
    state: &'f S,
}

impl<S> Clone for Remote<'_, S> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<S> Copy for Remote<'_, S> {}

impl<S: 'static, D> FromView<S, D> for Remote<'_, S> {
    type Item<'f> = Remote<'f, S>;

    fn from_view<'f>(cx: &'f ViewContext<'_, S>) -> Remote<'f, S> {
        Remote {
            frame: cx.frame(),
            store: cx.store(),
            state: cx.state(),
        }
    }
}

impl<'f, S: Clone + 'static> Remote<'f, S> {
    /// Reads `query`, running it if it isn't cached. Call it every frame you show the result:
    /// a query no view reads during a frame is dropped (see [`Query::keep_unread`]).
    #[expect(
        clippy::needless_pass_by_value,
        reason = "`remote.query(SearchAuthors(prefix))` reads better than with a reference"
    )]
    pub fn query<Q: Query<S>>(&self, query: Q) -> QueryState<Q::Output> {
        self.frame.remote.query(&query, self.state, &self.frame.ctx)
    }

    /// Reruns `query` the next time it is read. To rerun every query that depends on some data,
    /// [invalidate](crate::extract::Nav::invalidate) it instead.
    pub fn refresh<Q: Query<S>>(&self, query: &Q) {
        self.frame.remote.refresh(query);
        self.frame.ctx.request_repaint();
    }

    /// The view's handle to the command `C`: to run it, and to see whether it's running and
    /// what it returned.
    ///
    /// Its state belongs to the view, for as long as its page (or layout) is shown. A command
    /// still running when the page is left completes and refreshes what it changed, but its
    /// output is dropped.
    pub fn command<C: Command<S>>(&self) -> CommandHandle<'f, S, C> {
        let slot = Rc::clone(
            self.store
                .commands
                .borrow_mut()
                .entry(TypeId::of::<C>())
                .or_insert_with(|| Rc::new(RefCell::new(CommandSlot::<C::Output>::default()))),
        );
        CommandHandle {
            remote: *self,
            slot: slot.downcast().expect("stored by type"),
            command: PhantomData,
        }
    }
}

/// The state of a view's command.
struct CommandSlot<T> {
    pending: usize,
    /// How the last run ended, until the view takes it.
    outcome: Option<Result<T, RouteError>>,
    /// Why the last run failed, until it runs again.
    error: Option<RouteError>,
}

impl<T> Default for CommandSlot<T> {
    fn default() -> Self {
        Self {
            pending: 0,
            outcome: None,
            error: None,
        }
    }
}

/// Commands' state, per view, by command type.
pub(crate) type CommandSlots = RefCell<HashMap<TypeId, Rc<dyn Any>>>;

/// A view's handle to the command `C`, from [`Remote::command`].
pub struct CommandHandle<'f, S, C: Command<S>> {
    remote: Remote<'f, S>,
    slot: Rc<RefCell<CommandSlot<C::Output>>>,
    command: PhantomData<fn(C)>,
}

impl<S: Clone + 'static, C: Command<S>> CommandHandle<'_, S, C> {
    /// Starts `command`. Ignored in an [outgoing](crate::extract::Nav::is_outgoing) page.
    pub fn run(&self, command: C) {
        if self.remote.frame.outgoing {
            return;
        }
        let refreshes = command.refreshes();
        let future = command.run(self.remote.state.clone());
        let slot = Rc::clone(&self.slot);
        {
            let mut slot = slot.borrow_mut();
            slot.pending += 1;
            slot.error = None;
        }
        self.remote
            .frame
            .remote
            .commands
            .borrow_mut()
            .push(Box::pin(async move {
                let result = future.await;
                let mut slot = slot.borrow_mut();
                slot.pending -= 1;
                if let Err(error) = &result {
                    slot.error = Some(error.clone());
                }
                let refreshes = if result.is_ok() {
                    refreshes
                } else {
                    Vec::new()
                };
                slot.outcome = Some(result);
                refreshes
            }));
        self.remote.frame.ctx.request_repaint();
    }

    /// Whether the command is running.
    pub fn is_pending(&self) -> bool {
        self.slot.borrow().pending > 0
    }

    /// What the command returned, if its last run succeeded: taken, so that the view acts on it
    /// once (e.g. navigates to what it created).
    pub fn take_output(&self) -> Option<C::Output> {
        let mut slot = self.slot.borrow_mut();
        match slot.outcome.take()? {
            Ok(output) => Some(output),
            Err(error) => {
                slot.outcome = Some(Err(error));
                None
            }
        }
    }

    /// Why the last run failed, taken like [`take_output`](Self::take_output): to react once
    /// to a failure (a dialog, a shake). [`error`](Self::error) still has it.
    pub fn take_error(&self) -> Option<RouteError> {
        let mut slot = self.slot.borrow_mut();
        match slot.outcome.take()? {
            Ok(output) => {
                slot.outcome = Some(Ok(output));
                None
            }
            Err(error) => Some(error),
        }
    }

    /// How the last run ended, taken: once per run, in the frame after it completed (unless
    /// taken with [`take_output`](Self::take_output) or [`take_error`](Self::take_error)).
    ///
    /// ```
    /// # use egui_kit::remote::{Command, CommandHandle};
    /// # fn react<S: Clone + 'static, C: Command<S>>(ui: &mut egui::Ui, save: CommandHandle<'_, S, C>) {
    /// match save.take_result() {
    ///     Some(Ok(_)) => { /* close the dialog */ }
    ///     Some(Err(error)) => { /* show the error in a toast */ }
    ///     None => {}
    /// }
    /// # }
    /// ```
    pub fn take_result(&self) -> Option<Result<C::Output, RouteError>> {
        self.slot.borrow_mut().outcome.take()
    }

    /// Why the command last failed, to show it: kept until it runs again.
    pub fn error(&self) -> Option<RouteError> {
        self.slot.borrow().error.clone()
    }
}
