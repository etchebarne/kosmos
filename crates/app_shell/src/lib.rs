mod app;
mod delegates;

use gpui::{
    App, AppContext, Application, Bounds, WindowBounds, WindowDecorations, WindowOptions, px, size,
};
use icons::AppAssets;

use crate::app::KosmosApp;

const APP_NAME: &str = "Kosmos";
const DEFAULT_APP_ID: &str = "net.etchebarne.Kosmos";

pub fn run() {
    Application::new()
        .with_assets(AppAssets)
        .run(|cx: &mut App| {
            install_globals(cx);
            install_feature_state(cx);
            install_keybindings(cx);
            open_main_window(cx);
            cx.activate(true);
        });
}

fn install_globals(cx: &mut App) {
    cx.set_global(theme::Theme::dark());
    cx.set_global(settings::Settings::load());
    cx.set_global(ui::delegate::SettingsUiState::new());
    cx.set_global(ui::delegate::TabAnimationState::default());
}

fn install_feature_state(cx: &mut App) {
    ui::tabs::file_tree::FileTreeUi::install(cx);
    ui::tabs::terminal::TerminalUi::install(cx);
    file_editor::BufferStore::install(cx);
    file_editor::EditorViewStore::install(cx);
    terminal::TerminalStore::install(cx);
    syntax::SyntaxRegistry::install(cx);
    syntax::SyntaxStore::install(cx);
}

fn install_keybindings(cx: &mut App) {
    ui::components::install_default_keybindings(cx);
    shortcuts::install_defaults(cx);
}

fn open_main_window(cx: &mut App) {
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
            app_id: Some(runtime_app_id()),
            ..Default::default()
        },
        |window, cx| {
            window.set_window_title(APP_NAME);
            let entity = cx.new(KosmosApp::new);
            entity.update(cx, |app, cx| app.start_observing_window(window, cx));
            entity
        },
    )
    .unwrap();
}

fn runtime_app_id() -> String {
    std::env::var("KOSMOS_APP_ID")
        .ok()
        .filter(|app_id| !app_id.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_APP_ID.to_string())
}
