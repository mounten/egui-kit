//! [`LayoutData`]: the data of a layout around the view.

use core::any::type_name;
use core::cell::Ref;
use core::fmt;
use core::ops::Deref;

use super::FromView;
use crate::page::ViewContext;

/// The data loaded by the nearest layout around the view that loaded a `T`: what
/// [`Parent`](super::Parent) is to `load`s, for views.
///
/// ```
/// use egui::Ui;
/// use egui_kit::Rejection;
/// use egui_kit::extract::LayoutData;
/// use egui_kit::page::{Layout, Outlet, Page};
///
/// struct Workspace {
///     name: String,
/// }
///
/// async fn load_workspace() -> Result<Workspace, Rejection> {
///     Ok(Workspace { name: "Physio".into() })
/// }
///
/// // Takes its data as `&Workspace`, so the views below can read it too.
/// fn workspace_layout(ui: &mut Ui, mut outlet: Outlet, workspace: &Workspace) {
///     ui.heading(&workspace.name);
///     outlet.show(ui);
/// }
///
/// fn settings(ui: &mut Ui, workspace: LayoutData<Workspace>) {
///     ui.label(format!("Settings of {}", workspace.name));
/// }
///
/// let router = egui_kit::Router::<()>::new()
///     .layout(Layout::with_load(load_workspace, workspace_layout))
///     .page("/settings", Page::new(settings));
/// ```
///
/// # Panics
///
/// When no layout around the view loaded a `T`, or when the one that did takes its data as
/// `&mut T` in its view: it can't be read while that view runs (and the views below run inside
/// it). Take it as `&T` to share it.
pub struct LayoutData<'f, T: 'static>(Ref<'f, T>);

impl<S: 'static, D, T: 'static> FromView<S, D> for LayoutData<'_, T> {
    type Item<'f> = LayoutData<'f, T>;

    fn from_view<'f>(cx: &'f ViewContext<'_, S>) -> LayoutData<'f, T> {
        let mut borrowed_mut = false;
        for loaded in cx.layouts().iter().rev().flatten() {
            let Ok(data) = loaded.data.try_borrow() else {
                borrowed_mut = true;
                continue;
            };
            if let Ok(data) = Ref::filter_map(data, |data| data.downcast_ref::<T>()) {
                return LayoutData(data);
            }
        }
        let hint = if borrowed_mut {
            " (a layout that takes its data as `&mut` can't share it: take it as `&` instead)"
        } else {
            ""
        };
        panic!(
            "egui-kit: no layout around the view loaded a `{}`{hint}",
            type_name::<T>()
        );
    }
}

impl<T> Deref for LayoutData<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T: fmt::Debug> fmt::Debug for LayoutData<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("LayoutData").field(&**self).finish()
    }
}
