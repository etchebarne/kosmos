mod app;
mod delegates;

use gpui::{App, AppContext, Bounds, WindowBounds, WindowDecorations, WindowOptions, px, size};
use icons::AppAssets;
use settings::SettingValue;

use crate::app::KosmosApp;

const APP_NAME: &str = "Kosmos";
const DEFAULT_APP_ID: &str = "net.etchebarne.Kosmos";

pub fn run() {
    gpui_platform::application()
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
    gpui_component::init(cx);
    cx.set_global(settings::Settings::load());
    install_theme(cx);
    cx.set_global(ui::delegate::SettingsUiState::new());
    cx.set_global(ui::delegate::TabAnimationState::default());
}

fn install_theme(cx: &mut App) {
    theme::install(selected_theme_id(cx), cx);
    cx.observe_global::<settings::Settings>(|cx| {
        theme::apply(selected_theme_id(cx), cx);
    })
    .detach();
}

fn selected_theme_id(cx: &App) -> &'static str {
    cx.global::<settings::Settings>()
        .get(theme::SETTING_ID)
        .and_then(SettingValue::as_str)
        .map(theme::Theme::normalize_id)
        .unwrap_or(theme::DEFAULT_ID)
}

fn install_feature_state(cx: &mut App) {
    ui::tabs::terminal::TerminalUi::install(cx);
    file_editor::BufferStore::install(cx);
    terminal::TerminalStore::install(cx);
}

fn install_keybindings(cx: &mut App) {
    ui::tabs::install_keybindings(cx);
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
            let entity = cx.new(|cx| KosmosApp::new(window, cx));
            entity.update(cx, |app, cx| app.start_observing_window(window, cx));
            cx.new(|cx| gpui_component::Root::new(entity, window, cx))
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
