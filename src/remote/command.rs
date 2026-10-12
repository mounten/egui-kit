//! [`Command`]: async mutations.

use core::cell::{Ref, RefCell};
use core::fmt;
use core::future::Future;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use super::runtime::{self, MaybeSend};
use super::{lock, with_registry};

/// Where a run puts its result.
type Slot<T, E> = Arc<Mutex<Option<Result<T, E>>>>;

struct State<T, E> {
    running: Vec<Slot<T, E>>,
    result: Option<Result<T, E>>,
}

/// An async mutation: [run](Self::run) it with a future, and see whether it's
/// [pending](Self::is_pending) and how it ended.
///
/// Create one with [`command`] (or `Default`), keep it where the views can use it and run it
/// as often as needed. Clones share its state. A run continues when the command is dropped.
/// See the [module docs](crate::remote).
///
/// ```
/// # use egui_kit::remote::Command;
/// # async fn save(_text: String) -> Result<(), String> { Ok(()) }
/// #[derive(Default)]
/// struct Editor {
///     text: String,
///     save: Command<(), String>,
/// }
///
/// fn ui(ui: &mut egui::Ui, editor: &mut Editor) {
///     ui.text_edit_multiline(&mut editor.text);
///     if ui.add_enabled(!editor.save.is_pending(), egui::Button::new("Save")).clicked() {
///         editor.save.run(save(editor.text.clone()));
///     }
///     match editor.save.take_result() {
///         Some(Ok(())) => { /* close the editor */ }
///         Some(Err(error)) => { /* show the error in a toast */ }
///         None => {}
///     }
/// }
/// ```
pub struct Command<T, E> {
    state: Rc<RefCell<State<T, E>>>,
    refreshes: Vec<String>,
}

/// A [`Command`].
pub fn command<T: MaybeSend + 'static, E: MaybeSend + 'static>() -> Command<T, E> {
    Command::default()
}

impl<T: MaybeSend + 'static, E: MaybeSend + 'static> Command<T, E> {
    /// Once a run succeeded, reruns the loads and queries that depend on `key`, like
    /// [`Nav::invalidate`](crate::extract::Nav::invalidate).
    #[must_use]
    pub fn refreshes(mut self, key: impl Into<String>) -> Self {
        self.refreshes.push(key.into());
        self
    }

    /// Runs `future`, forgetting how the last run ended.
    pub fn run(&self, future: impl Future<Output = Result<T, E>> + MaybeSend + 'static) {
        let slot: Slot<T, E> = Arc::default();
        let refreshes = self.refreshes.clone();
        let (invalidated, ctx) =
            with_registry(|registry| (Arc::clone(&registry.invalidated), registry.ctx.clone()));
        runtime::spawn_detached({
            let slot = Arc::clone(&slot);
            async move {
                let result = future.await;
                if result.is_ok() {
                    lock(&invalidated).extend(refreshes);
                }
                *lock(&slot) = Some(result);
                if let Some(ctx) = ctx {
                    ctx.request_repaint();
                }
            }
        });
        let mut state = self.state.borrow_mut();
        state.result = None;
        state.running.push(slot);
    }

    /// Whether a run is in progress.
    pub fn is_pending(&self) -> bool {
        !self.sync().running.is_empty()
    }

    /// How the last run ended, until the next one starts or it is [taken](Self::take_result).
    ///
    /// Borrows the command: drop it before running the command again.
    pub fn result(&self) -> Option<Ref<'_, Result<T, E>>> {
        Ref::filter_map(self.sync(), |state| state.result.as_ref()).ok()
    }

    /// What the last run returned, if it succeeded. See [`result`](Self::result).
    pub fn output(&self) -> Option<Ref<'_, T>> {
        Ref::filter_map(self.sync(), |state| state.result.as_ref()?.as_ref().ok()).ok()
    }

    /// Why the last run failed, if it did. See [`result`](Self::result).
    pub fn error(&self) -> Option<Ref<'_, E>> {
        Ref::filter_map(self.sync(), |state| state.result.as_ref()?.as_ref().err()).ok()
    }

    /// How the last run ended, taken: to act on it once (close a dialog, navigate to what it
    /// created).
    pub fn take_result(&self) -> Option<Result<T, E>> {
        drop(self.sync());
        self.state.borrow_mut().result.take()
    }

    /// Picks up the finished runs.
    fn sync(&self) -> Ref<'_, State<T, E>> {
        let mut state = self.state.borrow_mut();
        let State { running, result } = &mut *state;
        running.retain(|slot| match lock(slot).take() {
            Some(done) => {
                *result = Some(done);
                false
            }
            None => true,
        });
        drop(state);
        self.state.borrow()
    }
}

impl<T, E> Default for Command<T, E> {
    fn default() -> Self {
        Self {
            state: Rc::new(RefCell::new(State {
                running: Vec::new(),
                result: None,
            })),
            refreshes: Vec::new(),
        }
    }
}

impl<T, E> Clone for Command<T, E> {
    fn clone(&self) -> Self {
        Self {
            state: Rc::clone(&self.state),
            refreshes: self.refreshes.clone(),
        }
    }
}

impl<T: fmt::Debug, E: fmt::Debug> fmt::Debug for Command<T, E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.state.borrow();
        f.debug_struct("Command")
            .field("pending", &!state.running.is_empty())
            .field("result", &state.result)
            .field("refreshes", &self.refreshes)
            .finish()
    }
}
