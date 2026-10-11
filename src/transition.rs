//! View transitions between pages.
//!
//! The router keeps the outgoing page alive for the duration of the transition and renders it next
//! to the incoming one. A [`ViewTransition`] (used in a layout, in place of [`Outlet::show`])
//! decides where and how opaque each of the two is drawn.
//!
//! The transition starts once all `load`s finished, right before the new page is shown. Layouts
//! shared by both pages stay put: only the layout closest to the page that both routes share
//! animates.

use core::cell::Cell;
use std::rc::Rc;

use egui::{Id, Rect, Ui, UiBuilder, Vec2};

use crate::navigation::Direction;
use crate::page::Outlet;

/// How one of the two pages is drawn in a frame of the transition
/// (`::view-transition-old` / `::view-transition-new`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layer {
    /// In points, relative to the outlet.
    pub offset: Vec2,
    pub opacity: f32,
}

impl Layer {
    pub const VISIBLE: Self = Self {
        offset: Vec2::ZERO,
        opacity: 1.0,
    };
}

/// The input of a transition effect for one frame.
#[derive(Debug, Clone, Copy)]
pub struct TransitionFrame {
    /// Linear progress, from `0.0` to `1.0`. Apply your own easing.
    pub t: f32,
    pub direction: Direction,
    /// The size of the outlet.
    pub size: Vec2,
}

type Effect = Rc<dyn Fn(&TransitionFrame) -> (Layer, Layer)>;

/// Animates page changes inside a layout. Use it instead of [`Outlet::show`]:
///
/// ```no_run
/// # use egui_kit::ViewTransition;
/// # use egui_kit::page::Outlet;
/// fn layout(ui: &mut egui::Ui, outlet: Outlet) {
///     ui.heading("My app"); // shared chrome: stays put
///     ViewTransition::slide().show(ui, outlet);
/// }
/// ```
///
/// When the user prefers reduced motion (the browser's `prefers-reduced-motion` on the web, or
/// [`ViewTransition::reduced_motion`]), offsets are dropped and only the fading remains, like the
/// blog post's `@media (prefers-reduced-motion)` suggestion.
#[derive(Clone)]
pub struct ViewTransition {
    duration: f32,
    effect: Effect,
    reduced_motion: Option<bool>,
}

impl ViewTransition {
    /// The browser's default: a crossfade.
    pub fn fade() -> Self {
        Self::custom(|frame| {
            let t = ease_in_out(frame.t);
            (
                Layer {
                    opacity: 1.0 - t,
                    ..Layer::VISIBLE
                },
                Layer {
                    opacity: t,
                    ..Layer::VISIBLE
                },
            )
        })
        .duration(0.2)
    }

    /// The blog post's transition: the old page fades out quickly while sliding away, the new one
    /// fades in a bit later while sliding in. Mirrored when going back.
    pub fn slide() -> Self {
        Self::slide_by(30.0)
    }

    /// [`ViewTransition::slide`] with a custom distance in points.
    pub fn slide_by(distance: f32) -> Self {
        Self::custom(move |frame| {
            let shift = distance * frame.direction.sign();
            let t = ease_out(frame.t);
            let old = Layer {
                // 90ms of 300ms
                opacity: 1.0 - (frame.t / 0.3).min(1.0),
                offset: Vec2::new(-shift * t, 0.0),
            };
            let new = Layer {
                // 210ms after a 90ms delay
                opacity: ((frame.t - 0.3) / 0.7).clamp(0.0, 1.0),
                offset: Vec2::new(shift * (1.0 - t), 0.0),
            };
            (old, new)
        })
        .duration(0.3)
    }

    /// A full-width push, like navigation stacks on mobile.
    pub fn push() -> Self {
        Self::custom(|frame| {
            let shift = frame.size.x * frame.direction.sign();
            let t = ease_in_out(frame.t);
            (
                Layer {
                    offset: Vec2::new(-shift * t, 0.0),
                    opacity: if shift == 0.0 { 1.0 - t } else { 1.0 },
                },
                Layer {
                    offset: Vec2::new(shift * (1.0 - t), 0.0),
                    opacity: if shift == 0.0 { t } else { 1.0 },
                },
            )
        })
        .duration(0.35)
    }

    /// Your own effect: return how the old and the new page are drawn for a frame.
    pub fn custom(effect: impl Fn(&TransitionFrame) -> (Layer, Layer) + 'static) -> Self {
        Self {
            duration: 0.3,
            effect: Rc::new(effect),
            reduced_motion: None,
        }
    }

    /// In seconds.
    pub fn duration(mut self, seconds: f32) -> Self {
        self.duration = seconds;
        self
    }

    /// Overrides the reduced-motion preference (by default: the browser's setting on the web,
    /// off on native).
    pub fn reduced_motion(mut self, reduced: bool) -> Self {
        self.reduced_motion = Some(reduced);
        self
    }

    /// Shows the outlet, animating from the previous page if this layout is where the current
    /// navigation transitions.
    pub fn show(&self, ui: &mut Ui, mut outlet: Outlet<'_>) {
        outlet.uses_transitions.set(true);
        let rect = ui.available_rect_before_wrap();
        let outlet_id = ui.id().with("egui_kit_outlet");

        let frame = outlet.transition.as_deref().and_then(|transition| {
            let state = transition.state();
            let now = ui.input(|i| i.time);
            let started = state.started.get().unwrap_or_else(|| {
                state.started.set(Some(now));
                now
            });
            state.claimed.set(true);
            let t = if self.duration > 0.0 {
                ((now - started) as f32 / self.duration).clamp(0.0, 1.0)
            } else {
                1.0
            };
            if t >= 1.0 {
                state.done.set(true);
                return None;
            }
            Some(TransitionFrame {
                t,
                direction: transition.direction(),
                size: rect.size(),
            })
        });

        let Some(frame) = frame else {
            // Same id as during transitions, so widget state survives the end of the animation.
            let mut page = ui.new_child(UiBuilder::new().id(outlet_id).max_rect(rect));
            outlet.show(&mut page);
            ui.advance_cursor_after_rect(page.min_rect());
            return;
        };

        ui.ctx().request_repaint();
        let (mut old, mut new) = (self.effect)(&frame);
        if self.reduced_motion.unwrap_or_else(prefers_reduced_motion) {
            old.offset = Vec2::ZERO;
            new.offset = Vec2::ZERO;
        }
        let clip = rect.intersect(ui.clip_rect());

        // The outgoing page is drawn first, so the incoming one ends up on top.
        let mut previous_ui = layer_ui(ui, outlet_id.with("previous"), rect, clip, old);
        if let Some(transition) = outlet.transition.as_deref_mut() {
            transition.render_previous(&mut previous_ui);
        }

        let mut page = layer_ui(ui, outlet_id, rect, clip, new);
        outlet.show(&mut page);
        ui.advance_cursor_after_rect(page.min_rect().translate(-new.offset));
    }
}

fn layer_ui(ui: &mut Ui, id: Id, rect: Rect, clip: Rect, layer: Layer) -> Ui {
    let mut child = ui.new_child(
        UiBuilder::new()
            .id(id)
            .max_rect(rect.translate(layer.offset)),
    );
    child.set_clip_rect(clip);
    child.multiply_opacity(layer.opacity.clamp(0.0, 1.0));
    child
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

fn ease_in_out(t: f32) -> f32 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

/// Whether the user asked their system for reduced motion (`prefers-reduced-motion: reduce`).
/// Always `false` on native, where there is no portable way to ask.
pub fn prefers_reduced_motion() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()
            .and_then(|window| {
                window
                    .match_media("(prefers-reduced-motion: reduce)")
                    .ok()
                    .flatten()
            })
            .is_some_and(|query| query.matches())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        false
    }
}

/// Bookkeeping of a running transition, shared with the outlet that runs it.
#[derive(Default)]
pub(crate) struct TransitionState {
    pub(crate) started: Cell<Option<f64>>,
    /// Whether a [`ViewTransition`] ran it this frame; unclaimed transitions end right away.
    pub(crate) claimed: Cell<bool>,
    pub(crate) done: Cell<bool>,
}

/// What the outlet of the transitioning layout gets on top of the new page.
pub(crate) struct OutletTransition<'a> {
    pub(crate) previous: &'a mut dyn FnMut(&mut Ui),
    pub(crate) state: &'a TransitionState,
    pub(crate) direction: Direction,
}

/// [`OutletTransition`] behind a trait object, so an [`Outlet`]'s lifetime doesn't have to match it.
pub(crate) trait TransitionHandle {
    fn render_previous(&mut self, ui: &mut Ui);
    fn state(&self) -> &TransitionState;
    fn direction(&self) -> Direction;
}

impl TransitionHandle for OutletTransition<'_> {
    fn render_previous(&mut self, ui: &mut Ui) {
        (self.previous)(ui);
    }

    fn state(&self) -> &TransitionState {
        self.state
    }

    fn direction(&self) -> Direction {
        self.direction
    }
}
