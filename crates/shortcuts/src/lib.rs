use gpui::{App, Global, KeyBinding};

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

#[derive(Clone, Debug)]
pub struct ShortcutRegistry {
    bindings: Vec<ShortcutBinding>,
}

impl ShortcutRegistry {
    pub fn new(bindings: Vec<ShortcutBinding>) -> Self {
        Self { bindings }
    }

    pub fn primary_label_for_action(&self, action: &str) -> Option<String> {
        primary_label_for_action_in(&self.bindings, action)
    }
}

impl Global for ShortcutRegistry {}

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
    let mut installed_bindings = Vec::with_capacity(bindings.len());
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
        installed_bindings.push(binding.clone());
        key_bindings.push(key_binding);
    }
    cx.bind_keys(key_bindings);
    cx.set_global(ShortcutRegistry::new(installed_bindings));
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

pub fn primary_label_for_action(action: &str, cx: &App) -> Option<String> {
    if let Some(registry) = cx.try_global::<ShortcutRegistry>() {
        return registry.primary_label_for_action(action);
    }
    primary_label_for_action_in(DEFAULTS, action)
}

pub fn primary_label_for_action_in(bindings: &[ShortcutBinding], action: &str) -> Option<String> {
    bindings
        .iter()
        .find(|binding| binding.action == action)
        .map(|binding| format_keystrokes(binding.keystrokes))
}

pub fn format_keystrokes(keystrokes: &str) -> String {
    keystrokes
        .split_whitespace()
        .map(format_chord)
        .collect::<Vec<_>>()
        .join(" ")
}

fn format_chord(chord: &str) -> String {
    let mut remaining = chord;
    let mut parts = Vec::new();
    loop {
        match parse_modifier(remaining) {
            Some((label, rest)) => {
                parts.push(label.to_string());
                remaining = rest;
            }
            None => {
                parts.push(format_key(remaining));
                break;
            }
        }
    }
    parts.join("+")
}

fn parse_modifier(source: &str) -> Option<(&'static str, &str)> {
    [
        ("ctrl-", "Ctrl"),
        ("cmd-", "Cmd"),
        ("alt-", "Alt"),
        ("shift-", "Shift"),
    ]
    .into_iter()
    .find_map(|(prefix, label)| source.strip_prefix(prefix).map(|rest| (label, rest)))
}

fn format_key(key: &str) -> String {
    match key {
        "esc" | "escape" => "Esc".to_string(),
        "backspace" => "Backspace".to_string(),
        "delete" => "Delete".to_string(),
        "enter" => "Enter".to_string(),
        "space" => "Space".to_string(),
        "tab" => "Tab".to_string(),
        "left" => "Left".to_string(),
        "right" => "Right".to_string(),
        "up" => "Up".to_string(),
        "down" => "Down".to_string(),
        key if key.len() == 1 => key.to_ascii_uppercase(),
        key => key.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_shortcut_labels_for_display() {
        assert_eq!(format_keystrokes("ctrl-s"), "Ctrl+S");
        assert_eq!(format_keystrokes("ctrl-shift-z"), "Ctrl+Shift+Z");
        assert_eq!(format_keystrokes("alt-shift-up"), "Alt+Shift+Up");
        assert_eq!(format_keystrokes("ctrl--"), "Ctrl+-");
    }

    #[test]
    fn finds_primary_shortcut_for_action() {
        assert_eq!(
            primary_label_for_action_in(DEFAULTS, "file_editor::Save"),
            Some("Ctrl+S".to_string())
        );
        assert_eq!(
            primary_label_for_action_in(DEFAULTS, "text_input::Redo"),
            Some("Ctrl+Y".to_string())
        );
    }
}
