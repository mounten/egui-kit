//! View transitions, driven through headless egui frames with controlled time.

use core::cell::RefCell;
use std::rc::Rc;

use egui::Ui;
use egui_kit::extract::{Nav, State};
use egui_kit::page::{Layout, Outlet, Page};
use egui_kit::{Layer, MemoryHistory, Navigation, Router, RouterApp, ViewTransition};

#[derive(Clone, Default)]
struct AppState {
    log: Rc<RefCell<Vec<String>>>,
    commands: Rc<RefCell<Vec<Command>>>,
}

type Command = Box<dyn FnOnce(Nav<'_>)>;

impl AppState {
    fn push(&self, entry: impl Into<String>) {
        self.log.borrow_mut().push(entry.into());
    }
}

fn page(name: &'static str) -> Page<AppState> {
    Page::new(
        move |ui: &mut Ui, State(state): State<&AppState>, nav: Nav<'_>, navigation: Navigation| {
            assert_eq!(
                ui.is_enabled(),
                !nav.is_outgoing(),
                "only the outgoing page is disabled"
            );
            let outgoing = if nav.is_outgoing() { "-out" } else { "" };
            state.push(format!(
                "{name}{outgoing}({:?},{:?})",
                navigation.kind, navigation.direction
            ));
        },
    )
}

fn app() -> RouterApp<AppState> {
    let users = Router::new()
        .layout(Layout::new(
            |ui: &mut Ui, outlet: Outlet<'_>, State(state): State<&AppState>| {
                state.push("users-layout");
                ViewTransition::fade().duration(1.0).show(ui, outlet);
            },
        ))
        .page("/{id}", page("user"));
    let plain = Router::new()
        .layout(Layout::new(
            |ui: &mut Ui, mut outlet: Outlet<'_>, State(state): State<&AppState>| {
                state.push("plain-layout");
                outlet.show(ui);
            },
        ))
        .page("/{id}", page("plain"));

    Router::new()
        .layout(Layout::new(
            |ui: &mut Ui, outlet: Outlet<'_>, State(state): State<&AppState>, nav: Nav<'_>| {
                state.push("root");
                let commands = core::mem::take(&mut *state.commands.borrow_mut());
                for command in commands {
                    command(nav);
                }
                ViewTransition::slide().duration(1.0).show(ui, outlet);
            },
        ))
        .page("/", page("home"))
        .page("/about", page("about"))
        .nest("/users", users)
        .nest("/plain", plain)
        .with_state_and_history(AppState::default(), MemoryHistory::new("/"))
}

fn frame(ctx: &egui::Context, app: &mut RouterApp<AppState>, time: f64) -> Vec<String> {
    let input = egui::RawInput {
        time: Some(time),
        ..Default::default()
    };
    let mut output = ctx.run_ui(input, |ui| app.ui(ui));
    output.textures_delta.clear();
    core::mem::take(&mut *app.state().log.borrow_mut())
}

#[test]
fn crossfades_old_and_new_page_in_the_root_layout() {
    let ctx = egui::Context::default();
    let mut app = app();
    assert_eq!(frame(&ctx, &mut app, 0.0), ["root", "home(Enter,None)"]);

    app.goto("/about");
    // Both pages render (old first, below the new one), inside a single root layout.
    assert_eq!(
        frame(&ctx, &mut app, 1.0),
        ["root", "home-out(Enter,None)", "about(Goto,Forward)"]
    );
    assert_eq!(
        frame(&ctx, &mut app, 1.5),
        ["root", "home-out(Enter,None)", "about(Goto,Forward)"]
    );
    // Done after the duration: only the new page is left.
    assert_eq!(frame(&ctx, &mut app, 2.1), ["root", "about(Goto,Forward)"]);
    assert_eq!(frame(&ctx, &mut app, 2.2), ["root", "about(Goto,Forward)"]);

    app.back();
    assert_eq!(
        frame(&ctx, &mut app, 3.0),
        ["root", "about-out(Goto,Forward)", "home(Popstate,Back)"]
    );
    frame(&ctx, &mut app, 4.1);
    app.forward();
    assert_eq!(
        frame(&ctx, &mut app, 5.0),
        ["root", "home-out(Popstate,Back)", "about(Popstate,Forward)"]
    );
}

#[test]
fn innermost_shared_layout_runs_the_transition() {
    let ctx = egui::Context::default();
    let mut app = app();
    app.goto("/users/1");
    frame(&ctx, &mut app, 0.0);
    frame(&ctx, &mut app, 10.0);

    // Only the content below the users layout animates; both layouts render once.
    app.goto("/users/2");
    assert_eq!(
        frame(&ctx, &mut app, 11.0),
        [
            "root",
            "users-layout",
            "user-out(Enter,None)",
            "user(Goto,Forward)"
        ]
    );

    // Between sections, the root layout animates, and the old section brings its layout along.
    app.goto("/about");
    frame(&ctx, &mut app, 20.0);
    app.goto("/users/3");
    assert_eq!(
        frame(&ctx, &mut app, 21.0),
        [
            "root",
            "about-out(Goto,Forward)",
            "users-layout",
            "user(Goto,Forward)"
        ]
    );
}

#[test]
fn falls_back_to_an_outer_layout_with_a_transition() {
    let ctx = egui::Context::default();
    let mut app = app();
    app.goto("/plain/1");
    frame(&ctx, &mut app, 0.0);
    frame(&ctx, &mut app, 10.0);

    // The innermost shared layout doesn't animate, so the root (which does) takes over.
    app.goto("/plain/2");
    assert_eq!(
        frame(&ctx, &mut app, 11.0),
        [
            "root",
            "plain-layout",
            "plain-out(Enter,None)",
            "plain-layout",
            "plain(Goto,Forward)"
        ]
    );
}

#[test]
fn invalidate_and_shallow_routing_do_not_transition() {
    let ctx = egui::Context::default();
    let mut app = app();
    frame(&ctx, &mut app, 0.0);

    app.state()
        .commands
        .borrow_mut()
        .push(Box::new(|nav| nav.invalidate_all()));
    frame(&ctx, &mut app, 1.0);
    assert_eq!(
        frame(&ctx, &mut app, 1.1),
        ["root", "home(Invalidate,None)"]
    );

    app.state()
        .commands
        .borrow_mut()
        .push(Box::new(|nav| nav.push_state("/#modal", 1u8)));
    frame(&ctx, &mut app, 2.0);
    assert_eq!(
        frame(&ctx, &mut app, 2.1),
        ["root", "home(Invalidate,None)"]
    );
}

#[test]
fn navigation_direction_with_history() {
    let ctx = egui::Context::default();
    let mut app = app();
    frame(&ctx, &mut app, 0.0);
    app.goto("/about");
    frame(&ctx, &mut app, 1.0);
    app.goto("/users/1");
    frame(&ctx, &mut app, 2.0);

    app.back();
    app.back();
    frame(&ctx, &mut app, 3.0);
    let last = frame(&ctx, &mut app, 10.0);
    assert_eq!(last, ["root", "home(Popstate,Back)"]);
    assert_eq!(app.request().unwrap().path(), "/");
}

#[test]
fn the_outgoing_page_keeps_its_opacity() {
    let ctx = egui::Context::default();
    let opacity = Rc::new(RefCell::new(Vec::new()));
    let seen = Rc::clone(&opacity);
    let mut app = Router::new()
        .layout(Layout::new(|ui: &mut Ui, outlet: Outlet<'_>| {
            ViewTransition::fade().duration(1.0).show(ui, outlet);
        }))
        .page(
            "/",
            Page::new(move |ui: &mut Ui| seen.borrow_mut().push(ui.opacity())),
        )
        .page("/about", Page::new(|_: &mut Ui| {}))
        .with_state_and_history(AppState::default(), MemoryHistory::new("/"));
    frame(&ctx, &mut app, 0.0);
    app.goto("/about");
    frame(&ctx, &mut app, 1.0);
    assert_eq!(
        *opacity.borrow(),
        [1.0, 1.0],
        "the fade starts at full opacity"
    );
}

#[test]
fn effects_know_the_navigation() {
    let ctx = egui::Context::default();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let log = Rc::clone(&seen);
    let mut app = Router::new()
        .layout(Layout::new(move |ui: &mut Ui, outlet: Outlet<'_>| {
            let log = Rc::clone(&log);
            ViewTransition::custom(move |frame| {
                let navigation = &frame.navigation;
                log.borrow_mut()
                    .push(format!("{:?} -> {}", navigation.from, navigation.to));
                (Layer::VISIBLE, Layer::VISIBLE)
            })
            .duration(1.0)
            .show(ui, outlet);
        }))
        .page("/", Page::new(|_: &mut Ui| {}))
        .page("/about", Page::new(|_: &mut Ui| {}))
        .with_state_and_history(AppState::default(), MemoryHistory::new("/"));
    frame(&ctx, &mut app, 0.0);
    app.goto("/about?tab=1");
    frame(&ctx, &mut app, 1.0);
    assert_eq!(*seen.borrow(), [r#"Some("/") -> /about?tab=1"#]);
}
