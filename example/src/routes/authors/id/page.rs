//! `/authors/{id=int}`

use egui_kit::extract::{Nav, Path, State};
use egui_kit::{Rejection, error};

use crate::routes::authors::Data;
use crate::state::Db;

pub async fn load(Path(id): Path<u32>, State(db): State<Db>) -> Result<Data, Rejection> {
    let author = db
        .author(id)
        .cloned()
        .ok_or_else(|| error(404, format!("no author #{id}")))?;
    Ok(Data::new(author, &db))
}

pub fn view(ui: &mut egui::Ui, data: &Data, nav: Nav<'_>) {
    crate::routes::authors::view(ui, data, nav);
    ui.weak("Matched /authors/{id=int}");
}
