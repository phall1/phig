use std::{collections::BTreeMap, path::Path};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::Action;

use super::{Config, KeyBindings, keys::parse_key, validate};

#[test]
fn zero_config_enables_osc52_copy_and_off_remains_valid() {
    let default = Config::default();
    assert_eq!(default.ui.clipboard, "osc52");
    assert_eq!(default.ui.glyphs, "auto");
    assert_eq!(default.theme.selection_fg, "cyan");
    assert_eq!(default.theme.selection_bg, "reset");
    let mut disabled = default;
    disabled.ui.clipboard = "off".into();
    validate(&disabled, Path::new("disabled.toml")).unwrap();
}

#[test]
fn rejects_invalid_glyph_policy() {
    let mut config = Config::default();
    config.ui.glyphs = "emoji".into();
    assert!(
        validate(&config, Path::new("glyphs.toml"))
            .unwrap_err()
            .to_string()
            .contains("ui.glyphs must be auto, unicode, or ascii")
    );
}

#[test]
fn recursively_rejects_unknown_keys_and_conflicts() {
    let error = toml::from_str::<Config>("version=1\n[ui]\nwat=true")
        .unwrap_err()
        .to_string();
    assert!(error.contains("unknown field"));
    assert!(toml::from_str::<Config>("[ui]\npreview=true").is_err());
    let mut config = Config::default();
    config.keys.insert("open".into(), "x".into());
    config.keys.insert("quit".into(), "x".into());
    assert!(
        validate(&config, Path::new("x"))
            .unwrap_err()
            .to_string()
            .contains("conflicts")
    );
}

#[test]
fn selection_labels_follow_effective_semantic_bindings() {
    assert_eq!(
        KeyBindings::default().selection_key_labels(),
        ("Enter".into(), "Esc/q".into())
    );

    let mut keys = BTreeMap::new();
    keys.insert("quit".into(), "x".into());
    assert_eq!(
        KeyBindings::from_config(&keys)
            .unwrap()
            .selection_key_labels(),
        ("Enter".into(), "Esc/x".into())
    );

    keys.clear();
    keys.insert("open".into(), "q".into());
    assert_eq!(
        KeyBindings::from_config(&keys)
            .unwrap()
            .selection_key_labels(),
        ("q".into(), "Esc".into()),
        "an override that claims q must remove q from the cancel hint"
    );
}

#[test]
fn labels_drop_defaults_claimed_by_other_actions() {
    let mut keys = BTreeMap::new();
    keys.insert("open".into(), "r".into());
    let bindings = KeyBindings::from_config(&keys).unwrap();
    assert_eq!(bindings.action_key_label(&Action::Open), "r");
    assert_eq!(bindings.action_key_label(&Action::ViewRefs), "unbound");

    keys.clear();
    keys.insert("open".into(), "esc".into());
    let bindings = KeyBindings::from_config(&keys).unwrap();
    assert_eq!(bindings.action_key_label(&Action::Open), "Esc");
    assert_eq!(bindings.action_key_label(&Action::Back), "unbound");
    assert_eq!(
        bindings.selection_key_labels(),
        ("Esc".into(), "q".into()),
        "selection labels advertised a displaced cancel binding"
    );
}

#[test]
fn uppercase_bindings_normalize_and_ctrl_c_is_reserved() {
    let mut keys = BTreeMap::new();
    keys.insert("last".into(), "G".into());
    let bindings = KeyBindings::from_config(&keys).unwrap();
    assert_eq!(
        bindings.resolve(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT), None,),
        Some(Action::Last)
    );

    keys.insert("last".into(), "shift+g".into());
    let bindings = KeyBindings::from_config(&keys).unwrap();
    assert_eq!(
        bindings.resolve(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT), None,),
        Some(Action::Last)
    );

    keys.clear();
    keys.insert("quit".into(), "ctrl+c".into());
    assert!(
        KeyBindings::from_config(&keys)
            .unwrap_err()
            .contains("reserved for interrupt")
    );
}

#[test]
fn semantic_override_resolves() {
    let mut config = Config::default();
    config.keys.insert("open".into(), "ctrl+x".into());
    config.keys.insert("help".into(), "h".into());
    let bindings = KeyBindings::from_config(&config.keys).unwrap();
    assert_eq!(
        bindings.resolve(
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
            None,
        ),
        Some(Action::Open)
    );
    assert_eq!(
        bindings.resolve(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            Some(Action::Open)
        ),
        None,
        "an override must disable every old default for that semantic action"
    );
    assert_eq!(
        bindings.resolve(
            KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE),
            Some(Action::ToggleHelp),
        ),
        None,
        "remapping help must disable its old question-mark key"
    );
    for invalid in ["wat+x", "ctrl+wat+x", "ctrl+ctrl+x", "ctrl+", "+x"] {
        assert!(parse_key(invalid).is_none(), "accepted {invalid}");
    }
}
