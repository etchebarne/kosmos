mod app;
mod assets;
mod bottom_bar;
mod drag;
mod header;
mod icon;
mod pane_tree;
mod persistence;
mod workspace;

use gpui::{
    App, AppContext, Application, Bounds, WindowBounds, WindowDecorations, WindowOptions, px, size,
};

use crate::app::IdeApp;
use crate::assets::AppAssets;

fn main() {
    Application::new()
        .with_assets(AppAssets)
        .run(|cx: &mut App| {
            let window_bounds = persistence::load_window_bounds().unwrap_or_else(|| {
                let bounds = Bounds::centered(None, size(px(900.0), px(600.0)), cx);
                WindowBounds::Windowed(bounds)
            });
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(window_bounds),
                    titlebar: None,
                    window_decorations: Some(WindowDecorations::Client),
                    ..Default::default()
                },
                |window, cx| {
                    let entity = cx.new(|_| IdeApp::new());
                    entity.update(cx, |app, cx| app.start_observing_window(window, cx));
                    entity
                },
            )
            .unwrap();

            cx.activate(true);
        });
}
