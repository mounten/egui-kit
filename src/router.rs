//! The route builder.

mod pattern;
mod table;

use std::rc::Rc;

use crate::app::RouterApp;
use crate::error::{Rejection, RouteError};
use crate::history::{self, History};
use crate::page::{self, ErasedHandler, ErasedView, Handler, Layout, LayoutReset, Page, View};
use pattern::Matcher;
pub(crate) use pattern::{Pattern, join, normalize};
pub(crate) use table::{RouteEntry, RouteTable, Target};

/// The route builder.
///
/// A router's [layout](Self::layout), [error page](Self::error) and [hooks](Self::hook) apply to
/// all of its pages and to all routers [nested](Self::nest) inside it. The order of the builder calls does not matter.
///
/// Paths are made of static segments and parameters: `{id}`, `{id=matcher}`, optional
/// `{?lang}` / `{?lang=matcher}` and a trailing `{*rest}`.
pub struct Router<S> {
    name: Option<String>,
    routes: Vec<(String, Page<S>)>,
    nested: Vec<(String, Self)>,
    hooks: Vec<ErasedHandler<S>>,
    layout: Option<Layout<S>>,
    error: Option<ErasedView<S>>,
    matchers: Vec<(String, Matcher)>,
}

impl<S: Clone + 'static> Default for Router<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: Clone + 'static> Router<S> {
    pub fn new() -> Self {
        Self {
            name: None,
            routes: Vec::new(),
            nested: Vec::new(),
            hooks: Vec::new(),
            layout: None,
            error: None,
            matchers: Vec::new(),
        }
    }

    /// Adds a page at `path`, e.g. `/`, `/about`, `/blog/{slug}` or `/files/{*path}`.
    ///
    /// # Panics
    /// When the pattern is invalid. Conflicting routes panic when the app is built.
    pub fn page(mut self, path: &str, page: Page<S>) -> Self {
        Pattern::parse(path);
        self.routes.push((path.to_owned(), page));
        self
    }

    /// Mounts `router` below `prefix`. The prefix may contain parameters, e.g. `/users/{id}`,
    /// which are then available to the nested hooks, layouts and pages.
    pub fn nest(mut self, prefix: &str, router: Self) -> Self {
        Pattern::parse(prefix);
        self.nested.push((prefix.to_owned(), router));
        self
    }

    /// Adds the routes of `router` at the same level, keeping its layout, hooks and error page
    /// scoped to its own routes.
    pub fn merge(self, router: Self) -> Self {
        self.nest("/", router)
    }

    /// Names this router, so pages and layouts below it can
    /// [reset their layouts](Page::reset_layouts_to) up to here.
    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Sets the layout wrapping all pages of this router.
    ///
    /// # Panics
    /// When a layout was already set.
    pub fn layout(mut self, layout: Layout<S>) -> Self {
        assert!(self.layout.is_none(), "a router can only have one layout");
        self.layout = Some(layout);
        self
    }

    /// Sets the error page for errors of this router's routes, including
    /// paths below its prefix that match no route (404).
    ///
    /// The view extracts the error as `&RouteError`, like a page extracts its data.
    /// It is rendered inside this router's layout. Errors raised by the layout's own `load`
    /// (or this router's hooks) are handled by the error page of a parent router.
    ///
    /// # Panics
    /// When an error page was already set.
    pub fn error<V, U>(mut self, view: V) -> Self
    where
        V: View<U, S, RouteError>,
    {
        assert!(
            self.error.is_none(),
            "a router can only have one error page"
        );
        self.error = Some(Rc::new(move |ui, cx| view.call(ui, cx)));
        self
    }

    /// Adds a hook that runs before every navigation to a route of this router (and its nested
    /// routers), before any `load`. Hooks of outer routers run first. Hooks also run when a
    /// route is [preloaded](crate::extract::Nav::preload), so keep them free of side effects.
    ///
    /// A hook is an async handler with extractor arguments, returning `Result<(), E>` where
    /// `E: Into<Rejection>`: return an error to show the error page, or a redirect.
    pub fn hook<H, T, E>(mut self, hook: H) -> Self
    where
        H: Handler<T, S, Output = Result<(), E>>,
        E: Into<Rejection>,
    {
        self.hooks.push(page::erase(hook));
        self
    }

    /// Registers a parameter matcher, usable as `{param=name}` in any route of the app.
    /// A route only matches if the matcher accepts the
    /// segment; otherwise other routes are tried.
    ///
    /// ```
    /// # use egui_kit::Router;
    /// let router = Router::<()>::new().matcher("int", |s| s.parse::<u64>().is_ok());
    /// ```
    ///
    /// # Panics
    /// When building an app with two matchers of the same name, or a route using an unknown one.
    pub fn matcher(
        mut self,
        name: impl Into<String>,
        matcher: impl Fn(&str) -> bool + 'static,
    ) -> Self {
        self.matchers.push((name.into(), Rc::new(matcher)));
        self
    }

    /// Builds the app, using the browser's history on wasm and an in-memory history elsewhere.
    pub fn with_state(self, state: S) -> RouterApp<S> {
        self.with_state_and_history(state, history::default_history())
    }

    /// Builds the app with a custom [`History`].
    ///
    /// # Panics
    /// When routes conflict or refer to unknown matchers or router names.
    pub fn with_state_and_history(self, state: S, history: impl History + 'static) -> RouterApp<S> {
        RouterApp::new(self.into_table(), state, Box::new(history))
    }

    pub(crate) fn into_table(self) -> RouteTable<S> {
        let mut table = RouteTable::default();
        self.flatten("/", &[], &mut table);
        table.validate();
        table
    }

    fn flatten(self, prefix: &str, parents: &[Rc<Level<S>>], table: &mut RouteTable<S>) {
        for (name, matcher) in self.matchers {
            assert!(
                table.matchers.insert(name.clone(), matcher).is_none(),
                "duplicate matcher `{name}`"
            );
        }

        let layout_reset = self.layout.as_ref().and_then(|layout| layout.reset.clone());
        let mut levels = match &layout_reset {
            Some(reset) => reset_layouts(parents, reset),
            None => parents.to_vec(),
        };
        levels.push(Rc::new(Level {
            name: self.name,
            hooks: self.hooks,
            layout: self.layout.map(Rc::new),
            error: self.error,
            params: Pattern::parse(prefix).param_count(),
        }));

        table.add_fallback(prefix, levels.clone());
        for (path, page) in self.routes {
            let levels = match &page.reset {
                Some(reset) => reset_layouts(&levels, reset),
                None => levels.clone(),
            };
            table.add_page(&join(prefix, &path), levels, page);
        }
        for (nested_prefix, router) in self.nested {
            router.flatten(&join(prefix, &nested_prefix), &levels, table);
        }
    }
}

/// Drops the layouts of the levels below the reset target. Hooks and error pages stay.
fn reset_layouts<S>(levels: &[Rc<Level<S>>], reset: &LayoutReset) -> Vec<Rc<Level<S>>> {
    let keep = match reset {
        LayoutReset::Root => 0,
        LayoutReset::To(name) => levels
            .iter()
            .rposition(|level| level.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("no parent router named `{name}` to reset the layouts to")),
    };
    levels
        .iter()
        .enumerate()
        .map(|(i, level)| {
            if i <= keep {
                Rc::clone(level)
            } else {
                Rc::new(level.without_layout())
            }
        })
        .collect()
}

/// What a [`Router`](crate::Router) contributes to all routes below it.
pub(crate) struct Level<S> {
    pub(crate) name: Option<String>,
    pub(crate) hooks: Vec<ErasedHandler<S>>,
    pub(crate) layout: Option<Rc<Layout<S>>>,
    pub(crate) error: Option<ErasedView<S>>,
    /// How many path parameters this level's prefix has: its hooks and layout see only those.
    pub(crate) params: usize,
}

impl<S> Level<S> {
    /// The same level, without its layout (for routes that reset their layouts).
    pub(crate) fn without_layout(&self) -> Self {
        Self {
            name: self.name.clone(),
            hooks: self.hooks.clone(),
            layout: None,
            error: self.error.clone(),
            params: self.params,
        }
    }
}
