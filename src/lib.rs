//! A router for [egui](https://github.com/emilk/egui), inspired by SvelteKit's routing and axum's
//! API.
//!
//! Routes map paths to pages. A page has an optional async `load` that runs when the page is
//! navigated to, and a view that is called every frame with what the load returned. Both get their
//! inputs as typed function arguments, the [extractors](extract).
//!
//! ```
//! use egui::Ui;
//! use egui_kit::{Rejection, Router, RouterApp};
//! use egui_kit::extract::{Nav, Path};
//! use egui_kit::page::Page;
//!
//! fn home(ui: &mut Ui, nav: Nav) {
//!     ui.heading("Home");
//!     nav.link(ui, "Say hello", "/hello/ferris");
//! }
//!
//! async fn load_greeting(Path(name): Path<String>) -> Result<String, Rejection> {
//!     Ok(format!("Hello, {name}!"))
//! }
//!
//! fn greeting(ui: &mut Ui, greeting: &String, nav: Nav) {
//!     ui.heading(greeting);
//!     nav.link(ui, "Home", "/");
//! }
//!
//! fn app() -> RouterApp<()> {
//!     Router::new()
//!         .page("/", Page::new(home))
//!         .page("/hello/{name}", Page::with_load(load_greeting, greeting))
//!         .with_state(())
//! }
//!
//! // Call this every frame, e.g. from `eframe::App::ui`.
//! fn frame(app: &mut RouterApp<()>, ui: &mut Ui) {
//!     app.ui(ui);
//! }
//! ```
//!
//! On the web, the router uses the browser's history, so the address bar and the back/forward
//! buttons work. Natively, it keeps the history in memory and handles the mouse's back/forward
//! buttons and Alt+←/→. With `BrowserHistory::link_preview`, the browser also treats hovered
//! links like real ones: it shows their url, and opens them in a new tab on Ctrl/Cmd-click.
//!
//! # Routing
//!
//! [`Router::page`] adds a page at a path, [`Router::nest`] adds another router under a prefix.
//! A router's [layout](Router::layout), [hooks](Router::hook) and [error page](Router::error)
//! apply to its own pages and to the routers nested in it.
//!
//! | Path segment | Matches |
//! |---|---|
//! | `about` | exactly `about` |
//! | `{slug}` | any segment |
//! | `{id=int}` | a segment the [`int` matcher](Router::matcher) accepts |
//! | `{?lang}`, `{?lang=lang}` | an optional segment |
//! | `{*path}` | the rest of the path (last segment only) |
//!
//! When several routes match, static segments win over parameters with a matcher, which win over
//! plain parameters.
//!
//! ```
//! use egui::Ui;
//! use egui_kit::Router;
//! use egui_kit::page::{Layout, Outlet, Page};
//!
//! fn view(ui: &mut Ui) {
//!     ui.label("A page");
//! }
//!
//! fn blog_layout(ui: &mut Ui, mut outlet: Outlet) {
//!     ui.heading("Blog");
//!     outlet.show(ui);
//! }
//!
//! let blog = Router::new()
//!     .layout(Layout::new(blog_layout))
//!     .page("/", Page::new(view)) // `/blog`
//!     .page("/{slug}", Page::new(view)) // `/blog/hello-world`
//!     .page("/{slug}/print", Page::new(view).reset_layouts()); // without the blog layout
//!
//! let app = Router::<()>::new()
//!     .matcher("int", |segment| segment.parse::<u32>().is_ok())
//!     .page("/", Page::new(view))
//!     .page("/users/{id=int}", Page::new(view)) // `/users/7`, but not `/users/ferris`
//!     .page("/{?lang}/about", Page::new(view)) // `/about` and `/de/about`
//!     .page("/files/{*path}", Page::new(view)) // `/files/a/b/c`
//!     .nest("/blog", blog)
//!     .with_state(());
//! ```
//!
//! [`Router::merge`] adds a router's pages at the same level, keeping its layout, hooks and error
//! page to its own pages. Building the app panics on conflicting routes and unknown matchers.
//!
//! # Pages and layouts
//!
//! A [`Page`](page::Page) is a view, created with [`Page::new`](page::Page::new), or a load and a
//! view, created with [`Page::with_load`](page::Page::with_load):
//!
//! - A **load** is an `async fn` of extractors returning `Result<D, E>`, where
//!   `E: Into<Rejection>`. No async runtime is needed. While it runs, the previous page stays on
//!   screen.
//! - A **view** is a function of `&mut Ui` and extractors, called every frame. It gets the load's
//!   data as `&D` or `&mut D`.
//!
//! A [`Layout`](page::Layout) wraps the pages of a router. It can have a load too, and its view
//! draws the page through the [`Outlet`](page::Outlet), or animates page changes with a
//! [`ViewTransition`].
//!
//! ```
//! use egui::Ui;
//! use egui_kit::{Rejection, Router, ViewTransition, error};
//! use egui_kit::extract::{Nav, Parent, Path};
//! use egui_kit::page::{Layout, Outlet, Page};
//!
//! #[derive(Clone)]
//! struct Blog {
//!     posts: Vec<String>,
//! }
//!
//! async fn load_blog() -> Result<Blog, Rejection> {
//!     Ok(Blog { posts: vec!["hello".into(), "world".into()] })
//! }
//!
//! fn blog_layout(ui: &mut Ui, outlet: Outlet, blog: &Blog, nav: Nav) {
//!     for post in &blog.posts {
//!         nav.nav_link(ui, post, &format!("/blog/{post}"));
//!     }
//!     ViewTransition::slide().show(ui, outlet);
//! }
//!
//! struct Post {
//!     slug: String,
//!     comment: String,
//! }
//!
//! // `Parent` gives access to the data of the layouts above.
//! async fn load_post(Path(slug): Path<String>, parent: Parent) -> Result<Post, Rejection> {
//!     let blog = parent.get::<Blog>().await?;
//!     if !blog.posts.contains(&slug) {
//!         return Err(error(404, "no such post"));
//!     }
//!     Ok(Post { slug, comment: String::new() })
//! }
//!
//! fn post(ui: &mut Ui, post: &mut Post) {
//!     ui.heading(&post.slug);
//!     ui.text_edit_singleline(&mut post.comment);
//! }
//!
//! let blog = Router::<()>::new()
//!     .layout(Layout::with_load(load_blog, blog_layout).depends("app:posts"))
//!     .page("/{slug}", Page::with_load(load_post, post));
//! ```
//!
//! A load only reruns when the parameters, query or parent data it uses change: above, going from
//! one post to another doesn't reload the blog. [`Nav::invalidate`](extract::Nav::invalidate)
//! reruns the loads that [`depend`](page::Page::depends) on a key. Links preload their target
//! while hovered.
//!
//! # Extractors
//!
//! Extractors are the arguments of loads, hooks and views. The [`extract`] module lists all of
//! them.
//!
//! ```
//! use egui::Ui;
//! use egui_kit::Rejection;
//! use egui_kit::extract::{Nav, Path, Query, State};
//! use serde::Deserialize;
//!
//! #[derive(Clone)]
//! struct AppState;
//!
//! #[derive(Deserialize)]
//! struct Pagination {
//!     page: Option<u32>,
//! }
//!
//! async fn load(
//!     Path((user, repo)): Path<(String, String)>,
//!     Query(pagination): Query<Pagination>,
//!     State(state): State<AppState>,
//! ) -> Result<Vec<String>, Rejection> {
//!     let page = pagination.page.unwrap_or(1);
//!     Ok(vec![format!("{user}/{repo}, page {page}")])
//! }
//!
//! fn view(ui: &mut Ui, issues: &Vec<String>, nav: Nav) {
//!     for issue in issues {
//!         ui.label(issue);
//!     }
//!     if ui.button("Back").clicked() {
//!         nav.back();
//!     }
//! }
//! ```
//!
//! | In loads and hooks | In views |
//! |---|---|
//! | [`Path<T>`](extract::Path), [`Query<T>`](extract::Query): parameters and query | `&D` / `&mut D`: the load's data; `&RouteError` in error pages |
//! | [`State<T>`](extract::State): the app state | [`State<&T>`](extract::State): the app state |
//! | [`Parent`](extract::Parent): the data of the layouts above | [`Nav`](extract::Nav): links and navigation |
//! | [`Request`]: the url and parameters | [`PageState<T>`](extract::PageState), [`Snapshot<T>`](extract::Snapshot), [`PersistentState<T>`](extract::PersistentState): [view state](#view-state) |
//! | | [`Scroll`](extract::Scroll), [`HistoryState<&T>`](extract::HistoryState), [`Navigation`], [`Request`] |
//! | | [`Remote<S>`](extract::Remote): [queries and commands](#queries-and-commands) |
//!
//! # Errors and redirects
//!
//! Loads and hooks return a [`Rejection`] to abort a navigation: a [`RouteError`], created with
//! [`error`], or a [`Redirect`], created with [`redirect`]. Any error type that implements
//! `Into<Rejection>` can be returned, so `?` works.
//!
//! An error shows the nearest [error page](Router::error) inside its router's layout, or a default
//! one. Paths that match no route are a 404, a [`Path`](extract::Path) that doesn't parse too, and
//! a [`Query`](extract::Query) that doesn't parse is a 400.
//!
//! ```
//! use egui::Ui;
//! use egui_kit::{Rejection, RouteError, Router, redirect};
//! use egui_kit::extract::{Nav, State};
//! use egui_kit::page::Page;
//!
//! #[derive(Clone)]
//! struct AppState {
//!     logged_in: bool,
//! }
//!
//! // A hook runs before the loads of the router's pages.
//! async fn require_login(State(state): State<AppState>) -> Result<(), Rejection> {
//!     if state.logged_in { Ok(()) } else { Err(redirect("/login")) }
//! }
//!
//! fn error_page(ui: &mut Ui, error: &RouteError, nav: Nav) {
//!     ui.heading(format!("{}: {}", error.status, error.message));
//!     nav.link(ui, "Home", "/");
//! }
//!
//! fn view(ui: &mut Ui) {
//!     ui.label("Admin");
//! }
//!
//! let admin = Router::<AppState>::new()
//!     .hook(require_login)
//!     .error(error_page)
//!     .page("/", Page::new(view));
//! ```
//!
//! # Queries and commands
//!
//! Not all data belongs to a route: a badge in a layout, an autocomplete. And mutations are often
//! async too, which a view can't `await`. Inspired by SvelteKit's remote functions, a
//! [`Query`](remote::Query) is async data any view can read, cached by value, and a
//! [`Command`](remote::Command) an async mutation that a view starts and watches. A command
//! refreshes the loads and queries that depend on what it changed. Views use both through the
//! [`Remote`](extract::Remote) extractor; see the [`remote`] module.
//!
//! # State
//!
//! The state passed to [`Router::with_state`] is available everywhere with the
//! [`State`](extract::State) extractor. Views borrow it as `State<&T>`, loads and hooks get a clone
//! as `State<T>`, so keep shared data behind an `Rc`. Nothing gets `&mut` access to it; use `Cell`
//! or `RefCell` to change it. [`FromRef`](extract::FromRef) lets you extract a part of the state.
//! [`RouterApp::state`] gives access to it outside of the router.
//!
//! ```
//! use std::cell::RefCell;
//! use std::rc::Rc;
//!
//! use egui::Ui;
//! use egui_kit::{Rejection, Router};
//! use egui_kit::extract::{FromRef, State};
//! use egui_kit::page::Page;
//!
//! #[derive(Clone, Default)]
//! struct AppState {
//!     db: Db,
//! }
//!
//! #[derive(Clone, Default)]
//! struct Db {
//!     todos: Rc<RefCell<Vec<String>>>,
//! }
//!
//! impl FromRef<AppState> for Db {
//!     fn from_ref(state: &AppState) -> &Db {
//!         &state.db
//!     }
//! }
//!
//! async fn load(State(db): State<Db>) -> Result<usize, Rejection> {
//!     Ok(db.todos.borrow().len())
//! }
//!
//! fn view(ui: &mut Ui, count: &usize, State(db): State<&Db>) {
//!     ui.label(format!("{count} todos when the page was loaded"));
//!     if ui.button("Add").clicked() {
//!         db.todos.borrow_mut().push("New todo".into());
//!     }
//! }
//!
//! let app = Router::new()
//!     .page("/", Page::with_load(load, view))
//!     .with_state(AppState::default());
//! ```
//!
//! ## View state
//!
//! Views can own state that the router keeps for them. It is created with `T::default()` the
//! first time a view extracts it.
//!
//! | | Lives |
//! |---|---|
//! | [`PageState<T>`](extract::PageState) | while the page or layout is shown |
//! | [`Snapshot<T>`](extract::Snapshot) | like `PageState`, and restored when coming back with back/forward |
//! | [`PersistentState<T>`](extract::PersistentState) | across app restarts, with eframe's `persistence` feature |
//!
//! ```
//! use egui::Ui;
//! use egui_kit::extract::{PageState, PersistentState, Snapshot};
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Default)]
//! struct Draft {
//!     title: String,
//! }
//!
//! #[derive(Default)]
//! struct Search {
//!     text: String,
//! }
//!
//! #[derive(Clone, Default, Serialize, Deserialize)]
//! struct Preferences {
//!     compact: bool,
//! }
//!
//! fn view(
//!     ui: &mut Ui,
//!     mut draft: PageState<Draft>,
//!     mut search: Snapshot<Search>,
//!     mut preferences: PersistentState<Preferences>,
//! ) {
//!     ui.text_edit_singleline(&mut draft.title);
//!     ui.text_edit_singleline(&mut search.text);
//!     ui.checkbox(&mut preferences.compact, "Compact");
//! }
//! ```

mod app;
mod error;
mod exec;
pub mod extract;
mod history;
mod load;
mod navigation;
pub mod page;
pub mod remote;
mod render;
mod request;
mod router;
mod session;
mod transition;

pub use app::RouterApp;
pub use error::{Redirect, Rejection, RouteError, error, redirect};
#[cfg(target_arch = "wasm32")]
pub use history::{BrowserHistory, HistoryMode};
pub use history::{EntryKey, History, HistoryEntry, MemoryHistory, default_history};
pub use navigation::{Direction, GotoOptions, Navigation, NavigationKind};
pub use request::Request;
pub use router::Router;
pub use transition::{Layer, TransitionFrame, ViewTransition, prefers_reduced_motion};

/// Calls `$m!` with 0 to 16 type parameters, for the extractor arguments of handlers and views.
macro_rules! all_tuples {
    ($m:ident) => {
        $m!();
        $m!(T1);
        $m!(T1, T2);
        $m!(T1, T2, T3);
        $m!(T1, T2, T3, T4);
        $m!(T1, T2, T3, T4, T5);
        $m!(T1, T2, T3, T4, T5, T6);
        $m!(T1, T2, T3, T4, T5, T6, T7);
        $m!(T1, T2, T3, T4, T5, T6, T7, T8);
        $m!(T1, T2, T3, T4, T5, T6, T7, T8, T9);
        $m!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10);
        $m!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11);
        $m!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12);
        $m!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13);
        $m!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14);
        $m!(
            T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15
        );
        $m!(
            T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15, T16
        );
    };
}
pub(crate) use all_tuples;
