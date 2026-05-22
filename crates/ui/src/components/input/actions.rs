use std::{
    sync::OnceLock,
    time::Instant,
};

use gpui::{App, Bounds, KeyBinding, Pixels, Point, Window, actions, point, rems, size};

pub const KEY_CONTEXT: &str = "TextInput";
const TEXT_CURSOR_WIDTH_REM: f32 = 0.0625;
const TEXT_CURSOR_HEIGHT_FRACTION: f32 = 0.78;
const TEXT_CURSOR_BLINK_PERIOD_MS: u128 = 1_000;
const TEXT_CURSOR_BLINK_VISIBLE_MS: u128 = 530;

pub fn text_cursor_bounds(
    origin: Point<Pixels>,
    line_height: Pixels,
    window: &Window,
) -> Bounds<Pixels> {
    let height = (line_height * TEXT_CURSOR_HEIGHT_FRACTION).round();
    let top = origin.y + ((line_height - height) / 2.0).round();
    Bounds::new(
        point(origin.x.round(), top),
        size(
            rems(TEXT_CURSOR_WIDTH_REM).to_pixels(window.rem_size()),
            height,
        ),
    )
}

pub fn should_paint_text_cursor(window: &mut Window) -> bool {
    window.request_animation_frame();

    static BLINK_START: OnceLock<Instant> = OnceLock::new();
    let elapsed = BLINK_START.get_or_init(Instant::now).elapsed().as_millis();
    elapsed % TEXT_CURSOR_BLINK_PERIOD_MS < TEXT_CURSOR_BLINK_VISIBLE_MS
}

actions!(
    text_input,
    [
        Backspace,
        Delete,
        Left,
        Right,
        Up,
        Down,
        WordLeft,
        WordRight,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        SelectWordLeft,
        SelectWordRight,
        SelectAll,
        Enter,
        Home,
        End,
        Paste,
        Cut,
        Copy,
        Undo,
        Redo,
        DuplicateLineUp,
        DuplicateLineDown
    ]
);

pub fn install_default_keybindings(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, Some(KEY_CONTEXT)),
        KeyBinding::new("delete", Delete, Some(KEY_CONTEXT)),
        KeyBinding::new("left", Left, Some(KEY_CONTEXT)),
        KeyBinding::new("right", Right, Some(KEY_CONTEXT)),
        KeyBinding::new("up", Up, Some(KEY_CONTEXT)),
        KeyBinding::new("down", Down, Some(KEY_CONTEXT)),
        KeyBinding::new("alt-left", WordLeft, Some(KEY_CONTEXT)),
        KeyBinding::new("alt-right", WordRight, Some(KEY_CONTEXT)),
        KeyBinding::new("shift-left", SelectLeft, Some(KEY_CONTEXT)),
        KeyBinding::new("shift-right", SelectRight, Some(KEY_CONTEXT)),
        KeyBinding::new("shift-up", SelectUp, Some(KEY_CONTEXT)),
        KeyBinding::new("shift-down", SelectDown, Some(KEY_CONTEXT)),
        KeyBinding::new("alt-shift-left", SelectWordLeft, Some(KEY_CONTEXT)),
        KeyBinding::new("alt-shift-right", SelectWordRight, Some(KEY_CONTEXT)),
        KeyBinding::new("enter", Enter, Some(KEY_CONTEXT)),
        KeyBinding::new("home", Home, Some(KEY_CONTEXT)),
        KeyBinding::new("end", End, Some(KEY_CONTEXT)),
    ]);
}
