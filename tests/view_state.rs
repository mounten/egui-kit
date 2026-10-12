//! State owned by views: `PageState`, `Snapshot` and `PersistentState`.

use core::cell::{Cell, RefCell};
use std::rc::Rc;

use egui::Ui;
use egui_kit::extract::{
    Nav, PageState, PageStateFrom, PersistentState, Snapshot, SnapshotFrom, State,
};
use egui_kit::page::{Layout, Outlet, Page};
use egui_kit::{GotoOptions, MemoryHistory, Rejection, Router, RouterApp};
use serde::{Deserialize, Serialize};

#[derive(Clone, Default)]
struct AppState {
    log: Rc<RefCell<Vec<String>>>,
    commands: Rc<RefCell<Vec<Command>>>,
    /// Text the views "type" into their state on the next frame.
    typed: Rc<RefCell<Option<String>>>,
    layout_typed: Rc<RefCell<Option<String>>>,
    loads: Rc<Cell<u32>>,
}

type Command = Box<dyn FnOnce(Nav<'_>)>;

impl AppState {
    fn push(&self, entry: impl Into<String>) {
        self.log.borrow_mut().push(entry.into());
    }
}

#[derive(Default)]
struct Draft(String);

#[derive(Default)]
struct Search(String);

#[derive(Clone, Default, Serialize, Deserialize)]
struct Visits(u32);

async fn count_load(State(state): State<AppState>) -> Result<u32, Rejection> {
    state.loads.set(state.loads.get() + 1);
    Ok(state.loads.get())
}

fn form(
    _: &mut Ui,
    load: &u32,
    mut draft: PageState<'_, Draft>,
    mut search: Snapshot<'_, Search>,
    State(state): State<&AppState>,
) {
    if let Some(text) = state.typed.take() {
        draft.0 = text.clone();
        search.0 = text;
    }
    state.push(format!(
        "form#{load}:page={}:snapshot={}",
        draft.0, search.0
    ));
}

fn blog_layout(
    ui: &mut Ui,
    mut outlet: Outlet<'_>,
    mut draft: PageState<'_, Draft>,
    mut search: Snapshot<'_, Search>,
    State(state): State<&AppState>,
) {
    if let Some(text) = state.layout_typed.take() {
        draft.0 = text.clone();
        search.0 = text;
    }
    state.push(format!("layout:page={}:snapshot={}", draft.0, search.0));
    outlet.show(ui);
}

/// The load it was made from.
struct Initial(u32);

impl From<&u32> for Initial {
    fn from(load: &u32) -> Self {
        Self(*load)
    }
}

/// Takes the data first, so the state is made before it's extracted.
#[expect(
    clippy::needless_pass_by_value,
    reason = "extractors are passed by value"
)]
fn from_data(
    _: &mut Ui,
    load: &mut u32,
    page: PageStateFrom<'_, Initial>,
    snapshot: SnapshotFrom<'_, Initial>,
    State(state): State<&AppState>,
) {
    state.push(format!(
        "load#{load}:page={}:snapshot={}",
        page.0, snapshot.0
    ));
}

fn page(name: &'static str) -> Page<AppState> {
    Page::new(move |_: &mut Ui, State(state): State<&AppState>| state.push(name))
}

fn app(ctx_url: &str) -> RouterApp<AppState> {
    Router::new()
        .layout(Layout::new(
            |ui: &mut Ui, mut outlet: Outlet<'_>, State(state): State<&AppState>, nav: Nav<'_>| {
                let commands = core::mem::take(&mut *state.commands.borrow_mut());
                for command in commands {
                    command(nav);
                }
                outlet.show(ui);
            },
        ))
        .page("/", page("home"))
        .page("/form", Page::with_load(count_load, form))
        .page("/from-data", Page::with_load(count_load, from_data))
        .page(
            "/visits",
            Page::new(
                |_: &mut Ui,
                 mut visits: PersistentState<'_, Visits>,
                 State(state): State<&AppState>| {
                    visits.0 += 1;
                    state.push(format!("visits:{}", visits.0));
                },
            ),
        )
        .nest(
            "/blog",
            Router::new()
                .layout(Layout::new(blog_layout))
                .page("/a", page("a"))
                .page("/b", page("b")),
        )
        .with_state_and_history(AppState::default(), MemoryHistory::new(ctx_url))
}

fn frame(ctx: &egui::Context, app: &mut RouterApp<AppState>) -> Vec<String> {
    let mut output = ctx.run_ui(Default::default(), |ui| app.ui(ui));
    output.textures_delta.clear();
    core::mem::take(&mut *app.state().log.borrow_mut())
}

fn act(
    ctx: &egui::Context,
    app: &mut RouterApp<AppState>,
    command: impl FnOnce(Nav<'_>) + 'static,
) {
    app.state().commands.borrow_mut().push(Box::new(command));
    frame(ctx, app);
}

#[test]
fn page_state_lives_with_the_page_and_snapshots_come_back() {
    let ctx = egui::Context::default();
    let mut app = app("/form");
    frame(&ctx, &mut app);
    *app.state().typed.borrow_mut() = Some("x".into());
    assert_eq!(frame(&ctx, &mut app), ["form#1:page=x:snapshot=x"]);

    // An invalidation reloads the data but keeps the page: both stay.
    act(&ctx, &mut app, |nav| nav.invalidate_all());
    assert_eq!(frame(&ctx, &mut app), ["form#2:page=x:snapshot=x"]);

    // So does shallow routing.
    act(&ctx, &mut app, |nav| nav.push_state("", 1u8));
    assert_eq!(frame(&ctx, &mut app), ["form#2:page=x:snapshot=x"]);

    // Leaving and coming back with back/forward: a new page state, the snapshot is restored.
    app.goto("/");
    frame(&ctx, &mut app);
    app.back();
    assert_eq!(frame(&ctx, &mut app), ["form#3:page=:snapshot=x"]);

    // ...also for the other entry of the same visit, without reloading.
    app.back();
    assert_eq!(frame(&ctx, &mut app), ["form#3:page=:snapshot=x"]);

    // A new navigation to the page starts over.
    app.goto("/");
    frame(&ctx, &mut app);
    app.goto("/form");
    assert_eq!(frame(&ctx, &mut app), ["form#4:page=:snapshot="]);
}

#[test]
fn a_link_to_the_current_page_keeps_it() {
    let ctx = egui::Context::default();
    let mut app = app("/form");
    frame(&ctx, &mut app);
    *app.state().typed.borrow_mut() = Some("x".into());
    frame(&ctx, &mut app);

    // Going where we already are does nothing...
    act(&ctx, &mut app, |nav| nav.goto("/form"));
    assert_eq!(frame(&ctx, &mut app), ["form#1:page=x:snapshot=x"]);

    // ...unless it asks for a reload.
    act(&ctx, &mut app, |nav| {
        nav.goto_with("/form", GotoOptions::default().invalidate_all());
    });
    assert_eq!(frame(&ctx, &mut app), ["form#2:page=:snapshot="]);
}

#[test]
fn layout_state_lives_as_long_as_the_layout() {
    let ctx = egui::Context::default();
    let mut app = app("/blog/a");
    assert_eq!(frame(&ctx, &mut app), ["layout:page=:snapshot=", "a"]);
    *app.state().layout_typed.borrow_mut() = Some("t".into());
    frame(&ctx, &mut app);

    // Navigating below the layout keeps it, and its state.
    app.goto("/blog/b");
    assert_eq!(frame(&ctx, &mut app), ["layout:page=t:snapshot=t", "b"]);

    // Leaving the layout drops its page state; back/forward restores its snapshot.
    app.goto("/");
    frame(&ctx, &mut app);
    app.back();
    assert_eq!(frame(&ctx, &mut app), ["layout:page=:snapshot=t", "b"]);
}

#[test]
fn persistent_state_outside_of_views() {
    let ctx = egui::Context::default();
    PersistentState::<Visits>::set(&ctx, Visits(41));
    let mut app = app("/visits");
    assert_eq!(frame(&ctx, &mut app), ["visits:42"]);
    assert_eq!(PersistentState::<Visits>::get(&ctx).0, 42);
}

#[test]
fn persistent_state_outlives_the_app() {
    let ctx = egui::Context::default();
    let mut app = app("/visits");
    assert_eq!(frame(&ctx, &mut app), ["visits:1"]);
    assert_eq!(frame(&ctx, &mut app), ["visits:2"]);

    // Kept in egui's persisted memory, which eframe saves: a new app sees it.
    let mut app = self::app("/visits");
    assert_eq!(frame(&ctx, &mut app), ["visits:3"]);
}

#[test]
#[should_panic(expected = "can only extract")]
fn a_view_extracts_each_page_state_type_once() {
    let ctx = egui::Context::default();
    let mut app = Router::new()
        .page(
            "/",
            Page::new(|_: &mut Ui, _: PageState<'_, Draft>, _: PageState<'_, Draft>| {}),
        )
        .with_state_and_history((), MemoryHistory::new("/"));
    frame_unit(&ctx, &mut app);
}

fn frame_unit(ctx: &egui::Context, app: &mut RouterApp<()>) {
    let mut output = ctx.run_ui(Default::default(), |ui| app.ui(ui));
    output.textures_delta.clear();
}

#[test]
fn view_state_can_start_from_the_data() {
    let ctx = egui::Context::default();
    let mut app = app("/from-data");
    assert_eq!(frame(&ctx, &mut app), ["load#1:page=1:snapshot=1"]);

    // Made once: reloads keep it.
    app.invalidate_all();
    frame(&ctx, &mut app);
    assert_eq!(frame(&ctx, &mut app), ["load#2:page=1:snapshot=1"]);

    // A new visit starts over, and back/forward brings the snapshot back.
    app.goto("/");
    frame(&ctx, &mut app);
    app.goto("/from-data");
    assert_eq!(frame(&ctx, &mut app), ["load#3:page=3:snapshot=3"]);
    app.back();
    frame(&ctx, &mut app);
    app.back();
    assert_eq!(frame(&ctx, &mut app), ["load#4:page=4:snapshot=1"]);
}
