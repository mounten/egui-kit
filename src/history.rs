//! Where the current url lives: in memory (native) or in the browser's address bar (wasm).

#[cfg(target_arch = "wasm32")]
mod browser;
mod memory;

#[cfg(target_arch = "wasm32")]
pub use browser::{BrowserHistory, HistoryMode};
pub use memory::MemoryHistory;

/// Identifies one history entry, so the router can recognise entries it created
/// (e.g. by [shallow routing](crate::extract::Nav::push_state)) when the user goes back or forward.
///
/// Keys must be unique among the entries of a history, including entries left over from an
/// earlier session (a browser reload).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EntryKey(pub u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    /// An app-relative url: a path plus optional query, e.g. `/blog?page=2`.
    pub url: String,
    pub key: EntryKey,
}

/// A session history the router keeps in sync with the current page.
pub trait History {
    /// The current entry. Used to pick the initial route.
    fn current(&self) -> HistoryEntry;

    /// Adds a new entry and returns its key.
    fn push(&mut self, url: &str) -> EntryKey;

    /// Changes the url of the current entry, keeping its key.
    fn replace(&mut self, url: &str);

    /// Moves one entry back. The resulting entry is reported through [`History::poll`].
    fn back(&mut self);

    /// Moves one entry forward. The resulting entry is reported through [`History::poll`].
    fn forward(&mut self);

    fn can_go_back(&self) -> bool;

    fn can_go_forward(&self) -> bool;

    /// Returns the entry the history moved to by itself since the last call (back/forward,
    /// or the browser's buttons). Entries the history doesn't know (e.g. the user edited the
    /// url's `#hash`) must be given a fresh key.
    ///
    /// `ctx` can be stored to request a repaint when such a change happens outside a frame.
    fn poll(&mut self, ctx: &egui::Context) -> Option<HistoryEntry>;
}

/// `BrowserHistory` on wasm, [`MemoryHistory`] starting at `/` elsewhere.
pub fn default_history() -> impl History {
    #[cfg(target_arch = "wasm32")]
    {
        BrowserHistory::new(HistoryMode::Path)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        MemoryHistory::new("/")
    }
}
