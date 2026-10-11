//! `/login?redirect=…`

use egui_kit::Rejection;
use egui_kit::extract::{Nav, PageState, Query, State};
use serde::Deserialize;

use crate::state::Session;

#[derive(Deserialize)]
pub struct LoginQuery {
    redirect: Option<String>,
}

pub struct Data {
    redirect: String,
}

/// What the user typed.
#[derive(Default)]
pub struct Form {
    name: String,
}

pub async fn load(Query(query): Query<LoginQuery>) -> Result<Data, Rejection> {
    Ok(Data {
        // Only allow app-relative redirects.
        redirect: query
            .redirect
            .filter(|r| r.starts_with('/'))
            .unwrap_or_else(|| "/".into()),
    })
}

pub fn view(
    ui: &mut egui::Ui,
    data: &Data,
    mut form: PageState<'_, Form>,
    State(session): State<&Session>,
    nav: Nav<'_>,
) {
    ui.heading("Log in");
    if data.redirect != "/" {
        ui.label(format!("You need to log in to see {}.", data.redirect));
    }
    ui.add_space(8.0);

    let response = ui.horizontal(|ui| {
        ui.label("Name:");
        ui.text_edit_singleline(&mut form.name)
    });
    let submitted = response.inner.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    let name = form.name.trim();

    if (ui
        .add_enabled(!name.is_empty(), egui::Button::new("Log in"))
        .clicked()
        || submitted)
        && !name.is_empty()
    {
        session.login(name.to_owned());
        nav.replace(data.redirect.clone());
    }
}
