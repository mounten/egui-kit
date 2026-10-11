# egui-kit

[![Crates.io](https://img.shields.io/crates/v/egui-kit.svg)](https://crates.io/crates/egui-kit)
[![Documentation](https://docs.rs/egui-kit/badge.svg)](https://docs.rs/egui-kit)
[![License](https://img.shields.io/crates/l/egui-kit.svg)](#license)

A router for [egui](https://github.com/emilk/egui), inspired by SvelteKit's routing and axum's
API.

Pages have an async `load` and a view that gets the loaded data, both taking typed arguments like
path parameters, the query or the app state. Routers can be nested and have layouts, hooks and
error pages. On the web, the router uses the browser's history.

> [!WARNING]
> egui-kit is still unstable. Until 1.0, new minor versions (`0.x` → `0.y`) may contain breaking
> changes; the [changelog](CHANGELOG.md) lists them.

## Example

```rust
use egui::Ui;
use egui_kit::{Rejection, Router, RouterApp};
use egui_kit::extract::{Nav, Path};
use egui_kit::page::Page;

fn home(ui: &mut Ui, nav: Nav) {
    ui.heading("Home");
    nav.link(ui, "Say hello", "/hello/ferris");
}

async fn load_greeting(Path(name): Path<String>) -> Result<String, Rejection> {
    Ok(format!("Hello, {name}!"))
}

fn greeting(ui: &mut Ui, greeting: &String, nav: Nav) {
    ui.heading(greeting);
    nav.link(ui, "Home", "/");
}

fn app() -> RouterApp<()> {
    Router::new()
        .page("/", Page::new(home))
        .page("/hello/{name}", Page::with_load(load_greeting, greeting))
        .with_state(())
}
```

Call `app.ui(ui)` every frame, e.g. from `eframe::App::ui`. See the
[documentation](https://docs.rs/egui-kit) for everything else, and the
[example](https://github.com/mounten/egui-kit/tree/main/example) app:

```sh
cargo run -p egui-kit-example        # native
cd example && trunk serve            # web
```

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT)
at your option.
