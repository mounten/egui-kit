//! Queries and commands, SvelteKit's `*.remote.ts`: data and mutations that aren't tied to a
//! route. Views use them through the `Remote` extractor.

use core::time::Duration;

use egui_kit::RouteError;
use egui_kit::remote::{Command, Query};

use crate::state::{AppState, POSTS};
use crate::util::sleep;

/// How many posts there are, for the top bar and the admin page. Reruns when the posts change.
#[derive(Clone, PartialEq, Eq)]
pub struct PostCount;

impl Query<AppState> for PostCount {
    type Output = usize;

    async fn run(self, state: AppState) -> Result<usize, RouteError> {
        sleep(Duration::from_millis(200)).await;
        Ok(state.db.posts().len())
    }

    fn depends(&self) -> Vec<String> {
        vec![POSTS.into()]
    }
}

/// Publishes a post and returns its slug.
pub struct Publish {
    pub title: String,
    pub body: String,
}

impl Command<AppState> for Publish {
    type Output = String;

    async fn run(self, state: AppState) -> Result<String, RouteError> {
        sleep(Duration::from_millis(500)).await;
        let author_id = 1;
        state
            .db
            .publish(&self.title, &self.body, author_id)
            .ok_or_else(|| RouteError::new(409, "there is a post with that title already"))
    }

    /// The blog sidebar, the post list, `PostCount`…
    fn refreshes(&self) -> Vec<String> {
        vec![POSTS.into()]
    }
}
