//! [`RouterApp`]: the router at runtime.
//!
//! Each frame it picks up history changes, drives the pending navigation and preloads, draws the
//! mounted page and then applies what the views asked for. Starting, intercepting and
//! committing navigations lives in [`navigate`].

mod navigate;

use core::any::Any;
use core::cell::RefCell;
use std::rc::Rc;

use egui::Ui;

use crate::extract::{NavAction, NavState};
use crate::history::{EntryKey, History, HistoryEntry};
use crate::load::{Invalidation, Job};
use crate::navigation::{Direction, GotoOptions, Navigation};
use crate::render::{self, Mounted, Transition};
use crate::request::Request;
use crate::router::RouteTable;
use crate::session::Session;
use navigate::{Blocked, NavKind, Pending, Resume};

/// How many routes may be preloaded at once; the oldest preload is dropped beyond.
const MAX_PRELOADS: usize = 8;

type AfterNavigate<S> = Box<dyn Fn(&Navigation, &S)>;

/// A built router: owns the app state, the history and the page on screen.
///
/// Call [`RouterApp::ui`] every frame, e.g. from `eframe::App::ui`.
pub struct RouterApp<S> {
    table: RouteTable<S>,
    state: S,
    history: Box<dyn History>,
    session: Session,
    /// Shared with the views during a frame.
    nav: NavState,

    mounted: Option<Mounted<S>>,
    transition: Option<Transition<S>>,
    pending: Option<Pending<S>>,
    preloads: Vec<Job<S>>,
    /// Invalidations requested while a navigation was pending, applied once it's done.
    deferred: Invalidation,

    /// The history entry being shown or navigated to.
    key: EntryKey,
    /// The history entry of the mounted page.
    shown_key: EntryKey,
    next_page_id: u64,

    /// Whether the page blocked navigation during the last frame.
    blocking: bool,
    blocked: Option<Blocked>,
    /// Lets the next navigation through a block (after the user confirmed it).
    unblock_next: bool,
    /// History moves made to undo a blocked back/forward, whose result we ignore.
    ignored_pops: usize,

    after_navigate: Vec<AfterNavigate<S>>,
}

impl<S: Clone + 'static> RouterApp<S> {
    pub(crate) fn new(table: RouteTable<S>, state: S, history: Box<dyn History>) -> Self {
        let initial = history.current();
        let mut app = Self {
            table,
            state,
            history,
            session: Session::default(),
            nav: NavState {
                preload_on_hover: true,
                ..NavState::default()
            },
            mounted: None,
            transition: None,
            pending: None,
            preloads: Vec::new(),
            deferred: Invalidation::None,
            key: initial.key,
            shown_key: initial.key,
            next_page_id: 0,
            blocking: false,
            blocked: None,
            unblock_next: false,
            ignored_pops: 0,
            after_navigate: Vec::new(),
        };
        app.navigate(&initial.url, NavKind::Silent, GotoOptions::default());
        app
    }

    /// Whether [`Nav::link`](crate::extract::Nav::link) preloads its target while hovered. On by default.
    pub fn preload_on_hover(mut self, enabled: bool) -> Self {
        self.nav.preload_on_hover = enabled;
        self
    }

    /// Calls `callback` after every navigation, once the new page is mounted, e.g. for
    /// analytics or focus management. Not called for invalidations.
    pub fn after_navigate(mut self, callback: impl Fn(&Navigation, &S) + 'static) -> Self {
        self.after_navigate.push(Box::new(callback));
        self
    }

    pub fn state(&self) -> &S {
        &self.state
    }

    /// The pattern of the page route that `url` matches, e.g. `/blog/{slug}`, or `None` if it
    /// matches none (a 404). Doesn't navigate or run anything: e.g. to check in a test that
    /// every address the app builds leads to a page.
    ///
    /// A match can still fail to load, e.g. with a [`Path`](crate::extract::Path) that doesn't
    /// parse.
    ///
    /// ```
    /// # use egui_kit::{MemoryHistory, Router};
    /// # use egui_kit::page::Page;
    /// let app = Router::<()>::new()
    ///     .page("/blog/{slug}", Page::new(|_: &mut egui::Ui| {}))
    ///     .with_state_and_history((), MemoryHistory::new("/"));
    /// assert_eq!(app.matches("/blog/hello?draft"), Some("/blog/{slug}"));
    /// assert_eq!(app.matches("/blog"), None);
    /// ```
    pub fn matches(&self, url: &str) -> Option<&str> {
        self.table.page_route(url)
    }

    /// The request of the page on screen.
    pub fn request(&self) -> Option<&Request> {
        self.mounted.as_ref().map(|mounted| &mounted.request)
    }

    /// The url of the current history entry, see [`Nav::url`](crate::extract::Nav::url).
    pub fn url(&self) -> Option<&str> {
        self.mounted.as_ref().map(|mounted| mounted.url.as_str())
    }

    pub fn is_loading(&self) -> bool {
        self.pending.is_some()
    }

    /// Navigates to `url`, adding a history entry. Going to the current url does nothing, unless
    /// [`GotoOptions::invalidate_all`] asks for a reload. See
    /// [`Nav::goto`](crate::extract::Nav::goto).
    pub fn goto(&mut self, url: &str) {
        self.goto_with(url, GotoOptions::default());
    }

    /// See [`Nav::goto_with`](crate::extract::Nav::goto_with).
    pub fn goto_with(&mut self, url: &str, options: GotoOptions) {
        self.request_goto(url, options);
    }

    pub fn back(&mut self) {
        self.history.back();
    }

    pub fn forward(&mut self) {
        self.history.forward();
    }

    /// See [`Nav::invalidate`](crate::extract::Nav::invalidate); e.g. after data changed in the background.
    pub fn invalidate(&mut self, key: impl Into<String>) {
        self.invalidate_with(Invalidation::Keys(vec![key.into()]));
    }

    /// See [`Nav::invalidate_all`](crate::extract::Nav::invalidate_all).
    pub fn invalidate_all(&mut self) {
        self.invalidate_with(Invalidation::All);
    }

    /// See [`Nav::preload`](crate::extract::Nav::preload).
    pub fn preload(&mut self, url: &str) {
        let (entry, request) = self.table.resolve(url);
        let url = request.untracked_url();
        let busy = |job: &Job<S>| job.request.untracked_url() == url;
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| busy(&pending.job))
            || self.preloads.iter().any(busy)
        {
            return;
        }
        let previous: Vec<_> = self.mounted.iter().flat_map(Mounted::loaded).collect();
        let job = Job::start(entry, request, &self.state, &previous, &Invalidation::None);
        if self.preloads.len() == MAX_PRELOADS {
            self.preloads.remove(0);
        }
        self.preloads.push(job);
    }

    /// Shows the current page, wrapped in its layouts.
    pub fn ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        #[cfg(feature = "remote")]
        let _runtime = crate::remote::enter();
        #[cfg(feature = "remote")]
        crate::remote::begin_frame(&ctx);

        #[cfg(not(target_arch = "wasm32"))]
        self.handle_back_forward_input(ui);
        if let Some(entry) = self.history.poll(&ctx) {
            self.traverse(entry);
        }
        self.poll(&ctx);

        self.nav.shows_links = self.history.shows_links();
        self.nav.can_go_back = self.history.can_go_back();
        self.nav.can_go_forward = self.history.can_go_forward();
        self.nav.blocked = self
            .blocked
            .as_ref()
            .map(|blocked| blocked.navigation.clone());
        let nav = RefCell::new(core::mem::take(&mut self.nav));
        match &mut self.mounted {
            Some(mounted) => {
                render::render(ui, mounted, self.transition.as_mut(), &self.state, &nav);
            }
            None => {
                ui.centered_and_justified(|ui| ui.spinner());
            }
        }
        self.nav = nav.into_inner();
        self.blocking = core::mem::take(&mut self.nav.blocking);
        let hovered_link = self.nav.hovered_link.take();
        // Changing the link mid-click or mid-drag would disturb it.
        if !ctx.input(|i| i.pointer.any_down()) {
            self.history.hover_link(hovered_link.as_deref());
        }
        if let Some(transition) = &self.transition
            && (transition.state.done.get() || !transition.state.claimed.get())
        {
            self.transition = None;
        }

        #[cfg(feature = "remote")]
        crate::remote::end_frame();

        let actions = core::mem::take(&mut self.nav.actions);
        if !actions.is_empty() {
            for action in actions {
                self.apply(action);
            }
            ctx.request_repaint();
        }
        self.poll(&ctx);
    }

    /// Mouse back/forward buttons and Alt+Left/Right. Browsers handle these themselves.
    #[cfg(not(target_arch = "wasm32"))]
    fn handle_back_forward_input(&mut self, ui: &Ui) {
        use egui::{Key, Modifiers, PointerButton};

        let (back, forward) = ui.input_mut(|i| {
            let back = i.pointer.button_pressed(PointerButton::Extra1)
                | i.consume_key(Modifiers::ALT, Key::ArrowLeft);
            let forward = i.pointer.button_pressed(PointerButton::Extra2)
                | i.consume_key(Modifiers::ALT, Key::ArrowRight);
            (back, forward)
        });
        if back {
            self.history.back();
        }
        if forward {
            self.history.forward();
        }
    }

    fn apply(&mut self, action: NavAction) {
        match action {
            NavAction::Goto(url, options) => self.request_goto(&url, options),
            NavAction::Back => self.history.back(),
            NavAction::Forward => self.history.forward(),
            NavAction::Invalidate(invalidation) => self.invalidate_with(invalidation),
            NavAction::Preload(url) => self.preload(&url),
            NavAction::PushState(url, state) => self.shallow(url, state, true),
            NavAction::ReplaceState(url, state) => self.shallow(url, state, false),
            NavAction::ReplaceUrl(url) => {
                let state = self.mounted.as_ref().and_then(|m| m.history_state.clone());
                self.shallow(url, state, false);
            }
            NavAction::ConfirmNavigation => self.confirm_blocked(),
            NavAction::CancelNavigation => self.blocked = None,
        }
    }

    /// Applies the invalidations of commands, drives the preloads and the pending navigation;
    /// commits the latter when done. Loads that complete synchronously are committed within the
    /// same frame, so they never flash a loading state.
    fn poll(&mut self, ctx: &egui::Context) {
        #[cfg(feature = "remote")]
        {
            let invalidated = crate::remote::take_invalidated();
            if !invalidated.is_empty() {
                self.invalidate_with(Invalidation::Keys(invalidated));
                ctx.request_repaint();
            }
        }
        for job in &mut self.preloads {
            job.poll(ctx);
        }
        while let Some(pending) = &mut self.pending {
            if !pending.job.poll(ctx) {
                return;
            }
            let mut pending = self.pending.take().expect("checked above");
            let outcome = pending.job.take_outcome().expect("the job is done");
            self.nav.loading = None;
            self.commit(pending, outcome);
        }
    }

    /// Shallow routing: a history entry for the mounted page, without a navigation.
    fn shallow(&mut self, url: String, state: Option<Rc<dyn Any>>, push: bool) {
        let Some(mounted) = &mut self.mounted else {
            return;
        };
        if !url.is_empty() {
            mounted.url = url;
        }
        if push {
            self.key = self.history.push(&mounted.url);
        } else {
            self.history.replace(&mounted.url);
        }
        self.shown_key = self.key;
        mounted.history_state = state.clone();
        self.session.record(self.key, mounted.id, state);
    }

    /// The history moved to `entry` (back/forward). Entries of the mounted page only swap the
    /// page state; anything else is a regular navigation, unless the page blocks it.
    fn traverse(&mut self, entry: HistoryEntry) {
        if self.ignored_pops > 0 {
            self.ignored_pops -= 1;
            return;
        }
        let meta = self.session.get(entry.key);
        if let (Some(mounted), Some(meta)) = (&mut self.mounted, meta)
            && meta.page == mounted.id
        {
            self.pending = None;
            self.nav.loading = None;
            self.key = entry.key;
            self.shown_key = entry.key;
            mounted.url = entry.url;
            mounted.history_state = meta.state.clone();
            return;
        }

        let direction = self.session.direction(self.shown_key, entry.key);
        if direction != Direction::None && self.should_block() {
            // The history already moved: move it back, and redo the move once confirmed.
            match direction {
                Direction::Back => self.history.forward(),
                _ => self.history.back(),
            }
            self.ignored_pops += 1;
            self.block(Resume::Traverse(direction), entry.url, direction);
            return;
        }
        self.key = entry.key;
        self.navigate(&entry.url, NavKind::Silent, GotoOptions::default());
    }

    /// Reruns the loads and queries that `invalidation` hits.
    fn invalidate_with(&mut self, invalidation: Invalidation) {
        #[cfg(feature = "remote")]
        crate::remote::invalidate_queries(&invalidation);
        self.reload(invalidation);
    }

    /// Reloads the mounted page with `invalidation`, after the pending navigation if any.
    fn reload(&mut self, invalidation: Invalidation) {
        self.preloads.clear();
        if self.pending.is_some() {
            self.deferred = core::mem::take(&mut self.deferred).merge(invalidation);
            return;
        }
        if let Some(mounted) = &self.mounted {
            let (url, id) = (mounted.request.untracked_url(), mounted.id);
            self.start(
                &url,
                NavKind::Silent,
                GotoOptions::default(),
                0,
                Some(id),
                invalidation,
            );
        }
    }
}
