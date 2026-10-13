//! [`Live`]: a query made from inputs that change.

use core::fmt;
use core::hash::Hash;
use core::time::Duration;
use std::rc::Rc;

use web_time::Instant;

use super::query::{Query, QueryState};
use super::runtime::MaybeSend;
use super::{context, hash};

/// A [`Query`] made from inputs that change, like a date range or a search: made again when
/// they do.
///
/// A query is made from its inputs, and a new one fetches on its first read, so a view can't
/// make one from its inputs every frame. Keep a `Live` in the view's state instead, and
/// [`update`](Self::update) it every frame with the inputs: it makes a new query only when they
/// changed. The inputs are only hashed (like a [key](Query::key)), so pass them borrowed.
///
/// Optionally, it [keeps showing the previous value](Self::keep_previous) while the query for
/// new inputs loads, so that a list doesn't flicker back to a spinner, and [waits for the
/// inputs to settle](Self::debounce) before it makes the query, so that a search doesn't ask for
/// every letter typed.
///
/// ```
/// use core::time::Duration;
///
/// use egui_kit::remote::{self, Live, Query};
///
/// # #[derive(Clone)]
/// # struct Api;
/// # impl Api {
/// #     async fn search(&self, _text: &str) -> Result<Vec<String>, String> { Ok(Vec::new()) }
/// # }
/// fn search(api: &Api, text: &str) -> Query<Vec<String>, String> {
///     let (api, owned) = (api.clone(), text.to_owned());
///     remote::query(move || {
///         let (api, text) = (api.clone(), owned.clone());
///         async move { api.search(&text).await }
///     })
///     .key(("search", text))
/// }
///
/// struct Search {
///     api: Api,
///     text: String,
///     results: Live<Vec<String>, String>,
/// }
///
/// impl Search {
///     fn new(api: Api) -> Self {
///         Self {
///             api,
///             text: String::new(),
///             results: remote::live()
///                 // Ask once the text didn't change for a moment…
///                 .debounce(Duration::from_millis(300))
///                 // …and show the last results until the new ones arrive.
///                 .keep_previous(),
///         }
///     }
///
///     fn ui(&mut self, ui: &mut egui::Ui) {
///         ui.text_edit_singleline(&mut self.text);
///         let results = self
///             .results
///             .update(&self.text, || search(&self.api, &self.text));
///         if let Some(names) = results.data() {
///             // Dimmed while they're the results of an earlier text.
///             ui.add_enabled_ui(!results.is_previous(), |ui| {
///                 for name in names.iter() {
///                     ui.label(name);
///                 }
///             });
///         }
///     }
/// }
/// ```
///
/// See [Queries from changing inputs](crate::remote#queries-from-changing-inputs).
pub struct Live<T, E> {
    /// The query made last, and the hash of its inputs.
    current: Option<(u64, Query<T, E>)>,
    /// The value of an earlier query, until `current` has an answer (with `keep_previous`).
    previous: Option<Rc<T>>,
    keep_previous: bool,
    debounce: Option<Duration>,
    /// Inputs seen but not made into a query yet, and since when (with `debounce`).
    wanted: Option<(u64, Instant)>,
}

/// A [`Live`].
pub fn live<T: MaybeSend + 'static, E: MaybeSend + 'static>() -> Live<T, E> {
    Live::default()
}

impl<T, E> Default for Live<T, E> {
    fn default() -> Self {
        Self {
            current: None,
            previous: None,
            keep_previous: false,
            debounce: None,
            wanted: None,
        }
    }
}

impl<T: MaybeSend + 'static, E: MaybeSend + 'static> Live<T, E> {
    /// While the query for new inputs has no answer yet, shows the last value of the one
    /// before: [`state`](Self::state) is `Ready` with it, and [`is_previous`](Self::is_previous)
    /// says so. Off by default, so that a view shows `Loading` for new inputs.
    #[must_use]
    pub fn keep_previous(mut self) -> Self {
        self.keep_previous = true;
        self
    }

    /// Makes the query only once the inputs didn't change for `after`, the first ones included.
    /// Meanwhile, it shows the query made before (and [`is_previous`](Self::is_previous)).
    #[must_use]
    pub fn debounce(mut self, after: Duration) -> Self {
        self.debounce = Some(after);
        self
    }

    /// Call it every frame with the inputs: makes the query with `make` when they differ from
    /// the last query's (once they [settled](Self::debounce)), and keeps the one made before
    /// otherwise. Returns `self`, to read it.
    ///
    /// The inputs are hashed, so they may be borrowed: `(&from, &to)`. `make` should key the
    /// query with them if it's to be [cached](Query::key).
    pub fn update(&mut self, inputs: impl Hash, make: impl FnOnce() -> Query<T, E>) -> &Self {
        let inputs = hash(inputs);
        // The new query answered: the previous value isn't needed anymore.
        if let Some((_, query)) = &self.current
            && self.previous.is_some()
        {
            let (value, failed) = query.peek();
            if value.is_some() || failed {
                self.previous = None;
            }
        }
        if self
            .current
            .as_ref()
            .is_some_and(|(made, _)| *made == inputs)
        {
            self.wanted = None;
            return self;
        }
        if let Some(after) = self.debounce {
            let now = Instant::now();
            let since = match self.wanted {
                Some((wanted, since)) if wanted == inputs => since,
                _ => {
                    self.wanted = Some((inputs, now));
                    now
                }
            };
            let left = after.saturating_sub(now.saturating_duration_since(since));
            if !left.is_zero() {
                if let Some(ctx) = context() {
                    ctx.request_repaint_after(left);
                }
                return self;
            }
        }
        self.wanted = None;
        let query = make();
        // Peeked, not read: the old query's fetch is aborted, not started.
        if let Some((_, old)) = self.current.replace((inputs, query))
            && self.keep_previous
            && let Some(value) = old.peek().0
        {
            self.previous = Some(value);
        }
        self
    }

    /// Drops the query, for when there's nothing to ask: it's `Loading` until the next
    /// [`update`](Self::update).
    pub fn clear(&mut self) {
        self.current = None;
        self.previous = None;
        self.wanted = None;
    }

    /// The query made last, if any.
    pub fn query(&self) -> Option<&Query<T, E>> {
        self.current.as_ref().map(|(_, query)| query)
    }

    /// What to show, like [`Query::state`], with the previous value while the query loads if
    /// [kept](Self::keep_previous). `Loading` while no query was made.
    pub fn state(&self) -> QueryState<T, E> {
        let Some(query) = self.query() else {
            return QueryState::Loading;
        };
        match (query.state(), &self.previous) {
            (QueryState::Loading, Some(previous)) => QueryState::Ready(Rc::clone(previous)),
            (state, _) => state,
        }
    }

    /// The query's value, like [`Query::data`], or the previous value while it loads if
    /// [kept](Self::keep_previous).
    pub fn data(&self) -> Option<Rc<T>> {
        let query = self.query()?;
        query.data().or_else(|| {
            query
                .error()
                .is_none()
                .then(|| self.previous.clone())
                .flatten()
        })
    }

    /// Why the query's last fetch failed.
    pub fn error(&self) -> Option<Rc<E>> {
        self.query()?.error()
    }

    /// Whether the query is fetching, see [`Query::is_fetching`].
    pub fn is_fetching(&self) -> bool {
        self.query().is_some_and(Query::is_fetching)
    }

    /// Whether what it shows isn't about the last [`update`](Self::update)'s inputs yet: a
    /// [debounce](Self::debounce) is waiting, or the [previous value](Self::keep_previous) is
    /// shown while the new query loads.
    pub fn is_previous(&self) -> bool {
        self.wanted.is_some()
            || self.previous.is_some()
                && self
                    .query()
                    .is_some_and(|query| query.data().is_none() && query.error().is_none())
    }
}

impl<T: fmt::Debug, E: fmt::Debug> fmt::Debug for Live<T, E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Live")
            .field("query", &self.current.as_ref().map(|(_, query)| query))
            .field("previous", &self.previous)
            .field("waiting", &self.wanted.is_some())
            .finish_non_exhaustive()
    }
}
