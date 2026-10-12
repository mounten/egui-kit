//! [`PersistentState`]: app-wide state that survives app restarts, like `localStorage`.

use core::any::type_name;
use core::ops::{Deref, DerefMut};
use std::fmt;

use egui::Id;
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::FromView;
use crate::page::ViewContext;

/// State that outlives the app, saved with egui's persisted memory.
///
/// eframe writes it to disk natively and to `localStorage` on the web. Like `localStorage`, there
/// is one value per type for the whole app, shared by every view that extracts it.
///
/// Use it for preferences: a theme, a sort order, a collapsed sidebar. It is created with
/// `T::default()`, read at the start of the view and written back when the view returns (the
/// last view to write wins), so keep it small. Outside of views, use
/// [`PersistentState::get`] and [`PersistentState::set`].
///
/// Saving needs eframe's `persistence` feature. Without it, the value still lives in egui's
/// memory, for as long as the app runs.
///
/// ```
/// use egui::Ui;
/// use egui_kit::extract::PersistentState;
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Clone, Default, Serialize, Deserialize)]
/// struct Preferences {
///     compact: bool,
/// }
///
/// fn view(ui: &mut Ui, mut preferences: PersistentState<Preferences>) {
///     ui.checkbox(&mut preferences.compact, "Compact");
/// }
/// ```
pub struct PersistentState<'f, T>
where
    T: Serialize + DeserializeOwned + Clone + Default + Send + Sync + 'static,
{
    value: Option<T>,
    ctx: &'f egui::Context,
}

impl<S: 'static, D, T> FromView<S, D> for PersistentState<'_, T>
where
    T: Serialize + DeserializeOwned + Clone + Default + Send + Sync + 'static,
{
    type Item<'f> = PersistentState<'f, T>;

    fn from_view<'f>(cx: &'f ViewContext<'_, S>) -> PersistentState<'f, T> {
        let ctx = &cx.frame().ctx;
        PersistentState {
            value: Some(Self::get(ctx)),
            ctx,
        }
    }
}

/// Outside of views, e.g. to apply a theme before the router draws.
impl<T> PersistentState<'_, T>
where
    T: Serialize + DeserializeOwned + Clone + Default + Send + Sync + 'static,
{
    /// The value.
    ///
    /// ```
    /// # use egui_kit::extract::PersistentState;
    /// # #[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
    /// # struct Preferences { dark: bool }
    /// fn apply_theme(ctx: &egui::Context) {
    ///     let preferences = PersistentState::<Preferences>::get(ctx);
    ///     ctx.set_theme(if preferences.dark { egui::Theme::Dark } else { egui::Theme::Light });
    /// }
    /// ```
    pub fn get(ctx: &egui::Context) -> T {
        ctx.data_mut(|data| data.get_persisted_mut_or_default::<T>(Self::id()).clone())
    }

    /// Replaces the value.
    pub fn set(ctx: &egui::Context, value: T) {
        ctx.data_mut(|data| data.insert_persisted(Self::id(), value));
    }

    /// Where the value is kept in egui's [memory](egui::Memory).
    pub fn id() -> Id {
        Id::new(("egui_kit::PersistentState", type_name::<T>()))
    }
}

impl<T> Deref for PersistentState<'_, T>
where
    T: Serialize + DeserializeOwned + Clone + Default + Send + Sync + 'static,
{
    type Target = T;

    fn deref(&self) -> &T {
        self.value.as_ref().expect("present until dropped")
    }
}

impl<T> DerefMut for PersistentState<'_, T>
where
    T: Serialize + DeserializeOwned + Clone + Default + Send + Sync + 'static,
{
    fn deref_mut(&mut self) -> &mut T {
        self.value.as_mut().expect("present until dropped")
    }
}

impl<T> Drop for PersistentState<'_, T>
where
    T: Serialize + DeserializeOwned + Clone + Default + Send + Sync + 'static,
{
    fn drop(&mut self) {
        if let Some(value) = self.value.take() {
            Self::set(self.ctx, value);
        }
    }
}

impl<T> fmt::Debug for PersistentState<'_, T>
where
    T: Serialize + DeserializeOwned + Clone + Default + Send + Sync + 'static + fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("PersistentState").field(&**self).finish()
    }
}
