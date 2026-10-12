//! `/admin`: write and publish posts. Leaving with an unsaved draft asks for confirmation
//! (SvelteKit's `beforeNavigate` + `cancel()`), also for the back button.
//!
//! The draft is `PageState`: the user's input, separate from the loaded data, so it survives
//! the page's reloads.
//!
//! Publishing is a `Command` (SvelteKit's remote `command`), kept in the draft: it runs in the
//! background while the view shows that it's pending, and refreshes everything that depends on
//! the posts once done, including the post count query shown here and in the top bar.

use egui_kit::Rejection;
use egui_kit::extract::{Nav, PageState, State};
use egui_kit::remote::{self, Command, QueryState};

use crate::state::{AppState, POSTS, Session};

pub struct Data {
    user: String,
}

pub struct Draft {
    title: String,
    body: String,
    publish: Command<String, String>,
}

impl Default for Draft {
    fn default() -> Self {
        Self {
            title: String::new(),
            body: String::new(),
            publish: remote::command().refreshes(POSTS),
        }
    }
}

pub async fn load(State(session): State<Session>) -> Result<Data, Rejection> {
    Ok(Data {
        // The hook guarantees a user.
        user: session.user().unwrap_or_default(),
    })
}

pub fn view(
    ui: &mut egui::Ui,
    data: &Data,
    mut draft: PageState<'_, Draft>,
    State(state): State<&AppState>,
    nav: Nav<'_>,
) {
    let session: &Session = &state.session;
    ui.heading("Admin");
    ui.horizontal(|ui| {
        ui.label(format!("Welcome, {}!", data.user));
        let posts = &state.post_count;
        match posts.state() {
            QueryState::Loading => ui.label("Counting posts…"),
            QueryState::Ready(count) => ui.label(format!("There are {count} posts.")),
            QueryState::Failed(error) => ui.label(format!("Counting failed: {error}")),
        };
        if posts.is_fetching() {
            ui.spinner();
        }
    });
    if ui.button("Log out").clicked() {
        session.logout();
        nav.goto("/");
    }

    ui.separator();
    ui.strong("New post");
    let pending = draft.publish.is_pending();
    ui.add_enabled_ui(!pending, |ui| {
        ui.add(egui::TextEdit::singleline(&mut draft.title).hint_text("Title"));
        ui.add(egui::TextEdit::multiline(&mut draft.body).hint_text("Write something…"));
    });
    ui.horizontal(|ui| {
        let can_publish = !draft.title.trim().is_empty() && !pending;
        if ui
            .add_enabled(can_publish, egui::Button::new("Publish"))
            .clicked()
        {
            draft.publish.run(crate::remote::publish(
                state.db.clone(),
                draft.title.trim().to_owned(),
                draft.body.trim().to_owned(),
            ));
        }
        if pending {
            ui.spinner();
            ui.weak("Publishing…");
        }
    });
    if let Some(error) = draft.publish.error() {
        ui.colored_label(ui.visuals().error_fg_color, &*error);
    }
    // The command already refreshed what depends on the posts.
    if let Some(Ok(slug)) = draft.publish.take_result() {
        *draft = Draft::default();
        nav.goto(format!("/blog/{slug}"));
    }

    // Decided at the end of the frame, after a possible publish emptied the draft.
    if !draft.title.trim().is_empty() || !draft.body.trim().is_empty() {
        nav.block_navigation();
    }
    if let Some(blocked) = nav.blocked_navigation().filter(|_| !nav.is_outgoing()) {
        let to = blocked.to.clone();
        let modal = egui::Modal::new(egui::Id::new("discard_draft")).show(ui.ctx(), |ui| {
            ui.heading("Discard your draft?");
            ui.label(format!("You are leaving for {to}."));
            ui.horizontal(|ui| {
                (
                    ui.button("Discard").clicked(),
                    ui.button("Keep editing").clicked(),
                )
            })
            .inner
        });
        let (discard, keep) = modal.inner;
        if discard {
            nav.confirm_navigation();
        } else if keep || modal.should_close() {
            nav.cancel_navigation();
        }
    }
}
