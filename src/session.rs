//! What the router remembers about each history entry (which page instance it belongs to, its
//! history state and its scroll position) and about the page instances that were left (their
//! snapshots).
//!
//! Kept in memory, so a browser reload forgets it.

use core::any::Any;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use crate::extract::ViewStore;
use crate::history::EntryKey;
use crate::navigation::Direction;
use crate::page::NodeId;

/// How many left page instances keep their snapshots; the oldest are forgotten beyond.
const MAX_VISITS: usize = 64;

pub(crate) struct EntryMeta {
    /// The page instance shown in this entry; shallow entries share their page's.
    pub(crate) page: u64,
    /// Increases with every new entry, to tell back from forward.
    seq: u64,
    pub(crate) state: Option<Rc<dyn Any>>,
    pub(crate) scroll: Option<f32>,
}

/// The view stores of a page instance that was left, for its snapshots.
pub(crate) struct Visit {
    /// The page that was shown (`None` for an error page).
    pub(crate) node: Option<NodeId>,
    pub(crate) page: Rc<ViewStore>,
    /// The layouts that were left along with the page.
    pub(crate) layouts: Vec<(NodeId, Rc<ViewStore>)>,
}

#[derive(Default)]
pub(crate) struct Session {
    entries: HashMap<EntryKey, EntryMeta>,
    next_seq: u64,
    /// By page instance id, oldest first.
    visits: VecDeque<(u64, Visit)>,
}

impl Session {
    pub(crate) fn get(&self, key: EntryKey) -> Option<&EntryMeta> {
        self.entries.get(&key)
    }

    pub(crate) fn get_mut(&mut self, key: EntryKey) -> Option<&mut EntryMeta> {
        self.entries.get_mut(&key)
    }

    /// Records that `key` shows `page` with `state`. Entries seen for the first time count as
    /// the newest.
    pub(crate) fn record(&mut self, key: EntryKey, page: u64, state: Option<Rc<dyn Any>>) {
        if let Some(meta) = self.entries.get_mut(&key) {
            meta.page = page;
            meta.state = state;
        } else {
            self.next_seq += 1;
            self.entries.insert(
                key,
                EntryMeta {
                    page,
                    seq: self.next_seq,
                    state,
                    scroll: None,
                },
            );
        }
    }

    /// Remembers the view stores of page instance `page`, which is being left.
    pub(crate) fn save_visit(&mut self, page: u64, visit: Visit) {
        self.visits.retain(|(id, _)| *id != page);
        if self.visits.len() == MAX_VISITS {
            self.visits.pop_front();
        }
        self.visits.push_back((page, visit));
    }

    /// The view stores of page instance `page`, to restore its snapshots, if it showed the page
    /// `node` (a shallow entry's url may load another page).
    pub(crate) fn take_visit(&mut self, page: u64, node: Option<NodeId>) -> Option<Visit> {
        let index = self
            .visits
            .iter()
            .position(|(id, visit)| *id == page && node.is_some() && visit.node == node)?;
        self.visits.remove(index).map(|(_, visit)| visit)
    }

    /// Where `to` is relative to `from` in the history.
    pub(crate) fn direction(&self, from: EntryKey, to: EntryKey) -> Direction {
        match (self.entries.get(&from), self.entries.get(&to)) {
            (Some(from), Some(to)) if to.seq > from.seq => Direction::Forward,
            (Some(from), Some(to)) if to.seq < from.seq => Direction::Back,
            _ => Direction::None,
        }
    }
}
