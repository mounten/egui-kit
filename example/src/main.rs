//! egui-kit example app.
//!
//! Native:  `cargo run -p egui-kit-example`
//! Web:     `cd example && trunk serve` (the url bar and browser back/forward follow the router)

mod hooks;
mod routes;
mod state;
mod util;

use egui_kit::{History, RouterApp};
use state::AppState;

struct App {
    router: RouterApp<AppState>,
}

impl App {
    fn new(history: impl History + 'static) -> Self {
        Self {
            router: routes::router()
                .with_state_and_history(AppState::new(), history)
                // SvelteKit's `afterNavigate`: here it just counts.
                .after_navigate(|_navigation, state: &AppState| {
                    state.navigations.set(state.navigations.get() + 1);
                }),
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.router.ui(ui);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    eframe::run_native(
        "egui-kit example",
        eframe::NativeOptions::default(),
        Box::new(|_cc| Ok(Box::new(App::new(egui_kit::default_history())))),
    )
}

#[cfg(target_arch = "wasm32")]
fn main() {
    use wasm_bindgen::JsCast as _;

    wasm_bindgen_futures::spawn_local(async {
        let canvas = web_sys::window()
            .and_then(|window| window.document())
            .and_then(|document| document.get_element_by_id("the_canvas_id"))
            .and_then(|element| element.dyn_into::<web_sys::HtmlCanvasElement>().ok())
            .expect("missing #the_canvas_id canvas");
        // Hovered links show their url in the browser's status bar, like real ones.
        let history =
            egui_kit::BrowserHistory::new(egui_kit::HistoryMode::Path).link_preview(&canvas);

        eframe::WebRunner::new()
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(|_cc| Ok(Box::new(App::new(history)))),
            )
            .await
            .expect("failed to start eframe");
    });
}
