//! Shared key identity contract for the Studio picker, validation and both sinks.
use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyboardKey {
    pub name: String,
    pub label: String,
    pub group: String,
    pub code: String,
    pub vk: u16,
    pub flex: String,
    pub hid: Option<u8>,
    pub modifier: u8,
    pub extended: bool,
}

impl KeyboardKey {
    pub fn hid_usage(&self) -> Option<u8> {
        self.hid
            .or_else(|| flexinput_virtual::keyboard::key_name_to_hid_usage(&self.flex))
    }
}

pub fn keys() -> &'static [KeyboardKey] {
    static KEYS: OnceLock<Vec<KeyboardKey>> = OnceLock::new();
    KEYS.get_or_init(|| {
        serde_json::from_str(include_str!("../../../src/keyboard-keys.json"))
            .expect("compiled keyboard catalogue")
    })
}

pub fn key(name: &str) -> Option<&'static KeyboardKey> {
    keys().iter().find(|key| key.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn every_picker_key_is_valid_and_has_one_distinct_keyboard_encoding() {
        let mut names = BTreeSet::new();
        let mut codes = BTreeSet::new();
        let mut usages = BTreeSet::new();
        let mut modifiers = 0u8;
        for key in keys() {
            assert!(names.insert(&key.name), "duplicate key {}", key.name);
            assert!(
                codes.insert(&key.code),
                "duplicate physical code {}",
                key.code
            );
            assert!(!key.label.is_empty() && !key.group.is_empty());
            assert!(crate::profile::valid_key(&key.name));
            assert!((1..=0xff).contains(&key.vk));
            if key.modifier != 0 {
                assert!(key.modifier.is_power_of_two());
                assert_eq!(modifiers & key.modifier, 0);
                modifiers |= key.modifier;
            } else {
                let usage = key.hid_usage().expect(&key.name);
                assert!(usage > 3);
                assert!(usages.insert(usage), "duplicate HID usage for {}", key.name);
            }
        }
        assert_eq!(modifiers, 0xff);
        assert!(key("F12").is_none());
        assert!(key("F0").is_none());
        assert!(key("F25").is_none());
    }
}
