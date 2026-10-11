//! `/blog/{slug}/print`: the same post without the blog's sidebar. The page resets its layouts
//! to the root layout, like SvelteKit's `+page@.svelte`.

use egui_kit::extract::Nav;

use super::page::Data;

pub fn view(ui: &mut egui::Ui, data: &Data, nav: Nav<'_>) {
    ui.vertical_centered(|ui| {
        ui.set_max_width(560.0);
        ui.heading(&data.post.title);
        ui.weak(format!("by {}", data.author.name));
        ui.add_space(12.0);
        ui.label(&data.post.body);
        ui.add_space(12.0);
        nav.link(ui, "Back to the post", &format!("/blog/{}", data.post.slug));
    });
}
