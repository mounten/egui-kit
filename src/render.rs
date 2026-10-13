//! The page on screen, and drawing it inside its layouts.

use core::any::Any;
use core::cell::RefCell;
use std::rc::Rc;

use egui::Ui;

use crate::error::RouteError;
use crate::extract::{Nav, NavState, PageScroll, ViewStore};
use crate::load::Loaded;
use crate::navigation::{Direction, Navigation};
use crate::page::{Children, ErasedView, Frame, NodeId, Page, ViewContext, ViewData};
use crate::request::Request;
use crate::router::Level;
use crate::transition::{ChildrenTransition, TransitionHandle, TransitionState};

/// What is drawn inside the layouts.
pub(crate) enum Content<S> {
    Page {
        page: Rc<Page<S>>,
        data: Loaded,
    },
    Error {
        /// How many of the route's levels wrap the error page.
        depth: usize,
        view: Option<ErasedView<S>>,
        error: RouteError,
    },
}

impl<S> Content<S> {
    /// The page shown, `None` for an error page.
    pub(crate) fn page_node(&self) -> Option<NodeId> {
        match self {
            Self::Page { page, .. } => Some(page.loader.id),
            Self::Error { .. } => None,
        }
    }
}

/// A page instance on screen, with the data of its layouts.
pub(crate) struct Mounted<S> {
    /// Identifies this page instance; history entries created by shallow routing share it.
    pub(crate) id: u64,
    pub(crate) request: Request,
    /// The url of the current history entry (differs from `request` after shallow routing).
    pub(crate) url: String,
    pub(crate) history_state: Option<Rc<dyn Any>>,
    pub(crate) levels: Vec<Rc<Level<S>>>,
    /// Per level; `None` for levels without layout.
    pub(crate) layouts: Vec<Option<Loaded>>,
    /// The page state and snapshots of each layout, per level. Shared with the next page
    /// instance as long as the layout stays on screen.
    pub(crate) layout_stores: Vec<Option<Rc<ViewStore>>>,
    /// The page state and snapshots of the page (or error page).
    pub(crate) store: Rc<ViewStore>,
    pub(crate) content: Content<S>,
    pub(crate) navigation: Navigation,
    pub(crate) scroll: PageScroll,
}

/// A page on its way out, kept alive while a `ViewTransition` animates it.
pub(crate) struct Transition<S> {
    pub(crate) previous: Mounted<S>,
    pub(crate) direction: Direction,
    pub(crate) state: TransitionState,
}

impl<S> Mounted<S> {
    /// Everything this page loaded, layouts first.
    pub(crate) fn loaded(&self) -> impl Iterator<Item = &Loaded> {
        let page = match &self.content {
            Content::Page { data, .. } => Some(data),
            Content::Error { .. } => None,
        };
        self.layouts.iter().flatten().chain(page)
    }

    /// The view store of the layout `node`, if it is on screen.
    pub(crate) fn layout_store(&self, node: NodeId) -> Option<Rc<ViewStore>> {
        let (_, store) = self
            .levels
            .iter()
            .zip(&self.layout_stores)
            .find(|(level, _)| {
                level
                    .layout
                    .as_ref()
                    .is_some_and(|layout| layout.loader.id == node)
            })?;
        store.clone()
    }

    /// The levels drawn around the view.
    fn rendered_depth(&self) -> usize {
        match &self.content {
            Content::Page { .. } => self.levels.len(),
            Content::Error { depth, .. } => *depth,
        }
    }

    /// Draws the page in its layouts. The outgoing page of a transition only draws what is
    /// below the transitioning layout (`from`).
    fn render(
        &mut self,
        ui: &mut Ui,
        state: &S,
        nav: &RefCell<NavState>,
        from: usize,
        handoff: Option<Handoff<'_>>,
    ) {
        let depth = self.rendered_depth();
        let from = from.min(depth);
        let Self {
            id,
            request,
            url,
            history_state,
            levels,
            layouts,
            layout_stores,
            store,
            content,
            navigation,
            scroll,
        } = self;

        let frame = Frame {
            ctx: ui.ctx().clone(),
            request,
            url,
            history_state: history_state.as_deref(),
            navigation,
            page_id: *id,
            scroll,
            outgoing: from > 0,
            nav,
        };
        let above: &[Option<Loaded>] = &layouts[..depth];
        let mut leaf = |ui: &mut Ui| match content {
            Content::Page { page, data } => {
                let mut data = data.data.borrow_mut();
                let data = ViewData::Mut(&mut **data);
                (page.view)(ui, &ViewContext::new(state, &frame, data, store, above));
            }
            Content::Error {
                view: Some(view),
                error,
                ..
            } => view(
                ui,
                &ViewContext::new(state, &frame, ViewData::Mut(error), store, above),
            ),
            Content::Error {
                view: None, error, ..
            } => default_error_view(ui, Nav { frame: &frame }, error),
        };
        let chain = Chain {
            levels: &levels[from..depth],
            layouts: &layouts[from..depth],
            stores: &layout_stores[from..depth],
            all_layouts: layouts,
            start: from,
        };
        render_levels(ui, state, &frame, chain, &mut leaf, handoff);
    }
}

/// Draws the mounted page, and the outgoing one if a transition is running.
pub(crate) fn render<S>(
    ui: &mut Ui,
    mounted: &mut Mounted<S>,
    transition: Option<&mut Transition<S>>,
    state: &S,
    nav: &RefCell<NavState>,
) {
    let Some((transition, depth)) = transition.and_then(|transition| {
        transition.state.claimed.set(false);
        let depth = transition_owner(mounted, &transition.previous)?;
        Some((transition, depth))
    }) else {
        return mounted.render(ui, state, nav, 0, None);
    };

    // The outgoing page renders below the transitioning layout, without navigation powers.
    let Transition {
        previous,
        direction,
        state: transition_state,
    } = transition;
    let mut render_previous = |ui: &mut Ui| {
        // Still drawn, but no longer the page: it looks the same, but doesn't respond.
        let opacity = ui.opacity();
        ui.disable();
        ui.set_opacity(opacity);
        previous.render(
            ui,
            state,
            &RefCell::new(NavState::default()),
            depth + 1,
            None,
        );
    };
    let navigation = mounted.navigation.clone();
    let handoff = Handoff {
        depth,
        transition: ChildrenTransition {
            previous: &mut render_previous,
            state: transition_state,
            direction: *direction,
            navigation: &navigation,
        },
    };
    mounted.render(ui, state, nav, 0, Some(handoff));
}

/// The layout that animates a page change: the innermost one shared by both pages, among those
/// that use a `ViewTransition` (known from previous frames, in which the old page rendered it).
fn transition_owner<S>(new: &Mounted<S>, old: &Mounted<S>) -> Option<usize> {
    let (new_levels, old_levels) = (
        &new.levels[..new.rendered_depth()],
        &old.levels[..old.rendered_depth()],
    );
    let shared = new_levels
        .iter()
        .zip(old_levels)
        .take_while(|(a, b)| Rc::ptr_eq(a, b))
        .count();
    (0..shared).rev().find(|&i| {
        new_levels[i]
            .layout
            .as_ref()
            .is_some_and(|layout| layout.transitions.get())
    })
}

/// Passed down the layouts to the children that runs the transition.
struct Handoff<'a> {
    /// Remaining levels until the transitioning layout.
    depth: usize,
    transition: ChildrenTransition<'a>,
}

/// The levels around a view, with their layouts' data and stores.
struct Chain<'m, S> {
    levels: &'m [Rc<Level<S>>],
    layouts: &'m [Option<Loaded>],
    stores: &'m [Option<Rc<ViewStore>>],
    /// The data of all the page's layouts, and where in them `layouts` starts.
    all_layouts: &'m [Option<Loaded>],
    start: usize,
}

impl<S> Clone for Chain<'_, S> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<S> Copy for Chain<'_, S> {}

impl<'m, S> Chain<'m, S> {
    /// The outermost level, its layout's data and store, and the levels below.
    #[expect(clippy::type_complexity)]
    fn split_first(
        self,
    ) -> Option<(
        (&'m Level<S>, &'m Option<Loaded>, &'m Option<Rc<ViewStore>>),
        Self,
    )> {
        let (level, levels) = self.levels.split_first()?;
        let (loaded, layouts) = self.layouts.split_first()?;
        let (store, stores) = self.stores.split_first()?;
        Some((
            (level, loaded, store),
            Chain {
                levels,
                layouts,
                stores,
                all_layouts: self.all_layouts,
                start: self.start + 1,
            },
        ))
    }

    /// The data of the layouts above the outermost level.
    fn above(self) -> &'m [Option<Loaded>] {
        &self.all_layouts[..self.start]
    }
}

fn render_levels<S>(
    ui: &mut Ui,
    state: &S,
    frame: &Frame<'_>,
    chain: Chain<'_, S>,
    leaf: &mut dyn FnMut(&mut Ui),
    handoff: Option<Handoff<'_>>,
) {
    let above = chain.above();
    let Some(((level, loaded, store), below_chain)) = chain.split_first() else {
        return leaf(ui);
    };
    let (mut here, mut below) = match handoff {
        Some(handoff) if handoff.depth == 0 => (Some(handoff.transition), None),
        Some(handoff) => (
            None,
            Some(Handoff {
                depth: handoff.depth - 1,
                ..handoff
            }),
        ),
        None => (None, None),
    };
    match (&level.layout, loaded, store) {
        (Some(layout), Some(loaded), Some(store)) => {
            let mut child =
                |ui: &mut Ui| render_levels(ui, state, frame, below_chain, leaf, below.take());
            let children = Children {
                inner: &mut child,
                uses_transitions: &layout.transitions,
                transition: here.as_mut().map(|t| t as &mut dyn TransitionHandle),
            };
            // Borrowed mutably only if the view asks for it: else the views below can read it.
            if layout.data_mut {
                let mut data = loaded.data.borrow_mut();
                let data = ViewData::Mut(&mut **data);
                (layout.view)(
                    ui,
                    children,
                    &ViewContext::new(state, frame, data, store, above),
                );
            } else {
                let data = loaded.data.borrow();
                let data = ViewData::Shared(&**data);
                (layout.view)(
                    ui,
                    children,
                    &ViewContext::new(state, frame, data, store, above),
                );
            }
        }
        _ => render_levels(ui, state, frame, below_chain, leaf, below),
    }
}

fn default_error_view(ui: &mut Ui, nav: Nav<'_>, error: &RouteError) {
    ui.vertical_centered(|ui| {
        ui.add_space(32.0);
        ui.heading(error.status.to_string());
        ui.label(&error.message);
        ui.add_space(8.0);
        nav.link(ui, "Go home", "/");
    });
}

/// Picks the nearest error page above the level that failed. An error page is
/// rendered inside the layout of its own level, so a failing layout falls back to its parent's.
pub(crate) fn error_view<S>(
    levels: &[Rc<Level<S>>],
    failed: usize,
    error: RouteError,
) -> Content<S> {
    let boundary = (0..failed.min(levels.len()))
        .rev()
        .find_map(|level| Some((level + 1, levels[level].error.clone()?)));
    let (depth, view) = match boundary {
        Some((depth, view)) => (depth, Some(view)),
        // The root level itself failed: show its error page without any layout.
        None if failed == 0 => (0, levels.first().and_then(|l| l.error.clone())),
        None => (1, None),
    };
    Content::Error { depth, view, error }
}
