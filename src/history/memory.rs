//! [`MemoryHistory`]: a history stack in memory.

use super::{EntryKey, History, HistoryEntry};

/// An in-memory history stack, used on native targets and in tests.
#[derive(Debug, Clone)]
pub struct MemoryHistory {
    entries: Vec<HistoryEntry>,
    index: usize,
    next_key: u64,
    moved: bool,
}

impl MemoryHistory {
    pub fn new(initial_url: impl Into<String>) -> Self {
        Self {
            entries: vec![HistoryEntry {
                url: initial_url.into(),
                key: EntryKey(0),
            }],
            index: 0,
            next_key: 1,
            moved: false,
        }
    }

    pub fn entries(&self) -> &[HistoryEntry] {
        &self.entries
    }
}

impl History for MemoryHistory {
    fn current(&self) -> HistoryEntry {
        self.entries[self.index].clone()
    }

    fn push(&mut self, url: &str) -> EntryKey {
        let key = EntryKey(self.next_key);
        self.next_key += 1;
        self.entries.truncate(self.index + 1);
        self.entries.push(HistoryEntry {
            url: url.to_owned(),
            key,
        });
        self.index += 1;
        key
    }

    fn replace(&mut self, url: &str) {
        self.entries[self.index].url = url.to_owned();
    }

    fn back(&mut self) {
        if self.can_go_back() {
            self.index -= 1;
            self.moved = true;
        }
    }

    fn forward(&mut self) {
        if self.can_go_forward() {
            self.index += 1;
            self.moved = true;
        }
    }

    fn can_go_back(&self) -> bool {
        self.index > 0
    }

    fn can_go_forward(&self) -> bool {
        self.index + 1 < self.entries.len()
    }

    fn poll(&mut self, _ctx: &egui::Context) -> Option<HistoryEntry> {
        core::mem::take(&mut self.moved).then(|| self.current())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn back_and_forward() {
        let ctx = egui::Context::default();
        let mut h = MemoryHistory::new("/");
        let a = h.push("/a");
        h.push("/b");
        h.back();
        h.back();
        assert_eq!(h.poll(&ctx).map(|e| e.url).as_deref(), Some("/"));
        assert_eq!(h.poll(&ctx), None);
        h.forward();
        assert_eq!(
            h.poll(&ctx),
            Some(HistoryEntry {
                url: "/a".into(),
                key: a
            })
        );
        h.replace("/a2");
        assert_eq!(h.current().key, a);
        h.push("/c");
        assert!(!h.can_go_forward());
        let urls: Vec<_> = h.entries().iter().map(|e| e.url.as_str()).collect();
        assert_eq!(urls, ["/", "/a2", "/c"]);
    }
}
