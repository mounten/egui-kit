//! Extractors: the arguments of hooks, `load`s and views.
//!
//! Hooks and `load`s are async functions whose arguments implement [`FromRequest`]; views are
//! functions of a `&mut egui::Ui` whose other arguments implement [`FromView`]. The extractors
//! can be in any order.
//!
//! # In hooks and `load`s
//!
//! | Extractor | Gives you |
//! |---|---|
//! | [`Path<T>`] | the route's path parameters, deserialized with serde |
//! | [`Query<T>`] | the query string, deserialized with serde |
//! | [`State<T>`] | a clone of the app state, or of any part of it that implements [`FromRef`] |
//! | [`Parent`] | the data loaded by the layouts above |
//! | [`Request`] | the url and the route's parameters, untyped |
//!
//! What a `load` extracts decides when it reruns: [`Path`] and [`Query`] make it depend on the
//! parameters and the query, [`Parent`] on the layouts above.
//!
//! # In views
//!
//! | Extractor | Gives you |
//! |---|---|
//! | `&mut D` / `&D` | the data the `load` returned (`()` without one), checked at compile time |
//! | `&RouteError` | the error, in error pages |
//! | [`LayoutData<T>`] | the data of the nearest layout around the view that loaded a `T` |
//! | [`State<&T>`](State) | the app state, or any part of it that implements [`FromRef`], borrowed |
//! | [`Nav`] | the router: links, `goto`, back/forward, loading, invalidation, shallow routing, blocking |
//! | [`Scroll`] | the page's scroll area, whose position the router restores |
//! | [`PageState<T>`] | state the view owns while its page is shown |
//! | [`PageStateFrom<T>`] | like [`PageState`], created from the `load`'s data |
//! | [`Snapshot<T>`] | like [`PageState`], and restored when coming back with back/forward |
//! | [`PersistentState<T>`] | app-wide state that survives app restarts, like `localStorage` |
//! | [`HistoryState<&T>`](HistoryState) | the shallow-routing state of the current history entry |
//! | [`Navigation`] | how the page was reached: from where, how, in which history direction |
//! | [`Request`] | the url the page was loaded with, and its route's parameters |
//!
//! Layout views also get the [`Children`](crate::page::Children) as their second argument; it is not an
//! extractor, every layout has exactly one.
//!
//! # Borrowing and mutating
//!
//! Views run synchronously within the frame, so what they extract is borrowed from the router:
//! nothing big is cloned every frame. Hooks and `load`s are futures that can outlive the frame,
//! so they get owned values, cloned once per navigation.
//!
//! # Which state to use
//!
//! | | Lives | Mutable by views |
//! |---|---|---|
//! | [`State<&T>`](State) | as long as the app, shared by everything | through `Cell` / `RefCell` |
//! | `&mut D` (the `load`'s data) | until the `load` reruns | yes |
//! | [`PageState<T>`] | while the page (or layout) is shown, across invalidations | yes |
//! | [`Snapshot<T>`] | like [`PageState`], and restored when coming back with back/forward | yes |
//! | [`HistoryState<&T>`](HistoryState) | with its history entry, set by [`Nav::push_state`] | through [`Nav`] |
//! | [`PersistentState<T>`] | across app restarts, shared by the whole app | yes |
//!
//! A rule of thumb: what you loaded goes in the `load`'s data, what the user did goes in a
//! [`PageState`] or a [`Snapshot`], and preferences go in a [`PersistentState`]. The app state
//! is shared by every view at once, so no view can get `&mut` access to it.
//!
//! [`RouteError`]: crate::RouteError

mod history_state;
mod layout_data;
mod nav;
pub(crate) mod parent;
mod path;
mod persistent_state;
mod query;
mod scroll;
mod state;
mod view_state;

use crate::error::Rejection;
use crate::navigation::Navigation;
use crate::page::ViewContext;
use crate::request::Request;
pub use history_state::HistoryState;
pub use layout_data::LayoutData;
pub use nav::Nav;
pub(crate) use nav::{NavAction, NavState};
pub use parent::Parent;
pub use path::Path;
pub use persistent_state::PersistentState;
pub use query::Query;
pub(crate) use scroll::PageScroll;
pub use scroll::Scroll;
pub use state::{FromRef, State};
pub(crate) use view_state::ViewStore;
pub use view_state::{PageState, PageStateFrom, Snapshot, SnapshotFrom};

/// Types that can be extracted from a navigation, used as arguments of hooks and `load`s.
///
/// Extraction is synchronous and runs before the handler's future is created. What an extractor
/// reads from the [`Request`] (path, query, params) decides when the `load` reruns.
#[diagnostic::on_unimplemented(
    message = "`{Self}` can't be extracted in a hook or `load`",
    note = "hooks and `load`s own what they extract: `State<T>` rather than `State<&T>`",
    note = "see the `egui_kit::extract` module for the available extractors"
)]
pub trait FromRequest<S>: Sized {
    /// Extracts the value from the request and the app state.
    ///
    /// # Errors
    ///
    /// The [`Rejection`] is handled like one returned from the hook or `load` itself: an error
    /// renders the nearest error page, a redirect navigates elsewhere.
    fn from_request(req: &Request, state: &S) -> Result<Self, Rejection>;
}

/// Types that can be extracted for a view, used as arguments of page, layout and error views.
///
/// `S` is the app state and `D` the data of the view: what its `load` returned
/// (`()` without one), or the [`RouteError`](crate::RouteError) for error pages. `Item` is the
/// extractor with the lifetime of the frame, e.g. `Nav<'f>` for `Nav<'_>`.
///
/// A custom extractor, e.g. for the signed-in user:
///
/// ```
/// use egui_kit::page::ViewContext;
/// use egui_kit::extract::FromView;
///
/// #[derive(Clone)]
/// struct AppState {
///     user: Option<String>,
/// }
///
/// struct User(Option<String>);
///
/// impl<D> FromView<AppState, D> for User {
///     type Item<'f> = User;
///
///     fn from_view(cx: &ViewContext<'_, AppState>) -> User {
///         User(cx.state().user.clone())
///     }
/// }
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` can't be extracted in a view",
    note = "views borrow the app state: `State<&T>` rather than `State<T>`, and `HistoryState<&T>`",
    note = "the data must be what the `load` returned: `&mut {D}` or `&{D}`",
    note = "see the `egui_kit::extract` module for the available extractors"
)]
pub trait FromView<S: 'static, D>: Sized {
    type Item<'f>;

    /// Whether this takes the view's data mutably (`&mut D`).
    #[doc(hidden)]
    const BORROWS_DATA_MUT: bool = false;

    /// Called for every extractor of a view before any is extracted, e.g. to read the data
    /// before an extractor takes it.
    #[doc(hidden)]
    fn prepare(_cx: &ViewContext<'_, S>) {}

    fn from_view<'f>(cx: &'f ViewContext<'_, S>) -> Self::Item<'f>;
}

impl<S> FromRequest<S> for Request {
    fn from_request(req: &Request, _state: &S) -> Result<Self, Rejection> {
        Ok(req.clone())
    }
}

/// The url the page was loaded with and the parameters of its route. Unaffected by shallow
/// routing, see [`Nav::url`].
impl<S: 'static, D> FromView<S, D> for Request {
    type Item<'f> = Self;

    fn from_view(cx: &ViewContext<'_, S>) -> Self {
        cx.frame().request.clone()
    }
}

/// How the page was reached.
impl<S: 'static, D> FromView<S, D> for Navigation {
    type Item<'f> = Self;

    fn from_view(cx: &ViewContext<'_, S>) -> Self {
        cx.frame().navigation.clone()
    }
}

/// The view's data: what its `load` returned, or the error of an error page.
impl<S: 'static, D: 'static> FromView<S, D> for &mut D {
    type Item<'f> = &'f mut D;

    const BORROWS_DATA_MUT: bool = true;

    fn from_view<'f>(cx: &'f ViewContext<'_, S>) -> &'f mut D {
        cx.take_data_mut().downcast_mut().expect(MISMATCH)
    }
}

/// Read-only access to the view's data.
impl<S: 'static, D: 'static> FromView<S, D> for &D {
    type Item<'f> = &'f D;

    fn from_view<'f>(cx: &'f ViewContext<'_, S>) -> &'f D {
        cx.take_data().downcast_ref().expect(MISMATCH)
    }
}

const MISMATCH: &str = "egui-kit: the data does not match the view (this is a bug)";
