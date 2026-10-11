//! Starting, blocking and committing navigations.

use super::*;
use crate::error::{Rejection, RouteError};
use crate::extract::{PageScroll, ViewStore};
use crate::load::Outcome;
use crate::navigation::NavigationKind;
use crate::render::{Content, error_view};
use crate::router::{Level, Target};
use crate::session::Visit;

const MAX_REDIRECTS: u8 = 20;

/// How a finished navigation updates the history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NavKind {
    Push,
    Replace,
    /// The history already points at the url (back/forward, initial load, invalidation).
    Silent,
}

/// A navigation whose hooks and loads are running.
pub(super) struct Pending<S> {
    pub(super) job: Job<S>,
    kind: NavKind,
    options: GotoOptions,
    redirects: u8,
    /// Set when reloading the mounted page (invalidation), which stays the same page instance.
    reload_of: Option<u64>,
    /// Carried over to the navigation that replaces this one, so it isn't lost.
    invalidation: Invalidation,
}

/// How to carry out a blocked navigation once confirmed.
pub(super) enum Resume {
    Goto(String, GotoOptions),
    Traverse(Direction),
}

pub(super) struct Blocked {
    pub(super) navigation: Navigation,
    resume: Resume,
}

impl<S: Clone + 'static> RouterApp<S> {
    /// A navigation asked for by the app (links, `goto`), unless the page blocks it.
    pub(super) fn request_goto(&mut self, url: &str, options: GotoOptions) {
        let to = Request::new(url).untracked_url();
        let same_url = self
            .mounted
            .as_ref()
            .is_some_and(|mounted| mounted.url == to);
        if !same_url && self.should_block() {
            return self.block(
                Resume::Goto(url.to_owned(), options),
                to,
                Direction::Forward,
            );
        }
        // Like a browser, following a link to the current url replaces the entry.
        let kind = if options.replace_state || same_url {
            NavKind::Replace
        } else {
            NavKind::Push
        };
        self.navigate(url, kind, options);
    }

    /// Whether a navigation away from the page must wait for the user's confirmation.
    pub(super) fn should_block(&mut self) -> bool {
        let unblocked = core::mem::take(&mut self.unblock_next);
        self.blocking && !unblocked
    }

    pub(super) fn block(&mut self, resume: Resume, to: String, direction: Direction) {
        let kind = match resume {
            Resume::Goto(..) => NavigationKind::Goto,
            Resume::Traverse(_) => NavigationKind::Popstate,
        };
        let navigation = Navigation {
            from: self.mounted.as_ref().map(|mounted| mounted.url.clone()),
            to,
            kind,
            direction,
        };
        self.blocked = Some(Blocked { navigation, resume });
    }

    pub(super) fn confirm_blocked(&mut self) {
        let Some(blocked) = self.blocked.take() else {
            return;
        };
        self.unblock_next = true;
        match blocked.resume {
            Resume::Goto(url, options) => self.request_goto(&url, options),
            Resume::Traverse(Direction::Back) => self.history.back(),
            Resume::Traverse(_) => self.history.forward(),
        }
    }

    pub(super) fn navigate(&mut self, url: &str, kind: NavKind, options: GotoOptions) {
        let invalidation = if options.invalidate_all {
            Invalidation::All
        } else {
            Invalidation::None
        };
        self.start(url, kind, options, 0, None, invalidation);
    }

    /// Starts loading `url` (or adopts its preload), replacing any navigation in progress.
    /// What the replaced navigation invalidated is invalidated by this one too.
    pub(super) fn start(
        &mut self,
        url: &str,
        kind: NavKind,
        options: GotoOptions,
        redirects: u8,
        reload_of: Option<u64>,
        invalidation: Invalidation,
    ) {
        let invalidation = match self.pending.take() {
            Some(replaced) => replaced.invalidation.merge(invalidation),
            None => invalidation,
        };
        let (entry, request) = self.table.resolve(url);
        let url = request.untracked_url();
        let preloaded = (invalidation == Invalidation::None)
            .then(|| {
                self.preloads
                    .iter()
                    .position(|job| job.request.untracked_url() == url)
            })
            .flatten()
            .map(|index| self.preloads.remove(index));
        let job = preloaded.unwrap_or_else(|| {
            let previous: Vec<_> = self.mounted.iter().flat_map(Mounted::loaded).collect();
            Job::start(entry, request, &self.state, &previous, &invalidation)
        });

        self.nav.loading = Some(url);
        self.pending = Some(Pending {
            job,
            kind,
            options,
            redirects,
            reload_of,
            invalidation,
        });
    }

    /// Shows the outcome of a navigation: follows redirects, updates the history, swaps the
    /// mounted page and starts the transition.
    pub(super) fn commit(&mut self, pending: Pending<S>, outcome: Outcome) {
        let Pending {
            job,
            kind,
            options,
            redirects,
            reload_of,
            ..
        } = pending;
        let levels = &job.entry.levels;
        let content = match (outcome.failure, outcome.page, &job.entry.target) {
            (Some((_, Rejection::Redirect(redirect))), ..) if redirects < MAX_REDIRECTS => {
                let kind = if kind == NavKind::Push {
                    NavKind::Push
                } else {
                    NavKind::Replace
                };
                return self.start(
                    &redirect.to,
                    kind,
                    options,
                    redirects + 1,
                    None,
                    Invalidation::None,
                );
            }
            (Some((failed, Rejection::Redirect(_))), ..) => {
                error_view(levels, failed, RouteError::internal("too many redirects"))
            }
            (Some((failed, Rejection::Error(error))), ..) => error_view(levels, failed, error),
            (None, Some(data), Target::Page(page)) => Content::Page {
                page: Rc::clone(page),
                data,
            },
            _ => unreachable!("a route without failures loaded its page"),
        };

        let url = job.request.untracked_url();
        match kind {
            NavKind::Push => self.key = self.history.push(&url),
            NavKind::Replace => self.history.replace(&url),
            NavKind::Silent => {}
        }

        let previous = self.mounted.take();
        let navigation_kind = match (reload_of, &previous, kind) {
            (Some(_), _, _) => NavigationKind::Invalidate,
            (None, None, _) => NavigationKind::Enter,
            (None, Some(_), NavKind::Push) => NavigationKind::Goto,
            (None, Some(_), NavKind::Replace) => NavigationKind::Replace,
            (None, Some(_), NavKind::Silent) => NavigationKind::Popstate,
        };
        let direction = match navigation_kind {
            NavigationKind::Goto => Direction::Forward,
            NavigationKind::Popstate => self.session.direction(self.shown_key, self.key),
            _ => Direction::None,
        };
        let navigation = Navigation {
            from: previous.as_ref().map(|previous| previous.url.clone()),
            to: url,
            kind: navigation_kind,
            direction,
        };
        let history_state = match kind {
            // Back/forward to an entry of an earlier page, or a reload: restore its state.
            NavKind::Silent => self
                .session
                .get(self.key)
                .and_then(|meta| meta.state.clone()),
            NavKind::Push | NavKind::Replace => None,
        };

        // Only a reload keeps the page instance. Other navigations may keep the page's data, but
        // show a different url, so they get their own instance (and history entry identity).
        // Back/forward to a page that was left continues that instance's snapshots.
        let reload = reload_of.is_some();
        let node = content.page_node();
        let visit = match navigation_kind {
            NavigationKind::Popstate => self
                .session
                .get(self.key)
                .map(|meta| meta.page)
                .and_then(|page| Some((page, self.session.take_visit(page, node)?))),
            _ => None,
        };
        let (id, store) = match (&previous, &visit) {
            (Some(previous), _) if reload => (previous.id, Rc::clone(&previous.store)),
            (_, Some((page, visit))) => (*page, Rc::new(ViewStore::restored(&visit.page))),
            _ => {
                self.next_page_id += 1;
                (self.next_page_id, Rc::default())
            }
        };
        let layout_stores = layout_stores(
            levels,
            previous.as_ref(),
            visit.as_ref().map(|(_, visit)| visit),
        );

        let mut mounted = Mounted {
            id,
            request: job.request,
            url: self.history.current().url,
            history_state,
            levels: levels.clone(),
            layouts: outcome.layouts,
            layout_stores,
            store,
            content,
            navigation: navigation.clone(),
            scroll: PageScroll::default(),
        };
        let previous = match previous {
            Some(previous) if reload => {
                mounted.scroll = previous.scroll;
                None
            }
            previous => {
                if let Some(previous) = &previous {
                    self.leave(previous, &mounted);
                }
                mounted.scroll.restore.set(match navigation_kind {
                    NavigationKind::Popstate => {
                        self.session.get(self.key).and_then(|meta| meta.scroll)
                    }
                    _ if options.no_scroll => previous
                        .as_ref()
                        .map(|previous| previous.scroll.offset.get()),
                    _ => None,
                });
                previous
            }
        };

        self.session
            .record(self.key, mounted.id, mounted.history_state.clone());
        self.shown_key = self.key;
        self.mounted = Some(mounted);
        // Preloads were planned against the previous page.
        self.preloads.clear();
        // Transitions start once everything is loaded.
        self.transition = previous.map(|previous| Transition {
            previous,
            direction,
            state: Default::default(),
        });
        if navigation_kind != NavigationKind::Invalidate {
            for callback in &self.after_navigate {
                callback(&navigation, &self.state);
            }
        }

        let deferred = core::mem::take(&mut self.deferred);
        if deferred != Invalidation::None {
            self.reload(deferred);
        }
    }

    /// Remembers what back/forward restores of the page being left: its snapshots (with those of
    /// the layouts that leave with it) and the scroll position of its history entry.
    fn leave(&mut self, previous: &Mounted<S>, next: &Mounted<S>) {
        let kept = |store: &Rc<ViewStore>| {
            next.layout_stores
                .iter()
                .flatten()
                .any(|kept| Rc::ptr_eq(kept, store))
        };
        let layouts = previous
            .levels
            .iter()
            .zip(&previous.layout_stores)
            .filter_map(|(level, store)| Some((level.layout.as_ref()?.loader.id, store.clone()?)))
            .filter(|(_, store)| !kept(store))
            .collect();
        let visit = Visit {
            node: previous.content.page_node(),
            page: Rc::clone(&previous.store),
            layouts,
        };
        self.session.save_visit(previous.id, visit);

        if self.shown_key == self.key {
            // The entry is being replaced by another page.
            return;
        }
        if let Some(meta) = self.session.get_mut(self.shown_key) {
            meta.scroll = Some(previous.scroll.offset.get());
        }
    }
}

/// The view stores of the layouts of `levels`: kept from the previous page while the layout
/// stays on screen, restored from a visit on back/forward, fresh otherwise.
fn layout_stores<S>(
    levels: &[Rc<Level<S>>],
    previous: Option<&Mounted<S>>,
    visit: Option<&Visit>,
) -> Vec<Option<Rc<ViewStore>>> {
    levels
        .iter()
        .map(|level| {
            let node = level.layout.as_ref()?.loader.id;
            let kept = previous.and_then(|previous| previous.layout_store(node));
            let restored = || {
                let (_, left) = visit?.layouts.iter().find(|(id, _)| *id == node)?;
                Some(Rc::new(ViewStore::restored(left)))
            };
            Some(kept.or_else(restored).unwrap_or_default())
        })
        .collect()
}
