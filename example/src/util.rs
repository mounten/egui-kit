use core::time::Duration;

/// Simulates network latency so the loading states are visible.
///
/// egui-kit polls `load` futures itself and doesn't need an async runtime, so this only needs
/// to wake the task: a thread on native, `setTimeout` on the web.
#[cfg(not(target_arch = "wasm32"))]
pub async fn sleep(duration: Duration) {
    use core::future::poll_fn;
    use core::task::{Poll, Waker};
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Timer {
        done: bool,
        waker: Option<Waker>,
    }

    let timer = Arc::new(Mutex::new(Timer::default()));
    let thread_timer = Arc::clone(&timer);
    std::thread::spawn(move || {
        std::thread::sleep(duration);
        let mut timer = thread_timer.lock().expect("timer mutex poisoned");
        timer.done = true;
        if let Some(waker) = timer.waker.take() {
            waker.wake();
        }
    });

    poll_fn(|cx| {
        let mut timer = timer.lock().expect("timer mutex poisoned");
        if timer.done {
            Poll::Ready(())
        } else {
            timer.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    })
    .await;
}

#[cfg(target_arch = "wasm32")]
pub async fn sleep(duration: Duration) {
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        web_sys::window()
            .expect("no window")
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                &resolve,
                duration.as_millis() as i32,
            )
            .expect("setTimeout failed");
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}
