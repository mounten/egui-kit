//! `/`: a page without a `load`.

use egui_kit::extract::Nav;

pub fn view(ui: &mut egui::Ui, nav: Nav<'_>) {
    ui.heading("egui-kit");
    ui.label("SvelteKit-style routing for egui. Hover a link to preload it, then try these:");
    ui.add_space(8.0);

    let links = [
        ("/blog", "Blog: layout load, the index reads it with Parent"),
        (
            "/blog?q=layout",
            "Typed query params; typed search text survives back/forward",
        ),
        (
            "/blog/hello-egui-kit",
            "A slow post: hover first, then click (preloading)",
        ),
        (
            "/blog/hello-egui-kit/print",
            "Breaks out of the blog layout",
        ),
        ("/blog/missing", "Missing post → the blog's own error page"),
        ("/blog/a/b/c", "Unknown path below /blog → blog 404"),
        ("/gallery", "Shallow routing and scroll restoration"),
        (
            "/gallery/2",
            "A photo's own page (what a reload of a modal url shows)",
        ),
        ("/authors/1", "{id=int} matcher"),
        ("/authors/ferris", "…and the {name} route next to it"),
        ("/about", "Optional {?lang=lang} parameter"),
        ("/de/about", "…with a language"),
        (
            "/admin",
            "Hook redirects to /login; drafts block navigation; publishing invalidates",
        ),
        ("/nowhere", "Unknown path → root 404"),
    ];
    egui::Grid::new("links").striped(true).show(ui, |ui| {
        for (href, description) in links {
            nav.link(ui, href, href);
            ui.label(description);
            ui.end_row();
        }
    });

    ui.add_space(8.0);
    ui.weak("Mouse back/forward buttons and Alt+←/→ work natively; on the web the browser's buttons do.");
}
