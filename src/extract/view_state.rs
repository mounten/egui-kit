//! [`PageState`] and [`Snapshot`]: state owned by a view, for as long as the page (or layout) is
//! shown, and kept across back/forward for snapshots.

use core::any::{Any, TypeId, type_name};
use core::cell::RefCell;
use core::ops::{Deref, DerefMut};
use std::collections::HashMap;
use std::fmt;

use super::FromView;
use crate::page::ViewContext;

/// Values by type. A value is taken out while a view uses it and put back afterwards, so a view
/// can hold several types at once without borrowing the map.
#[derive(Default)]
pub(crate) struct TypeMap(RefCell<HashMap<TypeId, Option<Box<dyn Any>>>>);

impl TypeMap {
    fn take<T: 'static>(&self, init: impl FnOnce() -> T) -> Box<T> {
        let mut map = self.0.borrow_mut();
        let slot = map
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Some(Box::new(init())));
        let value = slot.take().unwrap_or_else(|| {
            panic!(
                "egui-kit: a view can only extract `{}` once",
                type_name::<T>()
            )
        });
        value.downcast().expect("stored by type")
    }

    fn put<T: 'static>(&self, value: Box<T>) {
        self.0.borrow_mut().insert(TypeId::of::<T>(), Some(value));
    }

    fn contains<T: 'static>(&self) -> bool {
        self.0.borrow().contains_key(&TypeId::of::<T>())
    }

    pub(crate) fn take_all(&self) -> Self {
        Self(RefCell::new(core::mem::take(&mut *self.0.borrow_mut())))
    }
}

/// The state of one page instance or layout: its [`PageState`]s and [`Snapshot`]s.
#[derive(Default)]
pub(crate) struct ViewStore {
    pub(crate) page: TypeMap,
    pub(crate) snapshot: TypeMap,
}

impl ViewStore {
    /// A fresh store that continues `left`'s snapshots (coming back with back/forward).
    pub(crate) fn restored(left: &Self) -> Self {
        Self {
            page: TypeMap::default(),
            snapshot: left.snapshot.take_all(),
        }
    }
}

/// Makes a view state from the view's data.
fn from_data<S, D: 'static, T: for<'d> From<&'d D>>(cx: &ViewContext<'_, S>) -> T {
    cx.peek_data(|data| {
        T::from(
            data.downcast_ref::<D>()
                .expect("egui-kit: the data does not match the view (this is a bug)"),
        )
    })
}

/// A view state type: a value taken out of its map while the view uses it.
macro_rules! view_state {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        pub struct $name<'f, T: 'static> {
            value: Option<Box<T>>,
            map: &'f TypeMap,
        }

        impl<T> Deref for $name<'_, T> {
            type Target = T;

            fn deref(&self) -> &T {
                self.value.as_ref().expect("present until dropped")
            }
        }

        impl<T> DerefMut for $name<'_, T> {
            fn deref_mut(&mut self) -> &mut T {
                self.value.as_mut().expect("present until dropped")
            }
        }

        impl<T> Drop for $name<'_, T> {
            fn drop(&mut self) {
                if let Some(value) = self.value.take() {
                    self.map.put(value);
                }
            }
        }

        impl<T: fmt::Debug> fmt::Debug for $name<'_, T> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_tuple(stringify!($name)).field(&**self).finish()
            }
        }
    };
}

/// The extractors of a view state, created with `T::default()` and from the view's data.
macro_rules! view_state_extractors {
    ($default:ident, $from:ident, $map:ident) => {
        impl<S: 'static, D, T: Default + 'static> FromView<S, D> for $default<'_, T> {
            type Item<'f> = $default<'f, T>;

            fn from_view<'f>(cx: &'f ViewContext<'_, S>) -> $default<'f, T> {
                let map = &cx.store().$map;
                $default {
                    value: Some(map.take(T::default)),
                    map,
                }
            }
        }

        impl<S: 'static, D: 'static, T> FromView<S, D> for $from<'_, T>
        where
            T: for<'d> From<&'d D> + 'static,
        {
            type Item<'f> = $from<'f, T>;

            // Before any extractor takes the data.
            fn prepare(cx: &ViewContext<'_, S>) {
                let map = &cx.store().$map;
                if !map.contains::<T>() {
                    map.put(Box::new(from_data::<S, D, T>(cx)));
                }
            }

            fn from_view<'f>(cx: &'f ViewContext<'_, S>) -> $from<'f, T> {
                let map = &cx.store().$map;
                $from {
                    value: Some(map.take(|| from_data::<S, D, T>(cx))),
                    map,
                }
            }
        }
    };
}

view_state!(
    /// State a view owns for as long as its page is shown.
    ///
    /// Created with `T::default()` the first time it is extracted, then kept from frame to frame:
    /// use it for what the user does on the page (a form's text, a toggle). Unlike the `load`'s
    /// data, it survives invalidations. It is dropped when the user navigates away, and starts
    /// over when they come back. For state that comes back with back/forward, use [`Snapshot`].
    ///
    /// In a page view, it belongs to the page instance, which shallow routing and invalidations
    /// keep. In a layout view, it belongs to the layout and lasts as long as the layout stays on
    /// screen, so it survives navigations between the pages below it.
    ///
    /// Each type is one value per view: use a dedicated type, not `String` or `bool`.
    ///
    /// ```
    /// use egui::Ui;
    /// use egui_kit::extract::PageState;
    ///
    /// #[derive(Default)]
    /// struct Draft {
    ///     title: String,
    /// }
    ///
    /// fn view(ui: &mut Ui, mut draft: PageState<Draft>) {
    ///     ui.text_edit_singleline(&mut draft.title);
    /// }
    /// ```
    ///
    /// To start from the `load`'s data, see [`PageStateFrom`].
    PageState
);

view_state!(
    /// Like [`PageState`], but created from the view's data (what its `load` returned) with
    /// `T::from(&data)`, the first time it is extracted.
    ///
    /// ```
    /// use egui::Ui;
    /// use egui_kit::extract::PageStateFrom;
    ///
    /// struct Post {
    ///     title: String,
    /// }
    ///
    /// struct Form {
    ///     title: String,
    /// }
    ///
    /// impl From<&Post> for Form {
    ///     fn from(post: &Post) -> Self {
    ///         Self { title: post.title.clone() }
    ///     }
    /// }
    ///
    /// fn edit(ui: &mut Ui, post: &Post, mut form: PageStateFrom<Form>) {
    ///     ui.label(format!("Editing {}", post.title));
    ///     ui.text_edit_singleline(&mut form.title);
    /// }
    /// ```
    ///
    /// It shares its value with a [`PageState`] of the same type.
    PageStateFrom
);

view_state!(
    /// Like [`PageState`], but kept when the user navigates away and restored when they come
    /// back with back/forward.
    ///
    /// Use it for UI state worth getting back, such as a half-typed search or a draft. A new
    /// navigation to the page (a link, `goto`) starts over with `T::default()`. Snapshots are kept
    /// in memory, so a browser reload or an app restart loses them; see
    /// [`PersistentState`](super::PersistentState) for state that survives those.
    ///
    /// ```
    /// use egui::Ui;
    /// use egui_kit::extract::Snapshot;
    ///
    /// #[derive(Default)]
    /// struct Search {
    ///     text: String,
    /// }
    ///
    /// fn view(ui: &mut Ui, mut search: Snapshot<Search>) {
    ///     ui.text_edit_singleline(&mut search.text);
    /// }
    /// ```
    Snapshot
);

view_state!(
    /// Like [`Snapshot`], but created from the view's data (what its `load` returned) with
    /// `T::from(&data)`, like a [`PageStateFrom`]. A snapshot that comes back with back/forward
    /// keeps its value.
    SnapshotFrom
);

view_state_extractors!(PageState, PageStateFrom, page);
view_state_extractors!(Snapshot, SnapshotFrom, snapshot);
