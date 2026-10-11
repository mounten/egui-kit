//! `/{?lang=lang}/about`: an optional parameter, restricted by the `lang` matcher.
//! `/about`, `/en/about`, `/de/about` and `/nl/about` all match; `/fr/about` is a 404.

use egui_kit::Rejection;
use egui_kit::extract::{Nav, Path};

pub const LANGUAGES: [&str; 3] = ["en", "de", "nl"];

pub struct Data {
    lang: &'static str,
}

pub async fn load(Path(lang): Path<Option<String>>) -> Result<Data, Rejection> {
    let lang = LANGUAGES
        .into_iter()
        .find(|l| Some(*l) == lang.as_deref())
        .unwrap_or("en");
    Ok(Data { lang })
}

pub fn view(ui: &mut egui::Ui, data: &Data, nav: Nav<'_>) {
    ui.heading(match data.lang {
        "de" => "Über egui-kit",
        "nl" => "Over egui-kit",
        _ => "About egui-kit",
    });
    ui.label(match data.lang {
        "de" => "Ein von SvelteKit inspirierter Router für egui.",
        "nl" => "Een door SvelteKit geïnspireerde router voor egui.",
        _ => "A SvelteKit-inspired router for egui.",
    });
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        nav.nav_link(ui, "English", "/about");
        nav.nav_link(ui, "Deutsch", "/de/about");
        nav.nav_link(ui, "Nederlands", "/nl/about");
    });
}
