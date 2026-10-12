//! Views: functions of a `&mut Ui` and [extractors](crate::extract), the view-side twin of
//! [`Handler`](super::Handler).

use core::any::Any;
use core::cell::{Cell, RefCell};

use egui::Ui;

use crate::extract::{FromView, Nav, NavState, PageScroll, ViewStore};
use crate::load::Loaded;
use crate::navigation::Navigation;
use crate::remote::RemoteStore;
use crate::request::Request;
use crate::transition::TransitionHandle;

/// What all views of a page share during a frame.
pub(crate) struct Frame<'a> {
    pub(crate) request: &'a Request,
    pub(crate) url: &'a str,
    pub(crate) ctx: egui::Context,
    pub(crate) history_state: Option<&'a (dyn Any + 'static)>,
    pub(crate) navigation: &'a Navigation,
    pub(crate) page_id: u64,
    pub(crate) scroll: &'a PageScroll,
    /// Whether this is the previous page, drawn while a transition animates it out.
    pub(crate) outgoing: bool,
    pub(crate) nav: &'a RefCell<NavState>,
    pub(crate) remote: &'a RemoteStore,
}

/// The data of a view: shared for layouts whose view doesn't take it as `&mut D`, so that the
/// views below can read it too.
pub(crate) enum ViewData<'a> {
    Mut(&'a mut (dyn Any + 'static)),
    Shared(&'a (dyn Any + 'static)),
}

/// What a view's extractors are extracted from: the app state, the page and the view's data.
pub struct ViewContext<'a, S> {
    state: &'a S,
    frame: &'a Frame<'a>,
    /// The data of the view, until an extractor takes it.
    data: Cell<Option<ViewData<'a>>>,
    store: &'a ViewStore,
    /// The data of the layouts around the view, outermost first.
    layouts: &'a [Option<Loaded>],
}

impl<'a, S> ViewContext<'a, S> {
    pub(crate) fn new(
        state: &'a S,
        frame: &'a Frame<'a>,
        data: ViewData<'a>,
        store: &'a ViewStore,
        layouts: &'a [Option<Loaded>],
    ) -> Self {
        Self {
            state,
            frame,
            data: Cell::new(Some(data)),
            store,
            layouts,
        }
    }

    pub fn state(&self) -> &S {
        self.state
    }

    pub fn nav(&self) -> Nav<'_> {
        Nav { frame: self.frame }
    }

    pub(crate) fn frame(&self) -> &Frame<'a> {
        self.frame
    }

    /// The page state and snapshots of the view's page or layout.
    pub(crate) fn store(&self) -> &'a ViewStore {
        self.store
    }

    /// Lets `f` read the view's data, before an extractor takes it.
    pub(crate) fn peek_data<R>(&self, f: impl FnOnce(&dyn Any) -> R) -> R {
        let data = self
            .data
            .take()
            .expect("egui-kit: the view's data was already extracted (this is a bug)");
        let result = f(match &data {
            ViewData::Mut(data) => &**data,
            ViewData::Shared(data) => *data,
        });
        self.data.set(Some(data));
        result
    }

    /// Takes the view's data; it can only be extracted once.
    pub(crate) fn take_data(&self) -> &'a (dyn Any + 'static) {
        match self.take() {
            ViewData::Mut(data) => data,
            ViewData::Shared(data) => data,
        }
    }

    /// Takes the view's data mutably; it can only be extracted once.
    pub(crate) fn take_data_mut(&self) -> &'a mut (dyn Any + 'static) {
        match self.take() {
            ViewData::Mut(data) => data,
            ViewData::Shared(_) => panic!("egui-kit: the data is shared (this is a bug)"),
        }
    }

    fn take(&self) -> ViewData<'a> {
        self.data
            .take()
            .expect("egui-kit: a view can only extract its data once")
    }

    /// The data of the layouts around the view, outermost first.
    pub(crate) fn layouts(&self) -> &'a [Option<Loaded>] {
        self.layouts
    }
}

/// A page or error view: `Fn(&mut Ui, T1, …, T16)` where every `T` is a [`FromView`] extractor,
/// in any order. `T` is a tuple of the extractor types and only keeps the implementations apart.
///
/// The data extractors are checked against what the `load` returns:
///
/// ```compile_fail
/// # use egui_kit::Rejection;
/// # use egui_kit::page::Page;
/// async fn load() -> Result<u32, Rejection> {
///     Ok(1)
/// }
///
/// fn view(ui: &mut egui::Ui, data: &String) {}
///
/// let page = Page::<()>::with_load(load, view); // `load` returns a `u32`, not a `String`
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a view: `fn(&mut Ui, extractors...)` with up to 16 view extractors",
    note = "views borrow the app state: `State<&T>` rather than `State<T>`, and `HistoryState<&T>`",
    note = "the data must be what the `load` returned: `&mut {D}` or `&{D}`",
    note = "closures need annotated arguments, e.g. `|ui: &mut Ui, nav: Nav| ...`",
    note = "see the `egui_kit::extract` module for the available extractors"
)]
pub trait View<T, S, D>: 'static {
    fn call(&self, ui: &mut Ui, cx: &ViewContext<'_, S>);
}

/// A layout view: `Fn(&mut Ui, Outlet, T1, …, T16)` where every `T` is a [`FromView`] extractor.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a layout view: `fn(&mut Ui, Outlet, extractors...)` with up to 16 view extractors",
    note = "views borrow the app state: `State<&T>` rather than `State<T>`, and `HistoryState<&T>`",
    note = "the data must be what the `load` returned: `&mut {D}` or `&{D}`",
    note = "closures need annotated arguments, e.g. `|ui: &mut Ui, nav: Nav| ...`",
    note = "see the `egui_kit::extract` module for the available extractors"
)]
pub trait LayoutView<T, S, D>: 'static {
    fn call(&self, ui: &mut Ui, outlet: Outlet<'_>, cx: &ViewContext<'_, S>);

    /// Whether the view takes its data as `&mut D`. If not, the views below can read it with
    /// [`LayoutData`](crate::extract::LayoutData).
    #[doc(hidden)]
    fn borrows_data_mut(&self) -> bool {
        true
    }
}

// Both `Fn` bounds are needed: the first infers the extractor types from the function, the
// second lets it take them with the lifetime of the frame (as bevy does for systems).
macro_rules! impl_views {
    ($($ty:ident),*) => {
        #[expect(
            clippy::allow_attributes,
            reason = "whether the lints fire depends on the arity, so `expect` would be unfulfilled for some"
        )]
        impl<F, S: 'static, D, $($ty,)*> View<($($ty,)*), S, D> for F
        where
            F: Fn(&mut Ui, $($ty),*) + 'static,
            F: for<'f> Fn(&mut Ui, $(<$ty as FromView<S, D>>::Item<'f>),*),
            $($ty: FromView<S, D>,)*
        {
            #[allow(non_snake_case, unused_variables)]
            fn call(&self, ui: &mut Ui, cx: &ViewContext<'_, S>) {
                #[allow(clippy::too_many_arguments)]
                fn call_inner<$($ty),*>(f: impl Fn(&mut Ui, $($ty),*), ui: &mut Ui, $($ty: $ty),*) {
                    f(ui, $($ty),*)
                }
                $($ty::prepare(cx);)*
                $(let $ty = $ty::from_view(cx);)*
                call_inner(self, ui, $($ty),*)
            }
        }

        #[expect(
            clippy::allow_attributes,
            reason = "whether the lints fire depends on the arity, so `expect` would be unfulfilled for some"
        )]
        impl<F, S: 'static, D, $($ty,)*> LayoutView<($($ty,)*), S, D> for F
        where
            F: Fn(&mut Ui, Outlet<'_>, $($ty),*) + 'static,
            F: for<'f> Fn(&mut Ui, Outlet<'_>, $(<$ty as FromView<S, D>>::Item<'f>),*),
            $($ty: FromView<S, D>,)*
        {
            #[allow(non_snake_case, unused_variables)]
            fn call(&self, ui: &mut Ui, outlet: Outlet<'_>, cx: &ViewContext<'_, S>) {
                #[allow(clippy::too_many_arguments)]
                fn call_inner<$($ty),*>(
                    f: impl Fn(&mut Ui, Outlet<'_>, $($ty),*),
                    ui: &mut Ui,
                    outlet: Outlet<'_>,
                    $($ty: $ty),*
                ) {
                    f(ui, outlet, $($ty),*)
                }
                $($ty::prepare(cx);)*
                $(let $ty = $ty::from_view(cx);)*
                call_inner(self, ui, outlet, $($ty),*)
            }

            fn borrows_data_mut(&self) -> bool {
                [$(<$ty as FromView<S, D>>::BORROWS_DATA_MUT),*].contains(&true)
            }
        }
    };
}

crate::all_tuples!(impl_views);

/// The slot a [`Layout`](crate::page::Layout) renders its child (a nested layout, a page or an error
/// page) into. Layout views get it as their second argument.
///
/// Show it with [`Outlet::show`], or animated with a [`ViewTransition`](crate::ViewTransition).
pub struct Outlet<'o> {
    pub(crate) inner: &'o mut dyn FnMut(&mut Ui),
    /// Remembers that this layout animates page changes.
    pub(crate) uses_transitions: &'o Cell<bool>,
    pub(crate) transition: Option<&'o mut dyn TransitionHandle>,
}

impl Outlet<'_> {
    pub fn show(&mut self, ui: &mut Ui) {
        (self.inner)(ui);
    }
}
