//! [`HistoryState`]: the shallow-routing state of the current history entry.

use core::any::Any;

use super::FromView;
use crate::page::ViewContext;

/// The state attached to the current history entry, if it is a `T`.
///
/// It is set with [`Nav::push_state`](super::Nav::push_state) or
/// [`Nav::replace_state`](super::Nav::replace_state). Borrowed: extract it as `HistoryState<&T>`.
///
/// It belongs to the history entry, not to the page: back and forward between the entries of a
/// page swap it, and coming back to an entry of a page that was left restores it. A new
/// navigation starts without it, and a browser reload loses it.
///
/// For state the view owns and mutates, see [`PageState`](super::PageState) and
/// [`Snapshot`](super::Snapshot).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoryState<T>(pub Option<T>);

impl<S: 'static, D, T: Any> FromView<S, D> for HistoryState<&'_ T> {
    type Item<'f> = HistoryState<&'f T>;

    fn from_view<'f>(cx: &'f ViewContext<'_, S>) -> HistoryState<&'f T> {
        HistoryState(
            cx.frame()
                .history_state
                .and_then(|state| state.downcast_ref()),
        )
    }
}
