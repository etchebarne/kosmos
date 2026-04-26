use gpui::{App, BorrowAppContext, Context, InteractiveElement, Pixels, Window, actions, px};
use settings::{ActiveSettings, SettingValue, Settings};

pub const SETTING_ID: &str = "appearance.zoom";

const DEFAULT_PERCENT: i64 = 100;
const MIN_PERCENT: i64 = 75;
const MAX_PERCENT: i64 = 125;
const STEP_PERCENT: i64 = 5;
const BASE_REM_PX: f32 = 16.0;

actions!(zoom, [ZoomIn, ZoomOut, ResetZoom]);

/// Read the current zoom percentage from settings, clamped to the supported range.
pub fn current_percent(cx: &App) -> i64 {
    cx.settings()
        .get(SETTING_ID)
        .and_then(SettingValue::as_int)
        .unwrap_or(DEFAULT_PERCENT)
        .clamp(MIN_PERCENT, MAX_PERCENT)
}

/// Apply the current zoom setting to the window's rem size so the UI rescales.
pub fn apply(window: &mut Window, cx: &App) {
    window.set_rem_size(rem_for(current_percent(cx)));
}

fn rem_for(percent: i64) -> Pixels {
    px(BASE_REM_PX * percent as f32 / 100.0)
}

fn set_percent(cx: &mut App, percent: i64) {
    let clamped = percent.clamp(MIN_PERCENT, MAX_PERCENT);
    if clamped == current_percent(cx) {
        return;
    }
    cx.update_global::<Settings, _>(|settings, _| {
        settings.set(SETTING_ID, SettingValue::Int(clamped));
    });
}

/// Extension trait: chain `.wire_zoom_actions(cx)` onto a focusable element to
/// register the zoom action handlers in one line.
pub trait WireZoomActions: Sized {
    fn wire_zoom_actions<T: 'static>(self, cx: &mut Context<T>) -> Self;
}

impl<E: InteractiveElement + 'static> WireZoomActions for E {
    fn wire_zoom_actions<T: 'static>(self, cx: &mut Context<T>) -> Self {
        self.on_action(cx.listener(|_, _: &ZoomIn, _, cx| {
            let next = current_percent(cx) + STEP_PERCENT;
            set_percent(cx, next);
            cx.notify();
        }))
        .on_action(cx.listener(|_, _: &ZoomOut, _, cx| {
            let next = current_percent(cx) - STEP_PERCENT;
            set_percent(cx, next);
            cx.notify();
        }))
        .on_action(cx.listener(|_, _: &ResetZoom, _, cx| {
            set_percent(cx, DEFAULT_PERCENT);
            cx.notify();
        }))
    }
}
