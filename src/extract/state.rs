//! [`State`]: the app state, in hooks, `load`s and views.

use super::{FromRequest, FromView};
use crate::error::Rejection;
use crate::page::ViewContext;
use crate::request::Request;

/// The app state passed to [`Router::with_state`](crate::Router::with_state), or a part of it
/// that implements [`FromRef`].
///
/// - **Views borrow it**: `State<&T>`. They run synchronously within the frame, so they never
///   pay for a clone, however big the state is.
/// - **Hooks and `load`s own it**: `State<T>`, a clone (hence `T: Clone`). Their futures can
///   outlive the frame, but they only run once per navigation. Keep shared data behind `Rc`
///   anyway, so the clone is cheap and changes are visible everywhere.
///
/// A view asking for an owned `State<T>` doesn't compile, so it can't clone by accident:
///
/// ```compile_fail
/// # use egui_kit::extract::State;
/// # use egui_kit::page::Page;
/// fn view(ui: &mut egui::Ui, State(state): State<String>) {}
///
/// let page = Page::<String>::new(view); // `State<&String>` would work
/// ```
///
/// Neither can get `&mut` access: every view of the page (and every load) shares the state, and
/// a layout's view still holds it while its page draws. Mutate it with `Cell` and `RefCell`.
///
/// ```
/// use std::cell::Cell;
/// use std::rc::Rc;
///
/// use egui::Ui;
/// use egui_kit::Rejection;
/// use egui_kit::extract::State;
///
/// #[derive(Clone, Default)]
/// struct AppState {
///     clicks: Rc<Cell<u32>>,
/// }
///
/// async fn load(State(state): State<AppState>) -> Result<u32, Rejection> {
///     Ok(state.clicks.get())
/// }
///
/// fn view(ui: &mut Ui, State(state): State<&AppState>) {
///     if ui.button("Click").clicked() {
///         state.clicks.set(state.clicks.get() + 1);
///     }
/// }
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct State<T>(pub T);

impl<S, T: FromRef<S> + Clone> FromRequest<S> for State<T> {
    fn from_request(_req: &Request, state: &S) -> Result<Self, Rejection> {
        Ok(Self(T::from_ref(state).clone()))
    }
}

impl<S: 'static, D, T: FromRef<S> + 'static> FromView<S, D> for State<&'_ T> {
    type Item<'f> = State<&'f T>;

    fn from_view<'f>(cx: &'f ViewContext<'_, S>) -> State<&'f T> {
        State(T::from_ref(cx.state()))
    }
}

/// A part of the app state that [`State`] can extract on its own, e.g. `State<&Db>` for an
/// `AppState` with a `db` field.
///
/// It borrows: a part is a field of the state, which views borrow and
/// hooks and `load`s clone. Every state is a part of itself.
///
/// ```
/// use egui_kit::extract::FromRef;
///
/// #[derive(Clone)]
/// struct AppState {
///     db: Db,
/// }
///
/// #[derive(Clone)]
/// struct Db;
///
/// impl FromRef<AppState> for Db {
///     fn from_ref(state: &AppState) -> &Db {
///         &state.db
///     }
/// }
/// ```
pub trait FromRef<S> {
    fn from_ref(state: &S) -> &Self;
}

impl<S> FromRef<S> for S {
    fn from_ref(state: &S) -> &S {
        state
    }
}
