//! `/admin`: write and publish posts. Leaving with an unsaved draft asks for confirmation
//! (SvelteKit's `beforeNavigate` + `cancel()`), also for the back button.
//!
//! The draft is `PageState`: the user's input, separate from the loaded data, so it survives
//! the page's reloads (publishing reloads the post count).

use egui_kit::Rejection;
use egui_kit::extract::{Nav, PageState, State};

use crate::state::{Db, POSTS, Session};

pub struct Data {
    user: String,
    post_count: usize,
}

#[derive(Default)]
pub struct Draft {
    title: String,
    body: String,
}

pub async fn load(State(session): State<Session>, State(db): State<Db>) -> Result<Data, Rejection> {
    Ok(Data {
        // The hook guarantees a user.
        user: session.user().unwrap_or_default(),
        post_count: db.posts().len(),
    })
}

pub fn view(
    ui: &mut egui::Ui,
    data: &Data,
    mut draft: PageState<'_, Draft>,
    State(session): State<&Session>,
    State(db): State<&Db>,
    nav: Nav<'_>,
) {
    ui.heading("Admin");
    ui.label(format!(
        "Welcome, {}! There are {} posts.",
        data.user, data.post_count
    ));
    if ui.button("Log out").clicked() {
        session.logout();
        nav.goto("/");
    }

    ui.separator();
    ui.strong("New post");
    ui.add(egui::TextEdit::singleline(&mut draft.title).hint_text("Title"));
    ui.add(egui::TextEdit::multiline(&mut draft.body).hint_text("Write something…"));
    let can_publish = !draft.title.trim().is_empty();
    if ui
        .add_enabled(can_publish, egui::Button::new("Publish"))
        .clicked()
    {
        let slug = db.publish(draft.title.trim(), draft.body.trim(), 1);
        *draft = Draft::default();
        // Reloads every load that depends on the posts: this page's count, the blog sidebar…
        nav.invalidate(POSTS);
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
