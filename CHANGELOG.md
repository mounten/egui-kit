# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0](https://github.com/mounten/egui-kit/compare/v0.2.0...v0.3.0) - 2026-10-12

### Added

- *(remote)* [**breaking**] replace remote functions with owned `Query` and `Command` handles ([#30](https://github.com/mounten/egui-kit/pull/30))
  - `remote::query(|| async { .. })` creates a `Query<T, E>`: async data that fetches on its
    first read, with `refresh`, `refetch_every`, a loading state, and a cache shared by
    queries with the same `key`.
  - `remote::command()` creates a `Command<T, E>`: an async mutation with a pending state and a
    result, that `refreshes` the loads and queries depending on what it changed.
  - Both are plain values to keep in the app state, a `PageState` or any struct, and work
    outside of the router too. `remote::invalidate` invalidates from anywhere on the UI thread.
  - Natively, they run on Tokio: the runtime the app entered, or one egui-kit starts.
    `RouterApp::ui` enters it, so loads and hooks can use Tokio-based clients like `reqwest`
    without entering a runtime yourself.
  - Behind the `remote` feature, on by default.
- *(router)* add `RouterApp::matches` to resolve urls ([#26](https://github.com/mounten/egui-kit/pull/26))
- *(history)* implement `History` for boxed histories ([#25](https://github.com/mounten/egui-kit/pull/25))
- *(extract)* add `Nav::replace_url` ([#23](https://github.com/mounten/egui-kit/pull/23))
- *(extract)* read and write `PersistentState` outside of views ([#22](https://github.com/mounten/egui-kit/pull/22))
- *(extract)* add `PageStateFrom` and `SnapshotFrom` ([#21](https://github.com/mounten/egui-kit/pull/21))
- *(transition)* [**breaking**] tell effects which navigation they animate ([#20](https://github.com/mounten/egui-kit/pull/20))
- *(extract)* make `Nav::follow` public ([#18](https://github.com/mounten/egui-kit/pull/18))
- *(extract)* add `LayoutData` to read a layout's data in views ([#17](https://github.com/mounten/egui-kit/pull/17))

### Removed

- [**breaking**] the `remote::Query` and `remote::Command` traits, the `Remote` extractor,
  `CommandHandle`, `Spawner`, `Running`, `RouterApp::spawner` and the `task` module, replaced by
  the handles above ([#30](https://github.com/mounten/egui-kit/pull/30))

### Documentation

- document redirects in the history, goto to the current url and query cancellation ([#27](https://github.com/mounten/egui-kit/pull/27))

### Fixed

- *(extract)* make `Nav::is_active` follow shallow routing ([#24](https://github.com/mounten/egui-kit/pull/24))
- *(transition)* make the outgoing page non-interactive ([#19](https://github.com/mounten/egui-kit/pull/19))
- *(router)* don't add a 404 fallback for merged routers ([#8](https://github.com/mounten/egui-kit/pull/8))
- *(navigation)* keep a pending navigation when asked to go to its url again ([#7](https://github.com/mounten/egui-kit/pull/7))

### Migrating from 0.2

**Queries.** A query is no longer a type implementing `remote::Query`, read with
`remote.query(..)` in a view. Create a `Query` once, keep it, and read it:

```rust
// 0.2
#[derive(Clone, PartialEq)]
struct PostCount;

impl Query<AppState> for PostCount {
    type Output = usize;

    async fn run(self, state: AppState) -> Result<usize, RouteError> {
        Ok(state.db.posts().len())
    }

    fn depends(&self) -> Vec<String> {
        vec!["posts".into()]
    }
}

fn view(ui: &mut Ui, remote: Remote<'_, AppState>) {
    if let Some(count) = remote.query(PostCount).current() { /* .. */ }
}

// 0.3
let post_count = remote::query(move || {
    let db = db.clone();
    async move { Ok::<_, String>(db.posts().len()) }
})
.key("post_count")
.depends("posts");

// e.g. as a field of the app state
fn view(ui: &mut Ui, State(state): State<&AppState>) {
    if let Some(count) = state.post_count.data() { /* .. */ }
}
```

- The query's fields become the closure's captures, and its value becomes its `key`: queries
  with the same key share their value, like equal queries did.
- `depends`, `refetch_every` and `keep_unread` become the builder methods `depends`,
  `refetch_every` and `cache_for`.
- `QueryState::current`, `rc`, `error`, `version` and `is_loading` become `Query::data`,
  `error`, `version`, and `is_loading` (no value yet) or `is_fetching` (any fetch); `state()`
  returns a `QueryState` to `match` on. `remote.refresh(&query)` becomes `query.refresh()`.

**Commands.** A command is no longer a type implementing `remote::Command`. Keep a `Command` (in
a `PageState`, say) and run any future with it:

```rust
// 0.2
let publish = remote.command::<Publish>();
publish.run(Publish { title, body });

// 0.3
struct Draft {
    publish: Command<String, String>, // remote::command().refreshes("posts")
}

let db = state.db.clone();
draft.publish.run(async move { db.publish(&title, &body) });
```

- `Command::refreshes` moves from the trait to the builder.
- `CommandHandle::is_pending`, `take_result` and `error` keep their names; `take_output` and
  `take_error` become `take_result()` and matching on it, or `output()` and `error()`.
- `remote.command_keyed(key)` becomes one `Command` per key, e.g. in a `HashMap`.
- `remote.run(future)`, `Spawner`, `Running` and `task::Task` become a `Command` too: it runs
  any future, from anywhere on the UI thread.

**Errors and state.** The error type is yours: `Query<T, E>` and `Command<T, E>` instead of
`RouteError`. Futures don't get the router's state; capture what they need (a database or HTTP
client handle) instead. Natively, they must be `Send`: they run on Tokio's threads, so use `Arc`
and `Mutex` rather than `Rc` and `RefCell` for what they share.

**Runtimes.** Remove the `tokio::runtime::Runtime` you entered around `RouterApp::ui`. egui-kit
uses the one you entered, if any, else starts its own. Without default features, `remote` and
its Tokio dependency are left out.

## [0.2.0](https://github.com/mounten/egui-kit/compare/v0.1.0...v0.2.0) - 2026-10-11

### Added

- *(remote)* [**breaking**] new (async) queries and commands available for page ui methods ([#6](https://github.com/mounten/egui-kit/pull/6))
- *(history)* showing hovered links in the browser ([#4](https://github.com/mounten/egui-kit/pull/4))

## [0.1.0](https://github.com/mounten/egui-kit/releases/tag/v0.1.0) - 2026-10-11

### Added

Initial release.

- `Router` with nested and merged routers, path parameters, parameter matchers, optional
  parameters and rest parameters.
- Pages with an optional async `load` and a view, and layouts that wrap the pages of a router.
- Extractors for loads and hooks (`Path`, `Query`, `State`, `Parent`, `Request`) and for views
  (`Nav`, `State`, `Scroll`, `Navigation`, `Request`).
- Hooks that run before the loads of a router's pages, error pages, and redirects.
- Loads only rerun when their inputs change, can be invalidated by key, and are preloaded when a
  link is hovered.
- View state that lives with the page (`PageState`), the history entry (`Snapshot`) or across
  app restarts (`PersistentState`).
- Shallow routing with `Nav::push_state` and `HistoryState`.
- Blocking navigation while a page has unsaved changes.
- View transitions between pages.
- Browser history on the web, and in-memory history with mouse back/forward buttons and Alt+←/→
  natively.
