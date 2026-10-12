//! Queries and commands: fetching, caching, refreshing, aborting, and invalidating loads.
#![cfg(feature = "remote")]

use core::cell::Cell;
use core::future::pending;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use core::time::Duration;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use egui::Ui;
use egui_kit::extract::State;
use egui_kit::page::Page;
use egui_kit::remote::{self, Command, Query, QueryState};
use egui_kit::{MemoryHistory, Rejection, Router, RouterApp};
use tokio::sync::watch;

/// Waits for queries and commands to finish on the runtime's threads.
fn wait_until(mut done: impl FnMut() -> bool) {
    let start = Instant::now();
    while !done() {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "timed out waiting for a fetch"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// A query whose value counts its fetches, and whose even fetches fail if `fail_even`.
fn counting(fetches: &Arc<AtomicU32>, fail_even: bool) -> Query<u32, String> {
    let fetches = Arc::clone(fetches);
    remote::query(move || {
        let n = fetches.fetch_add(1, Ordering::SeqCst) + 1;
        async move {
            if fail_even && n.is_multiple_of(2) {
                Err(format!("fetch {n} failed"))
            } else {
                Ok(n)
            }
        }
    })
}

fn data(query: &Query<u32, String>) -> Option<u32> {
    query.data().map(|value| *value)
}

#[test]
fn queries_fetch_on_their_first_read() {
    let fetches = Arc::default();
    let query = counting(&fetches, false);
    assert_eq!(fetches.load(Ordering::SeqCst), 0, "not before it is read");
    assert!(matches!(query.state(), QueryState::Loading));
    wait_until(|| data(&query).is_some());
    assert!(matches!(query.state(), QueryState::Ready(value) if *value == 1));
    assert_eq!(query.version(), 1);
    assert!(!query.is_fetching());
    assert_eq!(fetches.load(Ordering::SeqCst), 1, "fetched once");
}

#[test]
fn refreshing_keeps_the_value_until_the_new_one_arrives() {
    let fetches = Arc::default();
    let query = counting(&fetches, false);
    wait_until(|| data(&query).is_some());
    query.refresh();
    assert_eq!(data(&query), Some(1));
    assert_eq!(fetches.load(Ordering::SeqCst), 2);
    wait_until(|| data(&query) == Some(2));
    assert_eq!(query.version(), 2);
}

#[test]
fn failed_fetches_keep_the_previous_value() {
    let fetches = Arc::default();
    let query = counting(&fetches, true);
    wait_until(|| data(&query).is_some());
    query.refresh();
    wait_until(|| query.error().is_some());
    assert!(matches!(query.state(), QueryState::Failed(error) if *error == "fetch 2 failed"));
    assert_eq!(data(&query), Some(1));
    query.refresh();
    wait_until(|| data(&query) == Some(3));
    assert!(query.error().is_none(), "cleared by a successful fetch");
}

#[test]
fn keyed_queries_share_one_fetch_and_cache_the_value() {
    let (first, second) = (Arc::default(), Arc::default());
    let hour = Duration::from_secs(3600);
    let a = counting(&first, false).key("shared").stale_after(hour);
    let b = counting(&second, false).key("shared").stale_after(hour);
    a.state();
    b.state();
    wait_until(|| data(&b).is_some());
    assert_eq!(data(&a), Some(1));
    assert_eq!(
        second.load(Ordering::SeqCst),
        0,
        "the first query's function fetches"
    );
    assert_eq!(first.load(Ordering::SeqCst), 1, "once for both");

    drop((a, b));
    let c = counting(&second, false).key("shared").stale_after(hour);
    assert_eq!(data(&c), Some(1), "cached");
    assert!(!c.is_fetching(), "fresh enough");
}

#[test]
fn new_queries_refetch_stale_cached_values() {
    let fetches = Arc::default();
    let a = counting(&fetches, false).key("stale");
    wait_until(|| data(&a).is_some());
    let b = counting(&fetches, false).key("stale");
    assert_eq!(
        data(&b),
        Some(1),
        "the cached value is shown while it refetches"
    );
    assert_eq!(fetches.load(Ordering::SeqCst), 2);
    wait_until(|| data(&a) == Some(2));
    // Reading again doesn't refetch.
    data(&b);
    assert_eq!(fetches.load(Ordering::SeqCst), 2);
}

#[test]
fn clones_share_whether_the_query_was_read() {
    let fetches = Arc::default();
    let query = counting(&fetches, false);
    wait_until(|| data(&query.clone()).is_some());
    for _ in 0..3 {
        data(&query.clone());
    }
    assert_eq!(
        fetches.load(Ordering::SeqCst),
        1,
        "reading a clone doesn't refetch"
    );
}

#[test]
fn queries_refetch_periodically() {
    let fetches = Arc::default();
    let query = counting(&fetches, false).refetch_every(Duration::from_millis(10));
    wait_until(|| data(&query).is_some_and(|n| n >= 3));
}

#[test]
fn dropping_a_query_aborts_its_fetch() {
    struct Guard(Arc<AtomicBool>);

    impl Drop for Guard {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    let (started, dropped) = (Arc::new(AtomicBool::new(false)), Arc::default());
    let query: Query<(), String> = {
        let (started, dropped) = (Arc::clone(&started), Arc::clone(&dropped));
        remote::query(move || {
            let guard = Guard(Arc::clone(&dropped));
            let started = Arc::clone(&started);
            async move {
                let _guard = guard;
                started.store(true, Ordering::SeqCst);
                pending::<Result<(), String>>().await
            }
        })
    };
    query.state();
    wait_until(|| started.load(Ordering::SeqCst));
    drop(query);
    wait_until(|| dropped.load(Ordering::SeqCst));
}

#[test]
fn commands_report_whether_they_are_pending_and_how_they_ended() {
    let command: Command<u32, String> = remote::command();
    let (open, mut gate) = watch::channel(false);
    command.run(async move {
        gate.wait_for(|open| *open).await.ok();
        Ok(5)
    });
    assert!(command.is_pending());
    assert!(command.result().is_none());
    open.send(true).unwrap();
    wait_until(|| !command.is_pending());
    assert_eq!(command.output().as_deref(), Some(&5));
    assert_eq!(command.take_result(), Some(Ok(5)));
    assert_eq!(command.take_result(), None, "taken");

    command.run(async { Err("no".to_owned()) });
    wait_until(|| !command.is_pending());
    assert_eq!(command.error().as_deref().map(String::as_str), Some("no"));
    command.run(pending());
    assert!(command.result().is_none(), "forgotten when it runs again");
}

#[derive(Clone, Default)]
struct AppState {
    loads: Rc<Cell<u32>>,
}

async fn load(State(state): State<AppState>) -> Result<(), Rejection> {
    state.loads.set(state.loads.get() + 1);
    Ok(())
}

fn app() -> RouterApp<AppState> {
    Router::new()
        .page("/", Page::with_load(load, |_: &mut Ui| {}).depends("posts"))
        .with_state_and_history(AppState::default(), MemoryHistory::new("/"))
}

fn frame(ctx: &egui::Context, app: &mut RouterApp<AppState>) {
    let mut output = ctx.run_ui(Default::default(), |ui| app.ui(ui));
    output.textures_delta.clear();
}

#[test]
fn commands_and_invalidate_refresh_loads_and_queries() {
    let ctx = egui::Context::default();
    let mut app = app();
    frame(&ctx, &mut app);
    assert_eq!(app.state().loads.get(), 1);
    let fetches = Arc::default();
    let query = counting(&fetches, false).depends("posts");
    wait_until(|| data(&query).is_some());

    let publish: Command<(), String> = remote::command().refreshes("posts");
    publish.run(async { Ok(()) });
    wait_until(|| !publish.is_pending());
    frame(&ctx, &mut app);
    assert_eq!(app.state().loads.get(), 2, "the load reran");
    wait_until(|| data(&query) == Some(2));

    // A failed run refreshes nothing.
    publish.run(async { Err("no".to_owned()) });
    wait_until(|| !publish.is_pending());
    frame(&ctx, &mut app);
    assert_eq!(app.state().loads.get(), 2);

    remote::invalidate("posts");
    frame(&ctx, &mut app);
    assert_eq!(app.state().loads.get(), 3);
    wait_until(|| data(&query) == Some(3));
}

#[test]
fn unused_cache_entries_expire() {
    let ctx = egui::Context::default();
    let mut app = app();
    let fetches = Arc::default();
    let query = counting(&fetches, false)
        .key("expiring")
        .cache_for(Duration::ZERO);
    wait_until(|| data(&query).is_some());
    drop(query);
    frame(&ctx, &mut app);
    let query = counting(&fetches, false).key("expiring");
    assert_eq!(data(&query), None, "dropped from the cache");
}

#[test]
fn work_started_before_the_context_is_set_wakes_the_app() {
    let ctx = egui::Context::default();
    let woken = Arc::new(AtomicBool::new(false));
    ctx.set_request_repaint_callback({
        let woken = Arc::clone(&woken);
        move |_| woken.store(true, Ordering::SeqCst)
    });
    let command: Command<(), String> = remote::command();
    let (open, mut gate) = watch::channel(false);
    command.run(async move {
        gate.wait_for(|open| *open).await.ok();
        Ok(())
    });
    remote::set_context(&ctx);
    open.send(true).unwrap();
    wait_until(|| woken.load(Ordering::SeqCst));
}
