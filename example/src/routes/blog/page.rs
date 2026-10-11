//! `/blog?q=…&tag=…`: typed query parameters, the layout's data through `Parent`, and a search
//! box whose typed-but-not-submitted text comes back with back/forward (a `Snapshot`).

use core::time::Duration;

use egui_kit::Rejection;
use egui_kit::extract::{Nav, Parent, Query, Scroll, Snapshot};
use serde::{Deserialize, Serialize};

use super::layout;
use crate::state::Post;
use crate::util::sleep;

#[derive(Deserialize, Serialize, Default)]
pub struct BlogQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    q: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tag: Option<String>,
}

pub struct Data {
    posts: Vec<Post>,
    tag: Option<String>,
    query: Option<String>,
}

/// The search box: what the user typed, restored by back/forward.
#[derive(Default)]
pub struct Search {
    /// Starts from the query.
    text: Option<String>,
}

pub async fn load(Query(query): Query<BlogQuery>, parent: Parent) -> Result<Data, Rejection> {
    // The posts come from the blog layout, which loads them anyway.
    let layout::Data { posts, .. } = parent.get::<layout::Data>().await?;
    sleep(Duration::from_millis(300)).await;

    let needle = query.q.as_deref().unwrap_or_default().to_lowercase();
    let posts = posts
        .into_iter()
        .filter(|post| query.tag.as_ref().is_none_or(|tag| post.tags.contains(tag)))
        .filter(|post| {
            post.title.to_lowercase().contains(&needle)
                || post.body.to_lowercase().contains(&needle)
        })
        .collect();

    Ok(Data {
        posts,
        tag: query.tag,
        query: query.q,
    })
}

pub fn view(
    ui: &mut egui::Ui,
    data: &Data,
    mut search: Snapshot<'_, Search>,
    nav: Nav<'_>,
    scroll: Scroll<'_>,
) {
    ui.heading(match &data.tag {
        Some(tag) => format!("Posts tagged #{tag}"),
        None => "All posts".to_owned(),
    });

    ui.horizontal(|ui| {
        let text = search
            .text
            .get_or_insert_with(|| data.query.clone().unwrap_or_default());
        let response = ui.add(egui::TextEdit::singleline(text).hint_text("Search…"));
        let submitted = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if ui.button("Search").clicked() || submitted {
            let query = BlogQuery {
                q: Some(text.trim().to_owned()).filter(|q| !q.is_empty()),
                tag: data.tag.clone(),
            };
            nav.goto(url(&query));
        }
        if data.tag.is_some() && ui.button("Clear tag").clicked() {
            nav.goto("/blog");
        }
    });
    ui.separator();

    scroll.show(ui, |ui| {
        if data.posts.is_empty() {
            ui.weak("No posts found.");
        }
        for post in &data.posts {
            nav.link(
                ui,
                egui::RichText::new(&post.title).heading(),
                &format!("/blog/{}", post.slug),
            );
            ui.label(&post.body);
            ui.weak(
                post.tags
                    .iter()
                    .map(|t| format!("#{t}"))
                    .collect::<Vec<_>>()
                    .join(" "),
            );
            ui.add_space(12.0);
        }
    });
}

fn url(query: &BlogQuery) -> String {
    match serde_urlencoded::to_string(query) {
        Ok(qs) if !qs.is_empty() => format!("/blog?{qs}"),
        _ => "/blog".to_owned(),
    }
}
