//! `/authors/{id=int}` and `/authors/{name}`: the `int` matcher decides which route a segment
//! belongs to, so `/authors/1` and `/authors/ferris` are different pages showing the same thing.

pub mod id;
pub mod name;

use egui_kit::extract::Nav;

use crate::state::{Author, Db, Post};

pub struct Data {
    author: Author,
    posts: Vec<Post>,
}

impl Data {
    fn new(author: Author, db: &Db) -> Self {
        let posts = db
            .posts()
            .into_iter()
            .filter(|post| post.author_id == author.id)
            .collect();
        Self { author, posts }
    }
}

fn view(ui: &mut egui::Ui, data: &Data, nav: Nav<'_>) {
    ui.heading(data.author.name);
    ui.label(format!("{} posts", data.posts.len()));
    for post in &data.posts {
        nav.link(ui, &post.title, &format!("/blog/{}", post.slug));
    }
}
