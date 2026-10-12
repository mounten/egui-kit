//! Queries and commands, SvelteKit's `*.remote.ts`: data and mutations that aren't tied to a
//! route. They are plain values: the post count lives in the app state, the command that
//! publishes a post in the admin page's state.

use core::time::Duration;

use egui_kit::remote::{self, Query};

use crate::state::{Db, POSTS};
use crate::util::sleep;

/// How many posts there are, for the top bar and the admin page. Refetches when the posts
/// change.
pub fn post_count(db: &Db) -> Query<usize, String> {
    let db = db.clone();
    remote::query(move || {
        let db = db.clone();
        async move {
            sleep(Duration::from_millis(200)).await;
            Ok(db.posts().len())
        }
    })
    .key("post_count")
    .depends(POSTS)
}

/// Publishes a post and returns its slug.
pub async fn publish(db: Db, title: String, body: String) -> Result<String, String> {
    sleep(Duration::from_millis(500)).await;
    let author_id = 1;
    db.publish(&title, &body, author_id)
        .ok_or_else(|| "there is a post with that title already".to_owned())
}
