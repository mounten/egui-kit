//! Polling futures from egui frames, outside of the router: the executor that drives loads,
//! queries and commands.
//!
//! No async runtime is needed: a future is polled when the app draws a frame, and wakes it up
//! with a repaint. Use it for async work that doesn't fit a load, query or command, e.g. before
//! the router exists, or in a widget that owns a file dialog.
//!
//! ```
//! use egui_kit::task::Task;
//!
//! struct Picker {
//!     task: Option<Task<Option<String>>>,
//!     picked: Option<String>,
//! }
//!
//! impl Picker {
//!     fn ui(&mut self, ui: &mut egui::Ui) {
//!         if let Some(task) = &mut self.task {
//!             ui.spinner();
//!             if let Some(picked) = task.poll(ui.ctx()) {
//!                 self.picked = picked;
//!                 self.task = None;
//!             }
//!         } else if ui.button("Pick a file").clicked() {
//!             self.task = Some(Task::new(async {
//!                 Some("photo.jpg".to_owned()) // e.g. `rfd::AsyncFileDialog::new().pick_file()`
//!             }));
//!         }
//!     }
//! }
//! ```

use core::fmt;
use core::future::Future;
use core::pin::Pin;
use core::task::Poll;

pub use crate::exec::poll_once;
use crate::page::LocalBoxFuture;

/// A future polled from egui frames: call [`Task::poll`] every frame until it returns the
/// output. Dropping it cancels the future.
pub struct Task<T> {
    future: Option<LocalBoxFuture<T>>,
}

impl<T: 'static> Task<T> {
    pub fn new(future: impl Future<Output = T> + 'static) -> Self {
        Self {
            future: Some(Box::pin(future)),
        }
    }
}

impl<T> Task<T> {
    /// Polls the future. Returns its output in the frame it completes, and `None` before and
    /// after that.
    pub fn poll(&mut self, ctx: &egui::Context) -> Option<T> {
        let future: Pin<&mut dyn Future<Output = T>> = self.future.as_mut()?.as_mut();
        match poll_once(ctx, future) {
            Poll::Ready(output) => {
                self.future = None;
                Some(output)
            }
            Poll::Pending => None,
        }
    }

    /// Whether the future completed, i.e. [`Task::poll`] returned its output.
    pub fn is_done(&self) -> bool {
        self.future.is_none()
    }
}

impl<T> fmt::Debug for Task<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Task")
            .field("done", &self.is_done())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use core::cell::Cell;
    use core::future::poll_fn;
    use core::task::Poll;
    use std::rc::Rc;

    use super::Task;

    #[test]
    fn returns_the_output_once() {
        let ctx = egui::Context::default();
        let open = Rc::new(Cell::new(false));
        let gate = Rc::clone(&open);
        let mut task = Task::new(poll_fn(move |_| {
            if gate.get() {
                Poll::Ready(7)
            } else {
                Poll::Pending
            }
        }));
        assert_eq!(task.poll(&ctx), None);
        assert!(!task.is_done());
        open.set(true);
        assert_eq!(task.poll(&ctx), Some(7));
        assert!(task.is_done());
        assert_eq!(task.poll(&ctx), None);
    }
}
