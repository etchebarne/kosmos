mod app;
mod delegates;

use gpui::{
    App, AppContext, Application, Bounds, WindowBounds, WindowDecorations, WindowOptions, px, size,
};
use icons::AppAssets;

use crate::app::KosmosApp;

fn main() {
    Application::new()
        .with_assets(AppAssets)
        .run(|cx: &mut App| {
            cx.set_global(theme::Theme::dark());
            cx.set_global(settings::Settings::load());
            cx.set_global(ui::delegate::SettingsUiState::new());
            ui::components::install_default_keybindings(cx);
            shortcuts::install_defaults(cx);
            let window_bounds = persistence::load_window_bounds().unwrap_or_else(|| {
                let bounds = Bounds::centered(None, size(px(900.0), px(600.0)), cx);
                WindowBounds::Windowed(bounds)
            });
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(window_bounds),
                    titlebar: None,
                    window_decorations: Some(WindowDecorations::Client),
                    window_min_size: Some(size(px(800.0), px(600.0))),
                    ..Default::default()
                },
                |window, cx| {
                    let entity = cx.new(KosmosApp::new);
                    entity.update(cx, |app, cx| app.start_observing_window(window, cx));
                    entity
                },
            )
            .unwrap();

            cx.activate(true);
        });
}
