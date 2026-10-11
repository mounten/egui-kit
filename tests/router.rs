//! Drives a `RouterApp` through real (headless) egui frames.

use core::cell::{Cell, RefCell};
use core::future::poll_fn;
use core::task::Poll;
use std::rc::Rc;

use egui::Ui;
use egui_kit::extract::{HistoryState, Nav, Path, Query, State};
use egui_kit::page::{Layout, Outlet, Page};
use egui_kit::{MemoryHistory, Rejection, Request, RouteError, Router, RouterApp, error, redirect};
use serde::Deserialize;

#[derive(Clone, Default)]
struct AppState {
    log: Rc<RefCell<Vec<String>>>,
    logged_in: Rc<Cell<bool>>,
    gate_open: Rc<Cell<bool>>,
    /// Run by the root layout, to call `Nav` methods from a test.
    commands: Rc<RefCell<Vec<Command>>>,
    photo_loads: Rc<Cell<u32>>,
}

type Command = Box<dyn FnOnce(Nav<'_>)>;

impl AppState {
    fn push(&self, entry: impl Into<String>) {
        self.log.borrow_mut().push(entry.into());
    }
}

#[derive(Deserialize)]
struct Search {
    q: Option<String>,
}

async fn search(Query(search): Query<Search>) -> Result<String, Rejection> {
    Ok(search.q.unwrap_or_default())
}

async fn user(Path(id): Path<u32>) -> Result<u32, Rejection> {
    if id == 0 {
        return Err(error(410, "gone"));
    }
    Ok(id)
}

async fn require_login(State(state): State<AppState>) -> Result<(), Rejection> {
    if state.logged_in.get() {
        Ok(())
    } else {
        Err(redirect("/login"))
    }
}

async fn photos(State(state): State<AppState>) -> Result<(), Rejection> {
    state.photo_loads.set(state.photo_loads.get() + 1);
    Ok(())
}

fn history_state(HistoryState(state): HistoryState<&u32>) -> String {
    state.map_or_else(|| "-".into(), |state| state.to_string())
}

async fn slow(State(state): State<AppState>) -> Result<&'static str, Rejection> {
    poll_fn(|_| {
        if state.gate_open.get() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    })
    .await;
    Ok("slow")
}

fn app(state: AppState, url: &str) -> RouterApp<AppState> {
    let users = Router::new()
        .layout(Layout::with_load(
            |Path(id): Path<u32>| async move { Ok::<_, Rejection>(format!("user {id}")) },
            |ui: &mut Ui,
             mut outlet: Outlet<'_>,
             State(state): State<&AppState>,
             title: &String| {
                state.push(format!("users-layout:{title}"));
                outlet.show(ui);
            },
        ))
        .error(
            |_: &mut Ui, State(state): State<&AppState>, err: &RouteError| {
                state.push(format!("users-error:{}", err.status));
            },
        )
        .page(
            "/",
            Page::with_load(
                user,
                |_: &mut Ui, State(state): State<&AppState>, id: &mut u32| {
                    state.push(format!("user:{id}"));
                },
            ),
        );

    Router::new()
        .layout(Layout::new(
            |ui: &mut Ui, mut outlet: Outlet<'_>, State(state): State<&AppState>, nav: Nav<'_>| {
                state.push("root-layout");
                let commands = core::mem::take(&mut *state.commands.borrow_mut());
                for command in commands {
                    command(nav);
                }
                outlet.show(ui);
            },
        ))
        .page(
            "/",
            Page::new(|_: &mut Ui, State(state): State<&AppState>| state.push("home")),
        )
        .page(
            "/login",
            Page::new(|_: &mut Ui, State(state): State<&AppState>| state.push("login")),
        )
        .page(
            "/search",
            Page::with_load(
                search,
                |_: &mut Ui, State(state): State<&AppState>, q: &String| {
                    state.push(format!("search:{q}"));
                },
            ),
        )
        .page(
            "/slow",
            Page::with_load(
                slow,
                |_: &mut Ui, State(state): State<&AppState>, s: &&str| state.push(*s),
            ),
        )
        .page(
            "/photos",
            Page::with_load(
                photos,
                |_: &mut Ui,
                 State(state): State<&AppState>,
                 nav: Nav<'_>,
                 ps: HistoryState<&u32>| {
                    state.push(format!("photos:{}:{}", nav.url(), history_state(ps)));
                },
            ),
        )
        .page(
            "/photos/{id}",
            Page::new(
                |_: &mut Ui,
                 State(state): State<&AppState>,
                 request: Request,
                 ps: HistoryState<&u32>| {
                    state.push(format!(
                        "photo:{}:{}",
                        request.param("id").expect("route has an id"),
                        history_state(ps)
                    ));
                },
            ),
        )
        .nest("/users/{id}", users)
        .nest(
            "/admin",
            Router::new().hook(require_login).page(
                "/",
                Page::new(|_: &mut Ui, State(state): State<&AppState>| state.push("admin")),
            ),
        )
        .with_state_and_history(state, MemoryHistory::new(url))
}

/// Runs one frame and returns what was rendered.
fn frame(ctx: &egui::Context, app: &mut RouterApp<AppState>) -> Vec<String> {
    let mut output = ctx.run_ui(Default::default(), |ui| app.ui(ui));
    output.textures_delta.clear();
    core::mem::take(&mut *app.state().log.borrow_mut())
}

#[test]
fn renders_initial_page_with_query() {
    let ctx = egui::Context::default();
    let mut app = app(AppState::default(), "/search?q=egui");
    assert_eq!(frame(&ctx, &mut app), ["root-layout", "search:egui"]);
}

#[test]
fn nested_params_layout_load_and_error_boundary() {
    let ctx = egui::Context::default();
    let mut app = app(AppState::default(), "/users/7");
    assert_eq!(
        frame(&ctx, &mut app),
        ["root-layout", "users-layout:user 7", "user:7"]
    );

    // The page load fails: the nested error page renders inside the nested layout.
    app.goto("/users/0");
    assert_eq!(
        frame(&ctx, &mut app),
        ["root-layout", "users-layout:user 0", "users-error:410"]
    );

    // Unknown path below the nested router: its 404, inside its layout.
    app.goto("/users/7/nope");
    assert_eq!(
        frame(&ctx, &mut app),
        ["root-layout", "users-layout:user 7", "users-error:404"]
    );

    // Typed param fails in the nested layout's load: falls back to the root's (default) error page.
    app.goto("/users/abc");
    assert_eq!(frame(&ctx, &mut app), ["root-layout"]);
    assert_eq!(app.request().unwrap().path(), "/users/abc");
}

#[test]
fn hook_redirects_and_history() {
    let ctx = egui::Context::default();
    let state = AppState::default();
    let mut app = app(state.clone(), "/");
    assert_eq!(frame(&ctx, &mut app), ["root-layout", "home"]);

    app.goto("/admin");
    assert_eq!(frame(&ctx, &mut app), ["root-layout", "login"]);
    assert_eq!(app.request().unwrap().path(), "/login");

    state.logged_in.set(true);
    app.goto("/admin");
    assert_eq!(frame(&ctx, &mut app), ["root-layout", "admin"]);

    app.back();
    assert_eq!(frame(&ctx, &mut app), ["root-layout", "login"]);
    app.back();
    assert_eq!(frame(&ctx, &mut app), ["root-layout", "home"]);
    app.forward();
    assert_eq!(frame(&ctx, &mut app), ["root-layout", "login"]);
}

#[test]
fn keeps_previous_page_while_loading() {
    let ctx = egui::Context::default();
    let state = AppState::default();
    let mut app = app(state.clone(), "/");
    frame(&ctx, &mut app);

    app.goto("/slow");
    assert!(app.is_loading());
    assert_eq!(frame(&ctx, &mut app), ["root-layout", "home"]);

    state.gate_open.set(true);
    assert_eq!(frame(&ctx, &mut app), ["root-layout", "slow"]);
    assert!(!app.is_loading());
}

/// Calls `command` with a `Nav` during a frame; its effects show from the next frame on.
fn act(
    ctx: &egui::Context,
    app: &mut RouterApp<AppState>,
    command: impl FnOnce(Nav<'_>) + 'static,
) {
    app.state().commands.borrow_mut().push(Box::new(command));
    frame(ctx, app);
}

#[test]
fn shallow_routing() {
    let ctx = egui::Context::default();
    let state = AppState::default();
    let mut app = app(state.clone(), "/photos");
    let loads = || state.photo_loads.get();
    assert_eq!(frame(&ctx, &mut app), ["root-layout", "photos:/photos:-"]);
    assert_eq!(loads(), 1);

    // New entry with state, same page, no load.
    act(&ctx, &mut app, |nav| nav.push_state("/photos/3", 3u32));
    assert_eq!(frame(&ctx, &mut app), ["root-layout", "photos:/photos/3:3"]);
    act(&ctx, &mut app, |nav| nav.replace_state("", 4u32));
    assert_eq!(frame(&ctx, &mut app), ["root-layout", "photos:/photos/3:4"]);
    assert_eq!(loads(), 1);

    // Back and forward between entries of the same page only swap the state.
    app.back();
    assert_eq!(frame(&ctx, &mut app), ["root-layout", "photos:/photos:-"]);
    app.forward();
    assert_eq!(frame(&ctx, &mut app), ["root-layout", "photos:/photos/3:4"]);
    assert_eq!(loads(), 1);

    // Invalidating reloads the data but keeps the url, the state and the page instance.
    act(&ctx, &mut app, |nav| nav.invalidate_all());
    assert_eq!(frame(&ctx, &mut app), ["root-layout", "photos:/photos/3:4"]);
    assert_eq!(loads(), 2);
    app.back();
    assert_eq!(frame(&ctx, &mut app), ["root-layout", "photos:/photos:-"]);
    assert_eq!(loads(), 2);
    app.forward();
    frame(&ctx, &mut app);

    // A full navigation starts without state...
    app.goto("/");
    assert_eq!(frame(&ctx, &mut app), ["root-layout", "home"]);

    // ...and coming back to a shallow entry of an earlier page loads its url for real,
    // restoring the entry's state.
    app.back();
    assert_eq!(frame(&ctx, &mut app), ["root-layout", "photo:3:4"]);
    app.back();
    assert_eq!(frame(&ctx, &mut app), ["root-layout", "photos:/photos:-"]);
    assert_eq!(loads(), 3);

    act(&ctx, &mut app, |nav| nav.push_state("", 7u32));
    assert_eq!(frame(&ctx, &mut app), ["root-layout", "photos:/photos:7"]);
    act(&ctx, &mut app, |nav| nav.clear_history_state());
    assert_eq!(frame(&ctx, &mut app), ["root-layout", "photos:/photos:-"]);
    assert_eq!(app.url(), Some("/photos"));
}

#[test]
fn sixteen_extractors() {
    // Loads own the state, views borrow it.
    type S = State<AppState>;
    type V<'a> = State<&'a AppState>;

    #[expect(clippy::too_many_arguments)]
    async fn load(
        Path(id): Path<u32>,
        _: Query<Search>,
        _: Request,
        _: S,
        _: S,
        _: S,
        _: S,
        _: S,
        _: S,
        _: S,
        _: S,
        _: S,
        _: S,
        _: S,
        _: S,
        State(state): S,
    ) -> Result<u32, Rejection> {
        state.push(format!("load:{id}"));
        Ok(id)
    }

    #[expect(clippy::too_many_arguments)]
    fn view(
        _: &mut Ui,
        id: &u32,
        _: Nav<'_>,
        _: Request,
        _: HistoryState<&u32>,
        _: egui_kit::Navigation,
        _: egui_kit::extract::Scroll<'_>,
        _: V<'_>,
        _: V<'_>,
        _: V<'_>,
        _: V<'_>,
        _: V<'_>,
        _: V<'_>,
        _: V<'_>,
        _: V<'_>,
        _: V<'_>,
        State(state): V<'_>,
    ) {
        state.push(format!("view:{id}"));
    }

    #[expect(clippy::too_many_arguments)]
    fn layout(
        ui: &mut Ui,
        mut outlet: Outlet<'_>,
        _: &(),
        _: V<'_>,
        _: V<'_>,
        _: V<'_>,
        _: V<'_>,
        _: V<'_>,
        _: V<'_>,
        _: V<'_>,
        _: V<'_>,
        _: V<'_>,
        _: V<'_>,
        _: V<'_>,
        _: V<'_>,
        _: V<'_>,
        _: V<'_>,
        State(state): V<'_>,
    ) {
        state.push("layout");
        outlet.show(ui);
    }

    let state = AppState::default();
    let mut app = Router::new()
        .layout(Layout::new(layout))
        .page("/{id}", Page::with_load(load, view))
        .with_state_and_history(state, MemoryHistory::new("/7"));
    let ctx = egui::Context::default();
    assert_eq!(frame(&ctx, &mut app), ["load:7", "layout", "view:7"]);
}
