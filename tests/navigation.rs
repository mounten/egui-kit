//! Navigation features: blocking, `after_navigate`, scroll restoration, matchers,
//! optional parameters and layout resets.

use core::cell::{Cell, RefCell};
use std::rc::Rc;

use egui::Ui;
use egui_kit::extract::{Nav, Path, Scroll, State};
use egui_kit::page::{Layout, Outlet, Page};
use egui_kit::{GotoOptions, MemoryHistory, Rejection, Router, RouterApp};

#[derive(Clone, Default)]
struct AppState {
    log: Rc<RefCell<Vec<String>>>,
    commands: Rc<RefCell<Vec<Command>>>,
    dirty: Rc<Cell<bool>>,
}

type Command = Box<dyn FnOnce(Nav<'_>)>;

impl AppState {
    fn push(&self, entry: impl Into<String>) {
        self.log.borrow_mut().push(entry.into());
    }
}

async fn about(Path(lang): Path<Option<String>>) -> Result<String, Rejection> {
    Ok(lang.unwrap_or_else(|| "en".into()))
}

fn page(name: &'static str) -> Page<AppState> {
    Page::new(move |_: &mut Ui, State(state): State<&AppState>| state.push(name))
}

fn layout(name: &'static str) -> Layout<AppState> {
    Layout::new(
        move |ui: &mut Ui, mut outlet: Outlet<'_>, State(state): State<&AppState>| {
            state.push(name);
            outlet.show(ui);
        },
    )
}

fn app(url: &str) -> RouterApp<AppState> {
    let editor = Page::new(|_: &mut Ui, State(state): State<&AppState>, nav: Nav<'_>| {
        if state.dirty.get() {
            nav.block_navigation();
        }
        let entry = match nav.blocked_navigation() {
            Some(blocked) => format!("editor (blocked: {:?} to {})", blocked.kind, blocked.to),
            None => "editor".to_owned(),
        };
        state.push(entry);
    });
    let long = Page::new(
        |ui: &mut Ui, State(state): State<&AppState>, scroll: Scroll<'_>| {
            scroll.show(ui, |ui| {
                state.push(format!("long@{}", (-ui.max_rect().top()).max(0.0)));
                for i in 0..200 {
                    ui.label(format!("line {i}"));
                }
            });
        },
    );

    Router::new()
        .matcher("int", |s| s.parse::<u32>().is_ok())
        .matcher("lang", |s| ["en", "de"].contains(&s))
        .layout(Layout::new(
            |ui: &mut Ui, mut outlet: Outlet<'_>, State(state): State<&AppState>, nav: Nav<'_>| {
                state.push("root");
                let commands = core::mem::take(&mut *state.commands.borrow_mut());
                for command in commands {
                    command(nav);
                }
                outlet.show(ui);
            },
        ))
        .page("/", page("home"))
        .page("/editor", editor)
        .page("/long", long)
        .page("/authors/{id=int}", page("author-by-id"))
        .page("/authors/{name}", page("author-by-name"))
        .page(
            "/{?lang=lang}/about",
            Page::with_load(
                about,
                |_: &mut Ui, State(state): State<&AppState>, lang: &String| {
                    state.push(format!("about:{lang}"));
                },
            ),
        )
        .nest(
            "/docs",
            Router::new()
                .named("docs")
                .layout(layout("docs-layout"))
                .nest(
                    "/api",
                    Router::new()
                        .layout(layout("api-layout"))
                        .page("/", page("api"))
                        .page("/print", page("print").reset_layouts())
                        .page("/plain", page("plain").reset_layouts_to("docs")),
                ),
        )
        .with_state_and_history(AppState::default(), MemoryHistory::new(url))
        .after_navigate(|navigation, state: &AppState| {
            state.push(format!("after:{:?}:{}", navigation.kind, navigation.to));
        })
}

fn frame(ctx: &egui::Context, app: &mut RouterApp<AppState>) -> Vec<String> {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(400.0, 300.0),
        )),
        ..Default::default()
    };
    let mut output = ctx.run_ui(input, |ui| app.ui(ui));
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
fn blocked_navigation_waits_for_confirmation() {
    let ctx = egui::Context::default();
    let mut app = app("/");
    frame(&ctx, &mut app);
    app.goto("/editor");
    frame(&ctx, &mut app);
    app.state().dirty.set(true);
    frame(&ctx, &mut app);

    // A link away is held back until confirmed.
    app.goto("/form");
    assert_eq!(
        frame(&ctx, &mut app),
        ["root", "editor (blocked: Goto to /form)"]
    );
    act(&ctx, &mut app, |nav| nav.cancel_navigation());
    assert_eq!(frame(&ctx, &mut app), ["root", "editor"]);

    // Back is undone in the history, and redone once confirmed.
    app.back();
    assert_eq!(
        frame(&ctx, &mut app),
        ["root", "editor (blocked: Popstate to /)"]
    );
    assert_eq!(
        frame(&ctx, &mut app),
        ["root", "editor (blocked: Popstate to /)"]
    );
    assert_eq!(app.url(), Some("/editor"));
    act(&ctx, &mut app, |nav| nav.confirm_navigation());
    assert_eq!(frame(&ctx, &mut app), ["after:Popstate:/", "root", "home"]);
}

#[test]
fn after_navigate_runs_for_navigations_only() {
    let ctx = egui::Context::default();
    let mut app = app("/");
    assert_eq!(frame(&ctx, &mut app), ["after:Enter:/", "root", "home"]);
    app.goto("/de/about");
    assert_eq!(
        frame(&ctx, &mut app),
        ["after:Goto:/de/about", "root", "about:de"]
    );
    act(&ctx, &mut app, |nav| nav.invalidate_all());
    assert_eq!(frame(&ctx, &mut app), ["root", "about:de"]);
}

#[test]
fn scroll_position_is_restored() {
    let ctx = egui::Context::default();
    let mut app = app("/long");
    assert_eq!(scroll_offset(&frame(&ctx, &mut app)), 0.0);

    scroll_by(&ctx, &mut app, 500.0);
    // egui smooths wheel scrolling over a few frames.
    for _ in 0..30 {
        frame(&ctx, &mut app);
    }
    let scrolled = scroll_offset(&frame(&ctx, &mut app));
    assert!(scrolled > 400.0, "scrolled to {scrolled}");

    // A new page starts at the top…
    app.goto("/");
    frame(&ctx, &mut app);
    app.goto("/long");
    assert_eq!(scroll_offset(&frame(&ctx, &mut app)), 0.0);

    // …going back restores where the user was…
    app.back();
    frame(&ctx, &mut app);
    app.back();
    assert!((scroll_offset(&frame(&ctx, &mut app)) - scrolled).abs() < 2.0);

    // …and `no_scroll` keeps the position of the page being left (here: reloaded).
    app.goto_with("/long", GotoOptions::default().no_scroll().invalidate_all());
    assert!((scroll_offset(&frame(&ctx, &mut app)) - scrolled).abs() < 2.0);
}

/// The offset logged by the `/long` page.
fn scroll_offset(log: &[String]) -> f32 {
    log.iter()
        .find_map(|entry| entry.strip_prefix("long@"))
        .and_then(|offset| offset.parse().ok())
        .expect("the long page rendered")
}

fn scroll_by(ctx: &egui::Context, app: &mut RouterApp<AppState>, delta: f32) {
    let pos = egui::pos2(200.0, 150.0);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(400.0, 300.0),
        )),
        events: vec![
            egui::Event::PointerMoved(pos),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -delta),
                modifiers: egui::Modifiers::NONE,
                phase: egui::TouchPhase::Move,
            },
        ],
        ..Default::default()
    };
    let mut output = ctx.run_ui(input, |ui| app.ui(ui));
    output.textures_delta.clear();
    app.state().log.borrow_mut().clear();
}

#[test]
fn matchers_and_optional_params() {
    let ctx = egui::Context::default();
    let mut app = app("/authors/7");
    assert_eq!(frame(&ctx, &mut app)[1..], ["root", "author-by-id"]);
    app.goto("/authors/ferris");
    assert_eq!(frame(&ctx, &mut app)[1..], ["root", "author-by-name"]);

    app.goto("/about");
    assert_eq!(frame(&ctx, &mut app)[1..], ["root", "about:en"]);
    app.goto("/de/about");
    assert_eq!(frame(&ctx, &mut app)[1..], ["root", "about:de"]);
    // `fr` isn't a `lang`, so no route matches.
    app.goto("/fr/about");
    assert_eq!(frame(&ctx, &mut app)[1..], ["root"]);
}

#[test]
fn layout_resets() {
    let ctx = egui::Context::default();
    let mut app = app("/docs/api");
    assert_eq!(
        frame(&ctx, &mut app)[1..],
        ["root", "docs-layout", "api-layout", "api"]
    );
    app.goto("/docs/api/print");
    assert_eq!(frame(&ctx, &mut app)[1..], ["root", "print"]);
    app.goto("/docs/api/plain");
    assert_eq!(frame(&ctx, &mut app)[1..], ["root", "docs-layout", "plain"]);
}
