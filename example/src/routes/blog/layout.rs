//! Blog layout: a sidebar whose content comes from the layout's own `load`. Hiding the tags is
//! the layout's `PageState`: it stays while moving between the blog's pages.

use egui::Panel;
use egui_kit::Rejection;
use egui_kit::extract::{Nav, PageState, PersistentState, State};
use egui_kit::page::Outlet;

use crate::state::{Db, Post, TransitionStyle};

/// `Clone`, so pages below can read it with `Parent`.
#[derive(Clone)]
pub struct Data {
    pub tags: Vec<String>,
    pub posts: Vec<Post>,
}

#[derive(Default)]
pub struct Sidebar {
    hide_tags: bool,
}

pub async fn load(State(db): State<Db>) -> Result<Data, Rejection> {
    Ok(Data {
        tags: db.tags(),
        posts: db.posts(),
    })
}

/// The sidebar stays while posts transition: within the blog, this layout is the innermost one
/// both pages share, so its `ViewTransition` runs instead of the root layout's.
#[expect(
    clippy::needless_pass_by_value,
    reason = "views take their extractors by value"
)]
pub fn view(
    ui: &mut egui::Ui,
    outlet: Outlet<'_>,
    data: &Data,
    mut sidebar: PageState<'_, Sidebar>,
    style: PersistentState<'_, TransitionStyle>,
    nav: Nav<'_>,
) {
    Panel::left("blog_sidebar")
        .resizable(false)
        .default_size(200.0)
        .show(ui, |ui| {
            ui.strong("Posts");
            for post in &data.posts {
                nav.nav_link(ui, &post.title, &format!("/blog/{}", post.slug));
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.strong("Tags");
                ui.checkbox(&mut sidebar.hide_tags, "hide");
            });
            if !sidebar.hide_tags {
                ui.horizontal_wrapped(|ui| {
                    for tag in &data.tags {
                        nav.link(ui, format!("#{tag}"), &format!("/blog?tag={tag}"));
                    }
                });
            }
        });

    let transition = style.view_transition();
    egui::CentralPanel::default().show(ui, |ui| transition.show(ui, outlet));
}
