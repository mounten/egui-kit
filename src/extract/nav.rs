//! [`Nav`]: the views' handle to the router.

use core::any::Any;
use std::rc::Rc;

use egui::{Response, Ui};

use super::FromView;
use crate::load::Invalidation;
use crate::navigation::{GotoOptions, Navigation};
use crate::page::{Frame, ViewContext};

/// Requests from views, applied after the frame.
pub(crate) enum NavAction {
    Goto(String, GotoOptions),
    Back,
    Forward,
    Invalidate(Invalidation),
    Preload(String),
    /// Shallow routing: a new or replaced history entry without a navigation.
    PushState(String, Option<Rc<dyn Any>>),
    ReplaceState(String, Option<Rc<dyn Any>>),
    /// Shallow routing that keeps the entry's state.
    ReplaceUrl(String),
    ConfirmNavigation,
    CancelNavigation,
}

/// Router state shared with the views during a frame.
#[derive(Default)]
pub(crate) struct NavState {
    pub(crate) actions: Vec<NavAction>,
    pub(crate) loading: Option<String>,
    pub(crate) can_go_back: bool,
    pub(crate) can_go_forward: bool,
    pub(crate) preload_on_hover: bool,
    /// Whether the history shows hovered links (and opens modified clicks) itself.
    pub(crate) shows_links: bool,
    /// The link under the pointer, set during the frame.
    pub(crate) hovered_link: Option<String>,
    /// Set by [`Nav::block_navigation`] during the frame.
    pub(crate) blocking: bool,
    pub(crate) blocked: Option<Navigation>,
}

/// The router, as seen by a view: navigating, loading, shallow routing and blocking.
///
/// An extractor: add a `nav: Nav` argument to any view. It is `Copy`, so it can be used in
/// nested closures. Requests are queued and applied after the frame.
#[derive(Clone, Copy)]
pub struct Nav<'f> {
    pub(crate) frame: &'f Frame<'f>,
}

impl<S: 'static, D> FromView<S, D> for Nav<'_> {
    type Item<'f> = Nav<'f>;

    fn from_view<'f>(cx: &'f ViewContext<'_, S>) -> Nav<'f> {
        cx.nav()
    }
}

impl Nav<'_> {
    fn push(&self, action: NavAction) {
        self.frame.nav.borrow_mut().actions.push(action);
    }
}

/// Where we are.
impl<'f> Nav<'f> {
    /// The url of the current history entry. Differs from the page's
    /// [`Request`](crate::Request) after [`Nav::push_state`] / [`Nav::replace_state`] with a
    /// new url.
    pub fn url(&self) -> &'f str {
        self.frame.url
    }

    /// Whether `href` is the current path, or (for anything but `/`) a parent of it. The path
    /// is [`Nav::url`]'s, so it follows shallow routing.
    pub fn is_active(&self, href: &str) -> bool {
        let path = |url: &str| crate::router::normalize(url.split('?').next().unwrap_or_default());
        let (href, path) = (path(href), path(self.frame.url));
        path == href
            || (href != "/" && path.starts_with(&href) && path[href.len()..].starts_with('/'))
    }

    /// Whether this is the previous page, rendered while a [`ViewTransition`](crate::ViewTransition)
    /// animates it out. Its `Ui` is disabled (without looking so), and its navigation calls are
    /// ignored. Skip things that live outside the page's `Ui` (windows, modals, popups), which
    /// the transition can't fade, clip or disable.
    pub fn is_outgoing(&self) -> bool {
        self.frame.outgoing
    }
}

/// Navigating.
impl Nav<'_> {
    /// Navigates to `url`, adding a history entry.
    ///
    /// The current page stays visible until all hooks and `load` handlers of the new route finished.
    /// Going to the current url does nothing (the page keeps its state), unless
    /// [`GotoOptions::invalidate_all`] asks for a reload. Going to the url that is already
    /// loading, with the same options, keeps that navigation going, so a view may ask for it
    /// every frame until it is done.
    pub fn goto(&self, url: impl Into<String>) {
        self.goto_with(url, GotoOptions::default());
    }

    /// [`Nav::goto`] with options.
    pub fn goto_with(&self, url: impl Into<String>, options: GotoOptions) {
        self.push(NavAction::Goto(url.into(), options));
    }

    /// Navigates to `url`, replacing the current history entry. Like [`Nav::goto`], going to the
    /// current url does nothing, unless [`GotoOptions::invalidate_all`] asks for a reload.
    pub fn replace(&self, url: impl Into<String>) {
        self.goto_with(url, GotoOptions::default().replace_state());
    }

    pub fn back(&self) {
        self.push(NavAction::Back);
    }

    pub fn forward(&self) {
        self.push(NavAction::Forward);
    }

    pub fn can_go_back(&self) -> bool {
        self.frame.nav.borrow().can_go_back
    }

    pub fn can_go_forward(&self) -> bool {
        self.frame.nav.borrow().can_go_forward
    }

    /// A hyperlink that navigates within the app. Preloads the target while hovered,
    /// unless [disabled](crate::RouterApp::preload_on_hover).
    pub fn link(&self, ui: &mut Ui, text: impl Into<egui::WidgetText>, href: &str) -> Response {
        let mut response = ui.link(text);
        // The browser shows the url itself.
        if !self.frame.nav.borrow().shows_links {
            response = response.on_hover_text(href);
        }
        self.follow(&response, href);
        response
    }

    /// A selectable label that is highlighted when `href` [is active](Self::is_active).
    pub fn nav_link(&self, ui: &mut Ui, text: impl Into<egui::WidgetText>, href: &str) -> Response {
        let response = ui.selectable_label(self.is_active(href), text);
        self.follow(&response, href);
        response
    }

    /// Makes any widget behave like a [link](Self::link) to `href`: a click navigates there,
    /// hovering preloads it and lets the browser show it (see `BrowserHistory::link_preview`),
    /// which then also opens Ctrl/Cmd/Shift-clicks in a new tab.
    ///
    /// ```
    /// # use egui_kit::extract::Nav;
    /// fn card(ui: &mut egui::Ui, nav: Nav, title: &str, href: &str) {
    ///     let response = egui::Frame::group(ui.style())
    ///         .show(ui, |ui| ui.heading(title))
    ///         .response
    ///         .interact(egui::Sense::click());
    ///     nav.follow(&response, href);
    /// }
    /// ```
    pub fn follow(&self, response: &Response, href: &str) {
        if response.hovered() && !self.frame.outgoing {
            self.frame.nav.borrow_mut().hovered_link = Some(href.to_owned());
        }
        if response.clicked() {
            // The browser opens Ctrl/Cmd/Shift-clicked links in a new tab or window.
            let modified = response
                .ctx
                .input(|i| i.modifiers.ctrl || i.modifiers.command || i.modifiers.shift);
            if !(modified && self.frame.nav.borrow().shows_links) {
                self.goto(href);
            }
        } else if response.hovered() && self.frame.nav.borrow().preload_on_hover {
            self.preload(href);
        }
    }
}

/// Loading.
impl Nav<'_> {
    /// Whether a navigation is in progress.
    pub fn is_loading(&self) -> bool {
        self.frame.nav.borrow().loading.is_some()
    }

    /// The url currently being loaded, if any.
    pub fn loading_url(&self) -> Option<String> {
        self.frame.nav.borrow().loading.clone()
    }

    /// Starts loading `url` (hooks and loads) in the background, so that navigating there is
    /// instant. Preloads are dropped once any navigation finishes.
    pub fn preload(&self, url: impl Into<String>) {
        self.push(NavAction::Preload(url.into()));
    }

    /// Reruns the loads of the current page that [depend](crate::page::Page::depends) on `key`, and
    /// the loads below them that use [`Parent`](crate::extract::Parent). Queries that depend on
    /// it refetch too.
    pub fn invalidate(&self, key: impl Into<String>) {
        self.push(NavAction::Invalidate(Invalidation::Keys(vec![key.into()])));
    }

    /// Reruns all loads of the current page, and refetches all queries.
    pub fn invalidate_all(&self) {
        self.push(NavAction::Invalidate(Invalidation::All));
    }
}

/// Shallow routing.
impl Nav<'_> {
    /// Adds a history entry with `url` (`""` keeps the current url) and attaches `state` to it,
    /// without running hooks or loads. The page stays as it is and can read the state with the
    /// [`HistoryState`](super::HistoryState) extractor.
    ///
    /// Going back and forward between entries of the same page only swaps the page state.
    /// Entries of a page that is no longer shown are loaded normally, and get their state back.
    /// A browser reload loses all page state, so `url` should also work on its own, e.g. for a
    /// photo opened in a modal, the photo's own page.
    pub fn push_state<T: Any>(&self, url: impl Into<String>, state: T) {
        self.push(NavAction::PushState(url.into(), Some(Rc::new(state))));
    }

    /// Like [`Nav::push_state`], but replaces the state (and url) of the current entry.
    pub fn replace_state<T: Any>(&self, url: impl Into<String>, state: T) {
        self.push(NavAction::ReplaceState(url.into(), Some(Rc::new(state))));
    }

    /// Changes the url of the current entry, keeping its state: to keep the address in step with
    /// the page (another day in a calendar, another tab) without a navigation. Like
    /// [`Nav::replace_state`], the url should also work on its own.
    pub fn replace_url(&self, url: impl Into<String>) {
        self.push(NavAction::ReplaceUrl(url.into()));
    }

    /// Removes the state of the current entry, keeping its url.
    pub fn clear_history_state(&self) {
        self.push(NavAction::ReplaceState(String::new(), None));
    }
}

/// Blocking navigation.
impl Nav<'_> {
    /// Call every frame while leaving the page would lose something (e.g. unsaved changes).
    /// Navigations away from the page (links, `goto`, back/forward) are then held back until
    /// you [confirm](Self::confirm_navigation) or [cancel](Self::cancel_navigation) them;
    /// see [`Nav::blocked_navigation`]. Shallow routing and invalidation are not affected.
    ///
    /// Closing the browser tab or reloading it can't be blocked this way.
    pub fn block_navigation(&self) {
        self.frame.nav.borrow_mut().blocking = true;
    }

    /// The navigation being held back, to ask the user what to do.
    pub fn blocked_navigation(&self) -> Option<Navigation> {
        self.frame.nav.borrow().blocked.clone()
    }

    /// Lets the [blocked navigation](Self::blocked_navigation) happen.
    pub fn confirm_navigation(&self) {
        self.push(NavAction::ConfirmNavigation);
    }

    /// Drops the [blocked navigation](Self::blocked_navigation).
    pub fn cancel_navigation(&self) {
        self.push(NavAction::CancelNavigation);
    }
}
