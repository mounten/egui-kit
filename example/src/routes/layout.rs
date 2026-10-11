//! Root layout: a top bar with history buttons, the url and navigation links.
//!
//! The top bar is shared by all pages, so it stays put while the pages below it transition
//! (what the SvelteKit blog post achieves with `view-transition-name: header`).

use egui::{Align, Button, Panel, RichText};
use egui_kit::extract::{Nav, PersistentState, Remote, State};
use egui_kit::page::Outlet;

use crate::remote::PostCount;
use crate::state::{AppState, TransitionStyle};

pub fn view(
    ui: &mut egui::Ui,
    outlet: Outlet<'_>,
    State(state): State<&AppState>,
    mut style: PersistentState<'_, TransitionStyle>,
    remote: Remote<'_, AppState>,
    nav: Nav<'_>,
) {
    Panel::top("top_bar").show(ui, |ui| {
        ui.horizontal(|ui| {
            if ui
                .add_enabled(nav.can_go_back(), Button::new("⏴"))
                .on_hover_text("Back")
                .clicked()
            {
                nav.back();
            }
            if ui
                .add_enabled(nav.can_go_forward(), Button::new("⏵"))
                .on_hover_text("Forward")
                .clicked()
            {
                nav.forward();
            }
            if ui
                .button("⟳")
                .on_hover_text("Reload (rerun load functions)")
                .clicked()
            {
                nav.invalidate_all();
            }
            ui.label(RichText::new(nav.url()).monospace());
            if nav.is_loading() {
                ui.spinner();
            }

            ui.separator();
            nav.nav_link(ui, "Home", "/");
            nav.nav_link(ui, "Blog", "/blog");
            nav.nav_link(ui, "Gallery", "/gallery");
            nav.nav_link(ui, "Admin", "/admin");

            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                let navigations = state.navigations.get();
                ui.weak(format!("{navigations} navigations"));
                // A query: the top bar fetches it itself, it's not part of any route's `load`.
                if let Some(count) = remote.query(PostCount).current() {
                    ui.weak(format!("{count} posts ·"));
                }

                egui::ComboBox::from_id_salt("transition")
                    .selected_text(format!("{:?}", *style))
                    .show_ui(ui, |ui| {
                        for option in TransitionStyle::ALL {
                            ui.selectable_value(&mut *style, option, format!("{option:?}"));
                        }
                    });
                ui.label("Transition:");
                if let Some(user) = state.session.user() {
                    ui.label(format!("👤 {user}"));
                }
            });
        });
    });

    let transition = style.view_transition();
    // Written back now, so the blog layout below sees a new choice in the same frame.
    drop(style);
    egui::CentralPanel::default().show(ui, |ui| transition.show(ui, outlet));
}
