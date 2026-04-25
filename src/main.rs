mod app;
mod assets;
mod bottom_bar;
mod drag;
mod header;
mod icon;
mod pane_tree;
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
            let bounds = Bounds::centered(None, size(px(900.0), px(600.0)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: None,
                    window_decorations: Some(WindowDecorations::Client),
                    ..Default::default()
                },
                |_, cx| cx.new(|_| IdeApp::new()),
            )
            .unwrap();

            cx.activate(true);
        });
}
