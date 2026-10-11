//! [`BrowserHistory`]: the browser's history and address bar, on wasm.

use core::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::JsCast as _;
use wasm_bindgen::JsValue;
use wasm_bindgen::prelude::Closure;

use super::{EntryKey, History, HistoryEntry};

/// How app urls are stored in the browser's url.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryMode {
    /// `https://example.com/blog/post`. Needs the server to serve the app for every path.
    Path,
    /// Like [`HistoryMode::Path`], for an app served below `base`, e.g. `/my-app`.
    PathWithBase(String),
    /// `https://example.com/#/blog/post`. Works with any static file server.
    Hash,
}

/// What we store in `history.state` of every entry: its position (to know whether we can go
/// back/forward) and its key.
#[derive(Clone, Copy)]
struct EntryState {
    index: u32,
    key: EntryKey,
}

impl EntryState {
    fn read(value: &JsValue) -> Option<Self> {
        let array = value.dyn_ref::<js_sys::Array>()?;
        Some(Self {
            index: array.get(0).as_f64()? as u32,
            key: EntryKey(array.get(1).as_f64()? as u64),
        })
    }

    fn to_js(self) -> JsValue {
        js_sys::Array::of2(&self.index.into(), &(self.key.0 as f64).into()).into()
    }
}

/// Random, so keys of entries from before a reload don't collide with new ones.
fn fresh_key() -> EntryKey {
    const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
    EntryKey((js_sys::Math::random() * MAX_SAFE_INTEGER) as u64)
}

struct Shared {
    popped: Option<HistoryEntry>,
    ctx: Option<egui::Context>,
    current: EntryState,
    max_index: u32,
}

/// Keeps the browser's address bar and back/forward buttons in sync with the router,
/// using `history.pushState` and the `popstate` event.
pub struct BrowserHistory {
    mode: HistoryMode,
    shared: Rc<RefCell<Shared>>,
    listener: Closure<dyn FnMut(web_sys::PopStateEvent)>,
}

impl BrowserHistory {
    pub fn new(mode: HistoryMode) -> Self {
        let current = history()
            .state()
            .ok()
            .and_then(|state| EntryState::read(&state))
            .unwrap_or_else(|| EntryState {
                index: 0,
                key: fresh_key(),
            });
        let _ = history().replace_state(&current.to_js(), "");
        let shared = Rc::new(RefCell::new(Shared {
            popped: None,
            ctx: None,
            current,
            max_index: current.index,
        }));

        let listener = {
            let shared = Rc::clone(&shared);
            let mode = mode.clone();
            Closure::<dyn FnMut(_)>::new(move |event: web_sys::PopStateEvent| {
                let mut shared = shared.borrow_mut();
                // Entries created outside of the router (e.g. by editing a `#hash`) have no state.
                let entry = EntryState::read(&event.state()).unwrap_or_else(|| {
                    let entry = EntryState {
                        index: shared.current.index + 1,
                        key: fresh_key(),
                    };
                    let _ = history().replace_state(&entry.to_js(), "");
                    entry
                });
                shared.current = entry;
                shared.max_index = shared.max_index.max(entry.index);
                shared.popped = Some(HistoryEntry {
                    url: read_location(&mode),
                    key: entry.key,
                });
                if let Some(ctx) = &shared.ctx {
                    ctx.request_repaint();
                }
            })
        };
        window()
            .add_event_listener_with_callback("popstate", listener.as_ref().unchecked_ref())
            .expect("failed to listen to popstate");

        Self {
            mode,
            shared,
            listener,
        }
    }

    fn href(&self, url: &str) -> String {
        match &self.mode {
            HistoryMode::Path => url.to_owned(),
            HistoryMode::PathWithBase(base) => format!("{}{url}", base.trim_end_matches('/')),
            HistoryMode::Hash => {
                let location = window().location();
                let path = location.pathname().unwrap_or_default();
                let search = location.search().unwrap_or_default();
                format!("{path}{search}#{url}")
            }
        }
    }
}

impl Drop for BrowserHistory {
    fn drop(&mut self) {
        let _ = window().remove_event_listener_with_callback(
            "popstate",
            self.listener.as_ref().unchecked_ref(),
        );
    }
}

impl History for BrowserHistory {
    fn current(&self) -> HistoryEntry {
        HistoryEntry {
            url: read_location(&self.mode),
            key: self.shared.borrow().current.key,
        }
    }

    fn push(&mut self, url: &str) -> EntryKey {
        let mut shared = self.shared.borrow_mut();
        shared.current = EntryState {
            index: shared.current.index + 1,
            key: fresh_key(),
        };
        shared.max_index = shared.current.index;
        let _ = history().push_state_with_url(&shared.current.to_js(), "", Some(&self.href(url)));
        shared.current.key
    }

    fn replace(&mut self, url: &str) {
        let current = self.shared.borrow().current;
        let _ = history().replace_state_with_url(&current.to_js(), "", Some(&self.href(url)));
    }

    fn back(&mut self) {
        let _ = history().back();
    }

    fn forward(&mut self) {
        let _ = history().forward();
    }

    fn can_go_back(&self) -> bool {
        self.shared.borrow().current.index > 0
    }

    fn can_go_forward(&self) -> bool {
        let shared = self.shared.borrow();
        shared.current.index < shared.max_index
    }

    fn poll(&mut self, ctx: &egui::Context) -> Option<HistoryEntry> {
        let mut shared = self.shared.borrow_mut();
        if shared.ctx.is_none() {
            shared.ctx = Some(ctx.clone());
        }
        shared.popped.take()
    }
}

fn read_location(mode: &HistoryMode) -> String {
    let location = window().location();
    match mode {
        HistoryMode::Hash => {
            let hash = location.hash().unwrap_or_default();
            match hash.trim_start_matches('#') {
                "" => "/".to_owned(),
                url => url.to_owned(),
            }
        }
        HistoryMode::Path | HistoryMode::PathWithBase(_) => {
            let path = location.pathname().unwrap_or_default();
            let path = match mode {
                HistoryMode::PathWithBase(base) => path
                    .strip_prefix(base.trim_end_matches('/'))
                    .unwrap_or(&path)
                    .to_owned(),
                _ => path,
            };
            format!("{path}{}", location.search().unwrap_or_default())
        }
    }
}

fn window() -> web_sys::Window {
    web_sys::window().expect("no window")
}

fn history() -> web_sys::History {
    window().history().expect("no history")
}
