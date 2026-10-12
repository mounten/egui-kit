//! Where queries and commands run: a Tokio runtime natively, the browser's event loop on the web.

use core::future::Future;
use core::pin::Pin;

#[cfg(not(target_arch = "wasm32"))]
pub use native::*;
#[cfg(target_arch = "wasm32")]
pub use web::*;

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use core::future::Future;
    use std::sync::OnceLock;

    use tokio::runtime::{EnterGuard, Handle, Runtime};

    /// `Send` natively, where futures run on the threads of a Tokio runtime, and nothing on the
    /// web, where they run on the browser's event loop.
    pub trait MaybeSend: Send {}

    impl<T: Send + ?Sized> MaybeSend for T {}

    /// The runtime used when the app didn't enter one.
    fn runtime() -> &'static Runtime {
        static RUNTIME: OnceLock<Runtime> = OnceLock::new();
        RUNTIME.get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .thread_name("egui-kit")
                .build()
                .expect("egui-kit: failed to start the Tokio runtime")
        })
    }

    fn handle() -> Handle {
        Handle::try_current().unwrap_or_else(|_| runtime().handle().clone())
    }

    /// Enters egui-kit's runtime, unless the app entered one of its own.
    pub(crate) fn enter() -> Option<EnterGuard<'static>> {
        Handle::try_current().is_err().then(|| runtime().enter())
    }

    /// Aborts its task when dropped.
    pub(crate) struct Abort(tokio::task::AbortHandle);

    impl Drop for Abort {
        fn drop(&mut self) {
            self.0.abort();
        }
    }

    pub(crate) fn spawn(future: impl Future<Output = ()> + Send + 'static) -> Abort {
        Abort(handle().spawn(future).abort_handle())
    }

    pub(crate) fn spawn_detached(future: impl Future<Output = ()> + Send + 'static) {
        drop(handle().spawn(future));
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    use core::cell::{Cell, RefCell};
    use core::future::{Future, poll_fn};
    use core::task::{Poll, Waker};
    use std::rc::Rc;

    /// `Send` natively, where futures run on the threads of a Tokio runtime, and nothing on the
    /// web, where they run on the browser's event loop.
    pub trait MaybeSend {}

    impl<T: ?Sized> MaybeSend for T {}

    /// The browser needs no runtime.
    pub(crate) fn enter() -> Option<()> {
        None
    }

    #[derive(Default)]
    struct AbortState {
        aborted: Cell<bool>,
        waker: RefCell<Option<Waker>>,
    }

    /// Aborts its task when dropped.
    pub(crate) struct Abort(Rc<AbortState>);

    impl Drop for Abort {
        fn drop(&mut self) {
            self.0.aborted.set(true);
            if let Some(waker) = self.0.waker.take() {
                waker.wake();
            }
        }
    }

    pub(crate) fn spawn(future: impl Future<Output = ()> + 'static) -> Abort {
        let state = Rc::new(AbortState::default());
        let task = Rc::clone(&state);
        wasm_bindgen_futures::spawn_local(async move {
            let mut future = core::pin::pin!(future);
            poll_fn(|cx| {
                if task.aborted.get() {
                    return Poll::Ready(());
                }
                let poll = future.as_mut().poll(cx);
                if poll.is_pending() {
                    task.waker.replace(Some(cx.waker().clone()));
                }
                poll
            })
            .await;
        });
        Abort(state)
    }

    pub(crate) fn spawn_detached(future: impl Future<Output = ()> + 'static) {
        wasm_bindgen_futures::spawn_local(future);
    }
}

/// A boxed future that can be spawned.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;
#[cfg(target_arch = "wasm32")]
pub(crate) type BoxFuture<T> = Pin<Box<dyn Future<Output = T>>>;
