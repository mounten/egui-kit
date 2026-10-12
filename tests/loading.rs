//! Which loads run, when: dependency tracking, invalidation, parent data, concurrency, preloading.

use core::cell::{Cell, RefCell};
use core::future::poll_fn;
use core::task::Poll;
use std::rc::Rc;

use egui::Ui;
use egui_kit::extract::{Nav, Parent, Path, Query, State};
use egui_kit::page::{Layout, Outlet, Page};
use egui_kit::{MemoryHistory, Rejection, Router, RouterApp};
use serde::Deserialize;

#[derive(Clone, Default)]
struct AppState {
    loads: Rc<RefCell<Vec<String>>>,
    rendered: Rc<RefCell<Vec<String>>>,
    commands: Rc<RefCell<Vec<Command>>>,
    counter: Rc<Cell<u32>>,
    gate_open: Rc<Cell<bool>>,
}

type Command = Box<dyn FnOnce(Nav<'_>)>;

impl AppState {
    fn loaded(&self, entry: impl Into<String>) {
        self.loads.borrow_mut().push(entry.into());
    }
}

#[derive(Deserialize)]
struct Search {
    q: String,
}

#[derive(Clone)]
struct Counter(u32);

async fn root(State(state): State<AppState>) -> Result<(), Rejection> {
    state.loaded("root");
    Ok(())
}

async fn users_layout(
    Path(id): Path<u32>,
    State(state): State<AppState>,
) -> Result<u32, Rejection> {
    state.loaded(format!("users-layout:{id}"));
    Ok(id)
}

async fn user_post(
    Path((id, post)): Path<(u32, u32)>,
    State(state): State<AppState>,
) -> Result<(), Rejection> {
    state.loaded(format!("post:{id}/{post}"));
    Ok(())
}

async fn search(
    Query(search): Query<Search>,
    State(state): State<AppState>,
) -> Result<String, Rejection> {
    state.loaded(format!("search:{}", search.q));
    Ok(search.q)
}

async fn feed(State(state): State<AppState>) -> Result<(), Rejection> {
    state.loaded("feed");
    Ok(())
}

async fn counter_layout(State(state): State<AppState>) -> Result<Counter, Rejection> {
    state.counter.set(state.counter.get() + 1);
    state.loaded(format!("counter-layout:{}", state.counter.get()));
    Ok(Counter(state.counter.get()))
}

async fn with_parent(parent: Parent, State(state): State<AppState>) -> Result<u32, Rejection> {
    let Counter(count) = parent.get::<Counter>().await?;
    state.loaded(format!("with-parent:{count}"));
    Ok(count)
}

async fn independent(State(state): State<AppState>) -> Result<(), Rejection> {
    state.loaded("independent");
    Ok(())
}

async fn slow_layout(State(state): State<AppState>) -> Result<(), Rejection> {
    poll_fn(|_| {
        if state.gate_open.get() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    })
    .await;
    state.loaded("slow-layout");
    Ok(())
}

fn render(name: &'static str) -> Page<AppState> {
    Page::new(move |_: &mut Ui, State(state): State<&AppState>| {
        state.rendered.borrow_mut().push(name.to_owned());
    })
}

fn outlet(ui: &mut Ui, mut outlet: Outlet<'_>) {
    outlet.show(ui);
}

fn app(url: &str) -> RouterApp<AppState> {
    let users = Router::new()
        .layout(Layout::with_load(users_layout, outlet))
        .page("/", render("user"))
        .page("/posts/{post}", Page::with_load(user_post, |_: &mut Ui| {}));
    let counted = Router::new()
        .layout(Layout::with_load(counter_layout, outlet).depends("counter"))
        .page(
            "/",
            Page::with_load(
                with_parent,
                |_: &mut Ui, State(state): State<&AppState>, count: &u32| {
                    state
                        .rendered
                        .borrow_mut()
                        .push(format!("with-parent:{count}"));
                },
            ),
        )
        .page(
            "/independent",
            Page::with_load(independent, |_: &mut Ui| {}),
        );
    let slow = Router::new()
        .layout(Layout::with_load(slow_layout, outlet))
        .page("/", Page::with_load(independent, |_: &mut Ui| {}));

    Router::new()
        .layout(Layout::with_load(
            root,
            |ui: &mut Ui, mut outlet: Outlet<'_>, State(state): State<&AppState>, nav: Nav<'_>| {
                let commands = core::mem::take(&mut *state.commands.borrow_mut());
                for command in commands {
                    command(nav);
                }
                outlet.show(ui);
            },
        ))
        .page("/", render("home"))
        .page(
            "/search",
            Page::with_load(
                search,
                |_: &mut Ui, State(state): State<&AppState>, q: &String| {
                    state.rendered.borrow_mut().push(format!("search:{q}"));
                },
            ),
        )
        .page(
            "/feed",
            Page::with_load(feed, |_: &mut Ui| {}).depends("feed"),
        )
        .nest("/users/{id}", users)
        .nest("/counted", counted)
        .nest("/slow", slow)
        .with_state_and_history(AppState::default(), MemoryHistory::new(url))
}

/// Runs a frame; returns the loads that ran and the views that rendered.
fn frame(ctx: &egui::Context, app: &mut RouterApp<AppState>) -> (Vec<String>, Vec<String>) {
    let mut output = ctx.run_ui(Default::default(), |ui| app.ui(ui));
    output.textures_delta.clear();
    let state = app.state();
    let loads = core::mem::take(&mut *state.loads.borrow_mut());
    let rendered = core::mem::take(&mut *state.rendered.borrow_mut());
    (loads, rendered)
}

fn loads(ctx: &egui::Context, app: &mut RouterApp<AppState>) -> Vec<String> {
    frame(ctx, app).0
}

fn act(
    ctx: &egui::Context,
    app: &mut RouterApp<AppState>,
    command: impl FnOnce(Nav<'_>) + 'static,
) -> Vec<String> {
    app.state().commands.borrow_mut().push(Box::new(command));
    let mut loaded = loads(ctx, app);
    loaded.extend(loads(ctx, app));
    loaded
}

#[test]
fn layouts_rerun_only_when_what_they_read_changes() {
    let ctx = egui::Context::default();
    let mut app = app("/users/1");
    assert_eq!(loads(&ctx, &mut app), ["root", "users-layout:1"]);

    // The layout only sees (and depends on) `{id}`; the page sees both parameters.
    app.goto("/users/1/posts/5");
    assert_eq!(loads(&ctx, &mut app), ["post:1/5"]);
    app.goto("/users/1/posts/6");
    assert_eq!(loads(&ctx, &mut app), ["post:1/6"]);
    app.goto("/users/2/posts/6");
    assert_eq!(loads(&ctx, &mut app), ["users-layout:2", "post:2/6"]);

    // Query changes rerun loads that read the query, and only those.
    app.goto("/search?q=a");
    assert_eq!(loads(&ctx, &mut app), ["search:a"]);
    app.goto("/search?q=b");
    assert_eq!(loads(&ctx, &mut app), ["search:b"]);
    app.goto("/search?q=b&unrelated=1");
    assert_eq!(loads(&ctx, &mut app), ["search:b"]);
}

#[test]
fn invalidate_by_key_and_all() {
    let ctx = egui::Context::default();
    let mut app = app("/feed");
    assert_eq!(loads(&ctx, &mut app), ["root", "feed"]);

    assert_eq!(act(&ctx, &mut app, |nav| nav.invalidate("feed")), ["feed"]);
    assert_eq!(
        act(&ctx, &mut app, |nav| nav.invalidate("unrelated")),
        Vec::<String>::new()
    );
    assert_eq!(
        act(&ctx, &mut app, |nav| nav.invalidate_all()),
        ["root", "feed"]
    );

    app.invalidate("feed");
    assert_eq!(loads(&ctx, &mut app), ["feed"]);
}

#[test]
fn parent_data_and_reruns() {
    let ctx = egui::Context::default();
    let mut app = app("/counted");
    let (loaded, rendered) = frame(&ctx, &mut app);
    assert_eq!(loaded, ["root", "counter-layout:1", "with-parent:1"]);
    assert_eq!(rendered, ["with-parent:1"]);

    // The page used `Parent`, so it reruns when the layout reloads…
    assert_eq!(
        act(&ctx, &mut app, |nav| nav.invalidate("counter")),
        ["counter-layout:2", "with-parent:2"]
    );
    // …but a page that didn't ask for parent data doesn't.
    app.goto("/counted/independent");
    assert_eq!(loads(&ctx, &mut app), ["independent"]);
    assert_eq!(
        act(&ctx, &mut app, |nav| nav.invalidate("counter")),
        ["counter-layout:3"]
    );

    // Invalidating and navigating in the same frame: the navigation reruns what was invalidated.
    let loaded = act(&ctx, &mut app, |nav| {
        nav.invalidate("counter");
        nav.goto("/counted");
    });
    assert_eq!(loaded, ["counter-layout:4", "with-parent:4"]);
}

#[test]
fn loads_run_concurrently() {
    let ctx = egui::Context::default();
    let mut app = app("/");
    frame(&ctx, &mut app);

    // The page's load doesn't wait for the slow layout's.
    app.goto("/slow");
    assert_eq!(loads(&ctx, &mut app), ["independent"]);
    assert!(app.is_loading());
    app.state().gate_open.set(true);
    assert_eq!(loads(&ctx, &mut app), ["slow-layout"]);
    assert_eq!(app.request().unwrap().path(), "/slow");
}

#[test]
fn going_to_the_loading_url_keeps_the_navigation() {
    let ctx = egui::Context::default();
    let mut app = app("/");
    frame(&ctx, &mut app);

    // A view asking for the same navigation every frame, as when a query's value triggers it.
    app.goto("/slow");
    assert_eq!(loads(&ctx, &mut app), ["independent"]);
    for _ in 0..3 {
        assert_eq!(
            act(&ctx, &mut app, |nav| nav.goto("/slow")),
            Vec::<String>::new()
        );
    }
    app.state().gate_open.set(true);
    assert_eq!(loads(&ctx, &mut app), ["slow-layout"]);
    assert_eq!(app.url(), Some("/slow"));

    // Other options start over.
    app.state().gate_open.set(false);
    app.goto("/");
    frame(&ctx, &mut app);
    app.goto("/slow");
    assert_eq!(loads(&ctx, &mut app), ["independent"]);
    assert_eq!(
        act(&ctx, &mut app, |nav| nav.replace("/slow")),
        ["independent"]
    );
}

#[test]
fn preloaded_routes_are_used_by_the_navigation() {
    let ctx = egui::Context::default();
    let mut app = app("/");
    frame(&ctx, &mut app);

    assert_eq!(
        act(&ctx, &mut app, |nav| nav.preload("/search?q=pre")),
        ["search:pre"]
    );
    app.goto("/search?q=pre");
    let (loaded, rendered) = frame(&ctx, &mut app);
    assert_eq!(*loaded, Vec::<String>::new());
    assert_eq!(rendered, ["search:pre"]);

    // Preloads are dropped once a navigation finished: they were planned against another page.
    assert_eq!(act(&ctx, &mut app, |nav| nav.preload("/feed")), ["feed"]);
    app.goto("/");
    frame(&ctx, &mut app);
    app.goto("/feed");
    assert_eq!(loads(&ctx, &mut app), ["feed"]);
}
