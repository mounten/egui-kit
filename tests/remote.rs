//! Queries and commands: caching, dropping unread queries, invalidation, and command state.

use core::cell::{Cell, RefCell};
use core::future::{Future, poll_fn, ready};
use core::task::Poll;
use core::time::Duration;
use std::rc::Rc;

use egui::Ui;
use egui_kit::extract::{Nav, Remote, State};
use egui_kit::page::{Layout, Outlet, Page};
use egui_kit::remote::{Command, Query, Running};
use egui_kit::{MemoryHistory, Rejection, RouteError, Router, RouterApp};

/// No runs.
const NOTHING: [&str; 0] = [];

type Action = Box<dyn FnOnce(Remote<'_, AppState>, Nav<'_>)>;

#[derive(Clone)]
struct AppState {
    /// What ran: queries, commands and loads.
    runs: Rc<RefCell<Vec<String>>>,
    /// What the page view saw of `Count`.
    seen: Rc<RefCell<Vec<String>>>,
    actions: Rc<RefCell<Vec<Action>>>,
    count: Rc<Cell<u32>>,
    /// Queries and commands wait while it's closed.
    gate: Rc<Cell<bool>>,
    /// Whether the views read the queries.
    show: Rc<Cell<bool>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            runs: Rc::default(),
            seen: Rc::default(),
            actions: Rc::default(),
            count: Rc::default(),
            gate: Rc::new(Cell::new(true)),
            show: Rc::new(Cell::new(true)),
        }
    }
}

impl AppState {
    fn ran(&self, what: impl Into<String>) {
        self.runs.borrow_mut().push(what.into());
    }

    async fn gate(&self) {
        poll_fn(|_| {
            if self.gate.get() {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;
    }
}

#[derive(Clone, PartialEq)]
struct Count;

impl Query<AppState> for Count {
    type Output = u32;

    async fn run(self, state: AppState) -> Result<u32, RouteError> {
        state.ran("count");
        state.gate().await;
        Ok(state.count.get())
    }

    fn depends(&self) -> Vec<String> {
        vec!["count".into()]
    }
}

#[derive(Clone, PartialEq)]
struct Double(u32);

impl Query<AppState> for Double {
    type Output = u32;

    fn run(self, state: AppState) -> impl Future<Output = Result<u32, RouteError>> {
        state.ran(format!("double:{}", self.0));
        ready(Ok(self.0 * 2))
    }
}

/// Polls `count` every 5 seconds.
#[derive(Clone, PartialEq)]
struct Inbox;

impl Query<AppState> for Inbox {
    type Output = u32;

    async fn run(self, state: AppState) -> Result<u32, RouteError> {
        state.ran("inbox");
        state.gate().await;
        Ok(state.count.get())
    }

    fn refetch_every(&self) -> Option<Duration> {
        Some(Duration::from_secs(5))
    }
}

/// Kept for 10 seconds without readers.
#[derive(Clone, PartialEq)]
struct Kept;

impl Query<AppState> for Kept {
    type Output = u32;

    fn run(self, state: AppState) -> impl Future<Output = Result<u32, RouteError>> {
        state.ran("kept");
        ready(Ok(state.count.get()))
    }

    fn keep_unread(&self) -> Duration {
        Duration::from_secs(10)
    }
}

struct Increment {
    fail: bool,
}

impl Command<AppState> for Increment {
    type Output = u32;

    async fn run(self, state: AppState) -> Result<u32, RouteError> {
        state.ran("increment");
        state.gate().await;
        if self.fail {
            return Err(RouteError::new(409, "nope"));
        }
        state.count.set(state.count.get() + 1);
        Ok(state.count.get())
    }

    fn refreshes(&self) -> Vec<String> {
        vec!["count".into()]
    }
}

async fn home_load(State(state): State<AppState>) -> Result<(), Rejection> {
    state.ran("load");
    Ok(())
}

fn layout(
    ui: &mut Ui,
    mut outlet: Outlet<'_>,
    State(state): State<&AppState>,
    remote: Remote<'_, AppState>,
    nav: Nav<'_>,
) {
    if state.show.get() {
        remote.query(Count);
    }
    let actions = core::mem::take(&mut *state.actions.borrow_mut());
    for action in actions {
        action(remote, nav);
    }
    outlet.show(ui);
}

fn home(_: &mut Ui, State(state): State<&AppState>, remote: Remote<'_, AppState>) {
    if !state.show.get() {
        return;
    }
    let count = remote.query(Count);
    state.seen.borrow_mut().push(format!(
        "{:?}{}",
        count.current(),
        if count.is_loading() { " loading" } else { "" }
    ));
    for n in [1, 2, 1] {
        let doubled = remote.query(Double(n));
        assert_eq!(doubled.current(), Some(&(n * 2)), "ready in the same frame");
    }
}

fn app() -> RouterApp<AppState> {
    Router::new()
        .layout(Layout::new(layout))
        .page("/", Page::with_load(home_load, home).depends("count"))
        .page("/other", Page::new(|_: &mut Ui| {}))
        .with_state_and_history(AppState::default(), MemoryHistory::new("/"))
}

/// Runs a frame; returns what ran.
fn frame(ctx: &egui::Context, app: &mut RouterApp<AppState>) -> Vec<String> {
    let mut output = ctx.run_ui(Default::default(), |ui| app.ui(ui));
    output.textures_delta.clear();
    core::mem::take(&mut *app.state().runs.borrow_mut())
}

fn seen(app: &RouterApp<AppState>) -> Vec<String> {
    core::mem::take(&mut *app.state().seen.borrow_mut())
}

fn act(app: &RouterApp<AppState>, action: impl FnOnce(Remote<'_, AppState>, Nav<'_>) + 'static) {
    app.state().actions.borrow_mut().push(Box::new(action));
}

#[test]
fn queries_are_shared_by_value_and_ready_in_the_same_frame() {
    let ctx = egui::Context::default();
    let mut app = app();
    // The layout and the page read `Count`, the page reads `Double(1)` twice: one run each.
    assert_eq!(
        frame(&ctx, &mut app),
        ["load", "count", "double:1", "double:2"]
    );
    assert_eq!(seen(&app), ["Some(0)"]);
    assert_eq!(frame(&ctx, &mut app), NOTHING);
    assert_eq!(seen(&app), ["Some(0)"]);
}

#[test]
fn unread_queries_are_dropped() {
    let ctx = egui::Context::default();
    let mut app = app();
    frame(&ctx, &mut app);
    app.state().show.set(false);
    assert_eq!(frame(&ctx, &mut app), NOTHING);
    app.state().show.set(true);
    assert_eq!(frame(&ctx, &mut app), ["count", "double:1", "double:2"]);
}

#[test]
fn invalidation_reruns_queries_keeping_the_previous_value() {
    let ctx = egui::Context::default();
    let mut app = app();
    frame(&ctx, &mut app);
    seen(&app);

    app.state().gate.set(false);
    app.state().count.set(5);
    app.invalidate("count");
    assert_eq!(frame(&ctx, &mut app), ["load", "count"]);
    assert_eq!(seen(&app), ["Some(0) loading"]);

    app.state().gate.set(true);
    assert_eq!(frame(&ctx, &mut app), NOTHING);
    assert_eq!(seen(&app), ["Some(5)"]);

    // A new value has a new version, and is shared with the cache.
    let versions = Rc::new(RefCell::new(Vec::new()));
    let read_version = |app: &RouterApp<AppState>| {
        let versions = Rc::clone(&versions);
        act(app, move |remote, _| {
            let count = remote.query(Count);
            let rc = count.rc().unwrap();
            assert!(core::ptr::eq(Rc::as_ptr(&rc), count.current().unwrap()));
            versions.borrow_mut().push(count.version());
        });
    };
    read_version(&app);
    frame(&ctx, &mut app);
    read_version(&app);
    frame(&ctx, &mut app);
    app.invalidate("count");
    read_version(&app);
    frame(&ctx, &mut app);
    let versions = versions.borrow();
    assert!(
        versions[0] > 0 && versions[0] == versions[1] && versions[2] > versions[1],
        "{versions:?}"
    );

    // Other keys leave it alone.
    app.invalidate("other");
    assert_eq!(frame(&ctx, &mut app), NOTHING);

    // Refreshed in the layout: the page's read later in the frame reruns it.
    act(&app, |remote, _| remote.refresh(&Count));
    assert_eq!(frame(&ctx, &mut app), ["count"]);
    assert_eq!(frame(&ctx, &mut app), NOTHING);
}

#[test]
fn commands_report_their_state_and_refresh_on_success() {
    let ctx = egui::Context::default();
    let mut app = app();
    frame(&ctx, &mut app);
    seen(&app);

    app.state().gate.set(false);
    act(&app, |remote, _| {
        let increment = remote.command::<Increment>();
        increment.run(Increment { fail: false });
        assert!(increment.is_pending());
    });
    assert_eq!(frame(&ctx, &mut app), ["increment"]);
    act(&app, |remote, _| {
        assert!(remote.command::<Increment>().is_pending());
    });
    assert_eq!(frame(&ctx, &mut app), NOTHING);

    // Done: it refreshes the page's load and the query.
    app.state().gate.set(true);
    assert_eq!(frame(&ctx, &mut app), ["load", "count"]);
    act(&app, |remote, _| {
        let increment = remote.command::<Increment>();
        assert!(!increment.is_pending());
        assert_eq!(increment.take_output(), Some(1));
        assert_eq!(increment.take_output(), None);
        assert_eq!(increment.error(), None);
    });
    frame(&ctx, &mut app);
    assert_eq!(seen(&app).last().map(String::as_str), Some("Some(1)"));

    // A failure is kept until the next run, and refreshes nothing.
    act(&app, |remote, _| {
        remote.command::<Increment>().run(Increment { fail: true });
    });
    assert_eq!(frame(&ctx, &mut app), ["increment"]);
    act(&app, |remote, _| {
        let increment = remote.command::<Increment>();
        assert_eq!(increment.error(), Some(RouteError::new(409, "nope")));
        assert_eq!(increment.take_output(), None);
        assert_eq!(increment.take_error(), Some(RouteError::new(409, "nope")));
        assert_eq!(increment.take_error(), None);
        assert_eq!(increment.error(), Some(RouteError::new(409, "nope")));
    });
    assert_eq!(frame(&ctx, &mut app), NOTHING);

    // `take_result` reports each run once, whatever its outcome.
    act(&app, |remote, _| {
        remote.command::<Increment>().run(Increment { fail: false });
    });
    frame(&ctx, &mut app);
    act(&app, |remote, _| {
        let increment = remote.command::<Increment>();
        assert_eq!(increment.error(), None);
        assert_eq!(increment.take_result(), Some(Ok(2)));
        assert_eq!(increment.take_result(), None);
    });
    frame(&ctx, &mut app);
}

#[test]
fn commands_complete_after_their_page_is_left() {
    let ctx = egui::Context::default();
    let mut app = app();
    frame(&ctx, &mut app);

    app.state().gate.set(false);
    act(&app, |remote, nav| {
        remote.command::<Increment>().run(Increment { fail: false });
        nav.goto("/other");
    });
    assert_eq!(frame(&ctx, &mut app), ["increment"]);
    assert_eq!(app.url(), Some("/other"));

    app.state().gate.set(true);
    frame(&ctx, &mut app);
    assert_eq!(app.state().count.get(), 1);
}

#[test]
fn queries_refetch_periodically_while_read() {
    let ctx = egui::Context::default();
    let mut app = Router::new()
        .page(
            "/",
            Page::new(
                |_: &mut Ui, State(state): State<&AppState>, remote: Remote<'_, AppState>| {
                    if state.show.get() {
                        let inbox = remote.query(Inbox);
                        state
                            .seen
                            .borrow_mut()
                            .push(format!("{:?}", inbox.current()));
                    }
                },
            ),
        )
        .with_state_and_history(AppState::default(), MemoryHistory::new("/"));
    let state = app.state().clone();
    let mut frame_at = |time: f64| {
        let input = egui::RawInput {
            time: Some(time),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| app.ui(ui));
        output.textures_delta.clear();
        let runs = core::mem::take(&mut *state.runs.borrow_mut());
        (runs, state.seen.borrow_mut().pop().unwrap_or_default())
    };

    assert_eq!(frame_at(0.0), (vec!["inbox".to_owned()], "Some(0)".into()));
    assert_eq!(frame_at(4.9).0, NOTHING);

    // Due: it reruns, keeping the previous value meanwhile.
    state.gate.set(false);
    state.count.set(3);
    assert_eq!(frame_at(5.0), (vec!["inbox".to_owned()], "Some(0)".into()));
    state.gate.set(true);
    assert_eq!(frame_at(7.0), (vec![], "Some(3)".into()));

    // The next run is due 5 seconds after the last one finished.
    assert_eq!(frame_at(11.9).0, NOTHING);
    assert_eq!(frame_at(12.0).0, ["inbox"]);
}

#[test]
fn queries_can_be_kept_without_readers() {
    let ctx = egui::Context::default();
    let mut app = Router::new()
        .page(
            "/",
            Page::new(
                |_: &mut Ui, State(state): State<&AppState>, remote: Remote<'_, AppState>| {
                    if state.show.get() {
                        remote.query(Kept);
                    }
                },
            ),
        )
        .with_state_and_history(AppState::default(), MemoryHistory::new("/"));
    let state = app.state().clone();
    let mut frame_at = |time: f64| {
        let input = egui::RawInput {
            time: Some(time),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| app.ui(ui));
        output.textures_delta.clear();
        core::mem::take(&mut *state.runs.borrow_mut())
    };

    assert_eq!(frame_at(0.0), ["kept"]);
    state.show.set(false);
    assert_eq!(frame_at(1.0), NOTHING);
    assert_eq!(frame_at(9.9), NOTHING);
    state.show.set(true);
    assert_eq!(frame_at(10.0), NOTHING, "still cached");

    // Dropped 10 seconds after the last read.
    state.show.set(false);
    frame_at(11.0);
    frame_at(20.0);
    state.show.set(true);
    assert_eq!(frame_at(21.0), ["kept"]);
}

#[test]
fn keyed_commands_have_their_own_state() {
    let ctx = egui::Context::default();
    let mut app = app();
    frame(&ctx, &mut app);

    app.state().gate.set(false);
    act(&app, |remote, _| {
        remote
            .command_keyed::<Increment>("a")
            .run(Increment { fail: true });
    });
    frame(&ctx, &mut app);
    act(&app, |remote, _| {
        assert!(remote.command_keyed::<Increment>("a").is_pending());
        assert!(!remote.command_keyed::<Increment>("b").is_pending());
        assert!(!remote.command::<Increment>().is_pending());
    });
    frame(&ctx, &mut app);

    app.state().gate.set(true);
    frame(&ctx, &mut app);
    act(&app, |remote, _| {
        assert!(remote.command_keyed::<Increment>("a").error().is_some());
        assert!(remote.command_keyed::<Increment>("b").error().is_none());
    });
    frame(&ctx, &mut app);
}

#[test]
fn futures_run_like_commands() {
    let ctx = egui::Context::default();
    let mut app = app();
    frame(&ctx, &mut app);

    // From a view: refreshes on success.
    let running: Rc<RefCell<Option<Running<u32>>>> = Rc::default();
    app.state().gate.set(false);
    let slot = Rc::clone(&running);
    let state = app.state().clone();
    act(&app, move |remote, _| {
        *slot.borrow_mut() = Some(remote.run(&["count"], async move {
            state.gate().await;
            state.count.set(7);
            Ok(7)
        }));
    });
    frame(&ctx, &mut app);
    let handle = running.borrow_mut().take().unwrap();
    assert!(handle.is_pending());
    app.state().gate.set(true);
    assert_eq!(frame(&ctx, &mut app), ["load", "count"]);
    assert!(!handle.is_pending());
    assert_eq!(handle.take_result(), Some(Ok(7)));
    assert_eq!(handle.take_result(), None);

    // From outside the views, starting another one from inside: failures refresh nothing.
    let spawner = app.spawner();
    let inner = spawner.clone();
    let outer = spawner.run(&["count"], async move {
        Ok(inner.run(&["count"], async {
            Err::<(), _>(RouteError::new(500, "inner"))
        }))
    });
    assert_eq!(frame(&ctx, &mut app), ["load", "count"]);
    let inner = outer.take_result().unwrap().unwrap();
    frame(&ctx, &mut app);
    assert_eq!(
        inner.take_result(),
        Some(Err(RouteError::new(500, "inner")))
    );
    assert_eq!(frame(&ctx, &mut app), NOTHING);
}
