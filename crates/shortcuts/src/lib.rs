use gpui::{App, KeyBinding};

/// Keymap context that activates the global Kosmos shortcuts.
pub const CONTEXT: &str = "Kosmos";

/// A keystroke -> action mapping prior to resolution against the action registry.
/// Settings will eventually deserialize into this same shape so the install path is shared.
///
/// Action types and their handlers live at the UI boundary that owns them;
/// this crate only stores the keymap that pairs keystrokes to action names and
/// drives gpui's binding registration.
#[derive(Clone, Debug)]
pub struct ShortcutBinding {
    pub keystrokes: &'static str,
    pub action: &'static str,
    pub context: &'static str,
}

impl ShortcutBinding {
    pub const fn new(keystrokes: &'static str, action: &'static str) -> Self {
        Self::in_context(keystrokes, action, CONTEXT)
    }

    pub const fn in_context(
        keystrokes: &'static str,
        action: &'static str,
        context: &'static str,
    ) -> Self {
        Self {
            keystrokes,
            action,
            context,
        }
    }
}

pub const DEFAULTS: &[ShortcutBinding] = &[
    ShortcutBinding::new("ctrl-w", "pane_tree::CloseTab"),
    ShortcutBinding::new("ctrl-t", "pane_tree::NewTab"),
    ShortcutBinding::new("ctrl-s", "file_editor::Save"),
    ShortcutBinding::new("ctrl-=", "zoom::ZoomIn"),
    ShortcutBinding::new("ctrl-+", "zoom::ZoomIn"),
    ShortcutBinding::new("ctrl--", "zoom::ZoomOut"),
    ShortcutBinding::new("ctrl-0", "zoom::ResetZoom"),
    ShortcutBinding::in_context("ctrl-a", "text_input::SelectAll", "TextInput"),
    ShortcutBinding::in_context("ctrl-c", "text_input::Copy", "TextInput"),
    ShortcutBinding::in_context("ctrl-v", "text_input::Paste", "TextInput"),
    ShortcutBinding::in_context("ctrl-x", "text_input::Cut", "TextInput"),
    ShortcutBinding::in_context("ctrl-z", "text_input::Undo", "TextInput"),
    ShortcutBinding::in_context("ctrl-y", "text_input::Redo", "TextInput"),
    ShortcutBinding::in_context("ctrl-shift-z", "text_input::Redo", "TextInput"),
    ShortcutBinding::in_context("alt-shift-up", "text_input::DuplicateLineUp", "TextInput"),
    ShortcutBinding::in_context(
        "alt-shift-down",
        "text_input::DuplicateLineDown",
        "TextInput",
    ),
];

/// Install a list of shortcut bindings into the app keymap. Bindings whose action
/// is not registered or whose keystrokes do not parse are silently skipped so a
/// single bad entry from settings cannot break the whole keymap.
pub fn install(cx: &mut App, bindings: &[ShortcutBinding]) {
    let mapper = cx.keyboard_mapper().clone();
    let mut key_bindings = Vec::with_capacity(bindings.len());
    for binding in bindings {
        let Ok(action) = cx.build_action(binding.action, None) else {
            continue;
        };
        let Ok(key_binding) = KeyBinding::load(
            binding.keystrokes,
            action,
            Some(parse_context(binding.context)),
            false,
            None,
            mapper.as_ref(),
        ) else {
            continue;
        };
        key_bindings.push(key_binding);
    }
    cx.bind_keys(key_bindings);
}

/// Convenience for the common case of registering only the built-in defaults.
pub fn install_defaults(cx: &mut App) {
    install(cx, DEFAULTS);
}

fn parse_context(source: &str) -> std::rc::Rc<gpui::KeyBindingContextPredicate> {
    gpui::KeyBindingContextPredicate::parse(source)
        .expect("hard-coded keymap context must parse")
        .into()
}
