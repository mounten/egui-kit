//! [`Scroll`]: a scroll area whose position the router manages like a browser.

use core::cell::Cell;

use egui::Ui;

use super::FromView;
use crate::page::{Frame, ViewContext};

/// The scroll position of a page.
#[derive(Debug, Default)]
pub(crate) struct PageScroll {
    /// Applied once, on the first frame of the page.
    pub(crate) restore: Cell<Option<f32>>,
    pub(crate) offset: Cell<f32>,
}

/// The page's scroll area, an extractor: new pages start at the top (unless navigated to with
/// [`GotoOptions::no_scroll`](crate::GotoOptions::no_scroll)), and back/forward restores where
/// the user was.
///
/// ```no_run
/// # use egui_kit::extract::Scroll;
/// fn view(ui: &mut egui::Ui, scroll: Scroll) {
///     scroll.show(ui, |ui| {
///         for i in 0..100 {
///             ui.label(format!("Row {i}"));
///         }
///     });
/// }
/// ```
#[derive(Clone, Copy)]
pub struct Scroll<'f> {
    pub(crate) frame: &'f Frame<'f>,
}

impl<S: 'static, D> FromView<S, D> for Scroll<'_> {
    type Item<'f> = Scroll<'f>;

    fn from_view<'f>(cx: &'f ViewContext<'_, S>) -> Scroll<'f> {
        Scroll { frame: cx.frame() }
    }
}

impl Scroll<'_> {
    /// Shows a vertical [`egui::ScrollArea`] with the page's content. Use it at most once per page.
    pub fn show<R>(&self, ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
        let frame = self.frame;
        let mut area = egui::ScrollArea::vertical()
            .id_salt(("egui_kit_page_scroll", frame.page_id))
            .auto_shrink(false);
        // An outgoing page is drawn in a different `Ui`, so it keeps its position explicitly.
        let offset = if frame.outgoing {
            Some(frame.scroll.offset.get())
        } else {
            frame.scroll.restore.take()
        };
        if let Some(offset) = offset {
            area = area.vertical_scroll_offset(offset);
        }
        let output = area.show(ui, add_contents);
        if !frame.outgoing {
            frame.scroll.offset.set(output.state.offset.y);
        }
        output.inner
    }
}
