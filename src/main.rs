mod app;
mod drag;
mod pane_tree;

use gpui::{App, AppContext, Application, Bounds, WindowBounds, WindowOptions, px, size};

use crate::app::IdeApp;

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(900.0), px(600.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(|_| IdeApp::new()),
        )
        .unwrap();

        cx.activate(true);
    });
}
