//! `/gallery/{id}`: what a modal's url shows when it is loaded for real.

use egui_kit::extract::{Nav, Path, State};
use egui_kit::{Rejection, error};

use crate::routes::gallery::swatch;
use crate::state::{Db, Photo};

pub struct Data {
    photo: Photo,
}

pub async fn load(Path(id): Path<u32>, State(db): State<Db>) -> Result<Data, Rejection> {
    let photo = db
        .photo(id)
        .cloned()
        .ok_or_else(|| error(404, format!("no photo #{id}")))?;
    Ok(Data { photo })
}

pub fn view(ui: &mut egui::Ui, data: &Data, nav: Nav<'_>) {
    ui.heading(&data.photo.title);
    swatch(ui, &data.photo, 400.0);
    nav.link(ui, "All photos", "/gallery");
}
