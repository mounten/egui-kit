//! `/blog/{slug}`: slow on purpose; hovering a link to a post preloads it, so clicking is instant.

use core::time::Duration;

use egui_kit::extract::{Nav, Path, State};
use egui_kit::{Rejection, error, href};

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
    nav.set_title(format!("{} · egui-kit example", data.post.title));
    ui.heading(&data.post.title);
    ui.horizontal(|ui| {
        ui.label("by");
        let author = href("/authors/{id=int}").param("id", data.author.id);
        nav.link(ui, data.author.name, &author.to_string());
        ui.separator();
        let print = href("/blog/{slug}/print").param("slug", &data.post.slug);
        nav.link(ui, "Print view", &print.to_string());
    });
    ui.add_space(8.0);
    ui.label(&data.post.body);
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        for tag in &data.post.tags {
            let url = href("/blog").query(&[("tag", tag)]);
            nav.link(ui, format!("#{tag}"), &url.to_string());
        }
    });
}
