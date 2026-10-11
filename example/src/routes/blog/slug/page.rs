//! `/blog/{slug}`: slow on purpose; hovering a link to a post preloads it, so clicking is instant.

use core::time::Duration;

use egui_kit::extract::{Nav, Path, State};
use egui_kit::{Rejection, error};

use crate::state::{Author, Db, Post};
use crate::util::sleep;

pub struct Data {
    pub post: Post,
    pub author: Author,
}

pub async fn load(Path(slug): Path<String>, State(db): State<Db>) -> Result<Data, Rejection> {
    sleep(Duration::from_millis(500)).await;

    let post = db
        .post(&slug)
        .ok_or_else(|| error(404, format!("there is no post called `{slug}`")))?;
    let author = db
        .author(post.author_id)
        .cloned()
        .ok_or_else(|| error(500, "post without author"))?;
    Ok(Data { post, author })
}

pub fn view(ui: &mut egui::Ui, data: &Data, nav: Nav<'_>) {
    ui.heading(&data.post.title);
    ui.horizontal(|ui| {
        ui.label("by");
        nav.link(
            ui,
            data.author.name,
            &format!("/authors/{}", data.author.id),
        );
        ui.separator();
        nav.link(ui, "Print view", &format!("/blog/{}/print", data.post.slug));
    });
    ui.add_space(8.0);
    ui.label(&data.post.body);
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        for tag in &data.post.tags {
            nav.link(ui, format!("#{tag}"), &format!("/blog?tag={tag}"));
        }
    });
}
