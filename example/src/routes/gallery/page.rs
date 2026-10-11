//! `/gallery`

use core::time::Duration;

use egui_kit::Rejection;
use egui_kit::extract::{HistoryState, Nav, Scroll, State};

use super::swatch;
use crate::state::{Db, Photo};
use crate::util::sleep;

/// The page state of a history entry with an open photo.
pub struct OpenPhoto(u32);

pub struct Data {
    photos: Vec<Photo>,
}

pub async fn load(State(db): State<Db>) -> Result<Data, Rejection> {
    // Slow on purpose: opening and closing photos never waits for this.
    sleep(Duration::from_millis(600)).await;
    Ok(Data {
        photos: db.photos().to_vec(),
    })
}

pub fn view(
    ui: &mut egui::Ui,
    data: &Data,
    nav: Nav<'_>,
    scroll: Scroll<'_>,
    HistoryState(open): HistoryState<&OpenPhoto>,
) {
    ui.heading("Gallery");
    ui.label(
        "Photos open in a modal with their own url and history entry. Use back to close them.",
    );
    ui.add_space(8.0);

    scroll.show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            for photo in &data.photos {
                if swatch(ui, photo, 120.0)
                    .on_hover_text(&photo.title)
                    .clicked()
                {
                    nav.push_state(format!("/gallery/{}", photo.id), OpenPhoto(photo.id));
                }
            }
        });
    });

    // The modal lives in its own egui layer, which a view transition can't fade out.
    let open = open.map(|open| open.0).filter(|_| !nav.is_outgoing());
    if let Some(photo) = open.and_then(|id| data.photos.iter().find(|photo| photo.id == id)) {
        let modal = egui::Modal::new(egui::Id::new("photo_modal")).show(ui.ctx(), |ui| {
            ui.heading(&photo.title);
            swatch(ui, photo, 320.0);
            ui.horizontal(|ui| {
                let previous = data.photos.iter().rev().find(|p| p.id < photo.id);
                let next = data.photos.iter().find(|p| p.id > photo.id);
                // Switching photos replaces the entry, so back still closes the modal.
                for (label, target) in [("⏴ Previous", previous), ("Next ⏵", next)] {
                    if ui
                        .add_enabled(target.is_some(), egui::Button::new(label))
                        .clicked()
                    {
                        let target = target.expect("button is enabled");
                        nav.replace_state(format!("/gallery/{}", target.id), OpenPhoto(target.id));
                    }
                }
                ui.separator();
                if ui.button("Open page").clicked() {
                    nav.goto(format!("/gallery/{}", photo.id));
                }
                ui.button("Close").clicked()
            })
            .inner
        });
        if modal.inner || modal.should_close() {
            nav.back();
        }
    }
}
