//! Views reading the data of the layouts around them.

use core::cell::RefCell;
use std::rc::Rc;

use egui::Ui;
use egui_kit::extract::{LayoutData, State};
use egui_kit::page::{Layout, Outlet, Page};
use egui_kit::{MemoryHistory, Rejection, Router, RouterApp};

type Log = Rc<RefCell<Vec<String>>>;

#[derive(Clone)]
struct Workspace(String);

async fn workspace() -> Result<Workspace, Rejection> {
    Ok(Workspace("outer".into()))
}

async fn inner_workspace() -> Result<Workspace, Rejection> {
    Ok(Workspace("inner".into()))
}

async fn count() -> Result<u32, Rejection> {
    Ok(7)
}

fn app(url: &str) -> RouterApp<Log> {
    let shared = |ui: &mut Ui, mut outlet: Outlet<'_>, _: &Workspace| outlet.show(ui);
    let page = Page::new(
        |_: &mut Ui, State(log): State<&Log>, workspace: LayoutData<'_, Workspace>| {
            log.borrow_mut().push(workspace.0.clone());
        },
    );
    let inner = Router::new()
        .layout(Layout::with_load(
            inner_workspace,
            // Reads the layout above, and shares its own data with the page below.
            |ui: &mut Ui,
             mut outlet: Outlet<'_>,
             State(log): State<&Log>,
             _: &Workspace,
             outer: LayoutData<'_, Workspace>| {
                log.borrow_mut()
                    .push(format!("inner layout in {}", outer.0));
                outlet.show(ui);
            },
        ))
        .page("/", page);
    let exclusive = Router::new()
        .layout(Layout::with_load(
            count,
            |ui: &mut Ui, mut outlet: Outlet<'_>, count: &mut u32| {
                *count += 1;
                outlet.show(ui);
            },
        ))
        .page("/", Page::new(|_: &mut Ui, _: LayoutData<'_, u32>| {}));

    Router::new()
        .layout(Layout::with_load(workspace, shared))
        .page(
            "/",
            Page::new(
                |_: &mut Ui, State(log): State<&Log>, workspace: LayoutData<'_, Workspace>| {
                    log.borrow_mut().push(workspace.0.clone());
                },
            ),
        )
        .page(
            "/missing",
            Page::new(|_: &mut Ui, _: LayoutData<'_, String>| {}),
        )
        .nest("/inner", inner)
        .nest("/exclusive", exclusive)
        .with_state_and_history(Log::default(), MemoryHistory::new(url))
}

fn frame(app: &mut RouterApp<Log>) -> Vec<String> {
    let ctx = egui::Context::default();
    let mut output = ctx.run_ui(Default::default(), |ui| app.ui(ui));
    output.textures_delta.clear();
    core::mem::take(&mut *app.state().borrow_mut())
}

#[test]
fn views_read_the_nearest_layout_data() {
    assert_eq!(frame(&mut app("/")), ["outer"]);
    assert_eq!(
        frame(&mut app("/inner")),
        ["inner layout in outer", "inner"]
    );
}

#[test]
#[should_panic(expected = "no layout around the view loaded a `alloc::string::String`")]
fn missing_layout_data_panics() {
    frame(&mut app("/missing"));
}

#[test]
#[should_panic(expected = "take it as `&` instead")]
fn layout_data_taken_mutably_panics() {
    frame(&mut app("/exclusive"));
}
