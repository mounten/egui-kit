//! A tiny executor: navigation futures are polled from the egui frame and wake egui up.

use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll, Waker};
use std::sync::Arc;
use std::task::Wake;

struct RepaintWaker(egui::Context);

impl Wake for RepaintWaker {
    fn wake(self: Arc<Self>) {
        self.0.request_repaint();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.request_repaint();
    }
}

/// Polls `future` once, with a waker that requests a repaint of `ctx`: the app draws a frame
/// (and can poll again) when the future makes progress.
pub(crate) fn poll_once<T>(
    ctx: &egui::Context,
    future: Pin<&mut dyn Future<Output = T>>,
) -> Poll<T> {
    let waker = Waker::from(Arc::new(RepaintWaker(ctx.clone())));
    future.poll(&mut Context::from_waker(&waker))
}
