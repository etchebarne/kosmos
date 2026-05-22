use std::{
    ops::Range,
    sync::OnceLock,
    time::Instant,
};

use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, Element, ElementId, ElementInputHandler,
    Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable, GlobalElementId, KeyBinding,
    LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point,
    Render, ScrollHandle, ShapedLine, SharedString, Style, TextAlign, TextRun, UTF16Selection,
    UnderlineStyle, Window, WrappedLine, actions, div, fill, hsla, point, prelude::*, relative,
    rems, size,
};
use unicode_segmentation::UnicodeSegmentation;

use gpui_component::scroll::{Scrollbar, ScrollbarShow};
use theme::ActiveTheme;

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

#[derive(Clone, Debug)]
pub struct ValueChanged {
    pub value: SharedString,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CharacterClass {
    Whitespace,
    Word,
    Punctuation,
}

fn character_class(ch: char) -> CharacterClass {
    if ch.is_whitespace() {
        CharacterClass::Whitespace
    } else if ch.is_alphanumeric() || ch == '_' {
        CharacterClass::Word
    } else {
        CharacterClass::Punctuation
    }
}

fn char_at(content: &str, offset: usize) -> Option<char> {
    content.get(offset..)?.chars().next()
}

fn previous_char_boundary(content: &str, offset: usize) -> usize {
    content[..offset]
        .grapheme_indices(true)
        .next_back()
        .map_or(0, |(index, _)| index)
}

fn next_char_boundary(content: &str, offset: usize) -> usize {
    content[offset..]
        .grapheme_indices(true)
        .nth(1)
        .map_or(content.len(), |(index, _)| offset + index)
}

fn previous_word_boundary(content: &str, offset: usize) -> usize {
    let mut offset = offset.min(content.len());
    while offset > 0 {
        let previous = previous_char_boundary(content, offset);
        if char_at(content, previous).is_none_or(|ch| !ch.is_whitespace()) {
            break;
        }
        offset = previous;
    }

    let Some(class) =
        char_at(content, previous_char_boundary(content, offset)).map(character_class)
    else {
        return 0;
    };
    while offset > 0 {
        let previous = previous_char_boundary(content, offset);
        if char_at(content, previous).map(character_class) != Some(class) {
            break;
        }
        offset = previous;
    }
    offset
}

fn next_word_boundary(content: &str, offset: usize) -> usize {
    let mut offset = offset.min(content.len());
    while offset < content.len() {
        if char_at(content, offset).is_none_or(|ch| !ch.is_whitespace()) {
            break;
        }
        offset = next_char_boundary(content, offset);
    }

    let Some(class) = char_at(content, offset).map(character_class) else {
        return content.len();
    };
    while offset < content.len() {
        if char_at(content, offset).map(character_class) != Some(class) {
            break;
        }
        offset = next_char_boundary(content, offset);
    }
    offset
}
