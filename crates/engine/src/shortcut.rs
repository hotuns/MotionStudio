//! Persisted global profile shortcut and its press-edge state, independent of
//! the Windows key polling adapter.
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const MAX_SHORTCUT_RULES: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShortcutRule {
    pub trigger: ShortcutTrigger,
    pub target: String,
    #[serde(default = "default_return_on_repeat")]
    pub return_on_repeat: bool,
}

fn default_return_on_repeat() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ShortcutTrigger {
    Keyboard { key: Shortcut },
    Gamepad { input: String },
}

pub fn validate_shortcut_rules(rules: &[ShortcutRule]) -> Result<()> {
    if rules.len() > MAX_SHORTCUT_RULES {
        bail!("最多支持 {MAX_SHORTCUT_RULES} 条快捷键规则");
    }
    let mut keyboard = HashSet::new();
    let mut gamepad = HashSet::new();
    for rule in rules {
        crate::storage::validate_profile_id(&rule.target)?;
        match &rule.trigger {
            ShortcutTrigger::Keyboard { key } => {
                key.validate()?;
                if !keyboard.insert((key.virtual_key(), key.required_modifiers())) {
                    bail!("同一个键盘快捷键只能用于一条切换规则");
                }
            }
            ShortcutTrigger::Gamepad { input } => {
                let mut parts = pad_shortcut_parts(input)?;
                parts.sort_unstable();
                if !gamepad.insert(parts) {
                    bail!("同一个手柄快捷键只能用于一条切换规则");
                }
            }
        }
    }
    Ok(())
}

/// Identity includes the destination so changing targets cannot replay an old press.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PadShortcut {
    pub input: String,
    pub target: String,
}
pub fn validate_pad_input(input: &str) -> Result<()> {
    pad_shortcut_parts(input).map(drop)
}

fn pad_shortcut_parts(input: &str) -> Result<Vec<&str>> {
    let parts = crate::bindings::input_chord(input)
        .ok_or_else(|| anyhow::anyhow!("手柄快捷键需要 1–4 个有效且不冲突的输入"))?;
    if parts.iter().any(|p| crate::profile::AXES[..4].contains(p)) {
        bail!("请使用摇杆方向或内外圈作为快捷键");
    }
    if parts.contains(&"back") && parts.contains(&"start") {
        bail!("View + Menu 已用于进入手柄界面操作，请选择其他组合");
    }
    Ok(parts)
}

/// Runs on every raw controls event, independently of profile engine replacement.
#[derive(Default)]
pub struct PadShortcutEdges {
    binding: Option<PadShortcut>,
    parts: Vec<String>,
    down: bool,
    enabled: bool,
}
impl PadShortcutEdges {
    pub fn update(
        &mut self,
        binding: Option<&PadShortcut>,
        buttons: &[bool],
        axes: &[f32; 6],
        extra: &[Option<&str>],
        enabled: bool,
        snapshot: bool,
    ) -> bool {
        let held = crate::profile::pressed_inputs(buttons, axes, if self.down { 0.5 } else { 0.6 });
        self.update_with_held(binding, &held, &held, extra, enabled, snapshot)
    }

    /// Reuse precomputed controls when checking several shortcuts for one event.
    pub fn update_with_held(
        &mut self,
        binding: Option<&PadShortcut>,
        pressed: &[String],
        released: &[String],
        extra: &[Option<&str>],
        enabled: bool,
        snapshot: bool,
    ) -> bool {
        let changed = self.binding.as_ref() != binding;
        if changed {
            self.binding = binding.cloned();
            self.parts = binding
                .and_then(|b| crate::bindings::input_chord(&b.input))
                .unwrap_or_default()
                .into_iter()
                .map(String::from)
                .collect();
        }
        let held = if self.down { released } else { pressed };
        let down = !self.parts.is_empty()
            && self.parts.iter().all(|part| {
                held.contains(part) || extra.iter().flatten().any(|input| *input == part.as_str())
            });
        let pressed = !changed && !snapshot && enabled && self.enabled && down && !self.down;
        self.down = down;
        self.enabled = enabled;
        pressed
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Shortcut {
    pub key: String,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}
impl Default for Shortcut {
    fn default() -> Self {
        Self {
            key: "F9".into(),
            ctrl: false,
            alt: false,
            shift: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub windows: bool,
}

impl Shortcut {
    /// The shared keyboard catalogue also drives the action editor.
    pub fn virtual_key(&self) -> Option<u16> {
        match self.key.as_str() {
            "WIN" | "RWIN" | "NUMPADENTER" => None,
            name => crate::keyboard::key(name).map(|key| key.vk),
        }
    }
    pub fn validate(&self) -> Result<()> {
        if self.virtual_key().is_none() {
            bail!("此按键不可用作快捷键。F12 用于紧急停止，Windows 键由系统处理。");
        }
        if self.ctrl && !self.alt && matches!(self.key.as_str(), "S" | "K" | "ENTER") {
            bail!("Ctrl+S、Ctrl+K 和 Ctrl+Enter（含 Shift）已用于保存、搜索和应用配置，请选择其他组合");
        }
        if self.alt
            && matches!(
                (self.ctrl, self.key.as_str()),
                (false, "F4" | "TAB") | (true, "DELETE")
            )
        {
            bail!("此组合已由 Windows 使用，请选择其他快捷键");
        }
        Ok(())
    }
    pub fn matches_modifiers(&self, m: Modifiers) -> bool {
        self.required_modifiers() == (m.ctrl, m.alt, m.shift) && !m.windows
    }
    fn required_modifiers(&self) -> (bool, bool, bool) {
        let primary = crate::keyboard::key(&self.key).map_or(0, |key| key.modifier);
        (
            self.ctrl || primary & 0x11 != 0,
            self.alt || primary & 0x44 != 0,
            self.shift || primary & 0x22 != 0,
        )
    }
}

/// Changing a binding, starting output or finishing capture establishes a new
/// baseline. Only a new main-key press can switch; modifier changes never do.
#[derive(Default)]
pub struct ShortcutEdges {
    binding: Option<(Shortcut, Option<String>)>,
    last_down: bool,
    last_enabled: bool,
}
impl ShortcutEdges {
    pub fn update(
        &mut self,
        shortcut: &Shortcut,
        target: Option<&str>,
        down: bool,
        modifiers: Modifiers,
        enabled: bool,
    ) -> bool {
        let changed = self
            .binding
            .as_ref()
            .is_none_or(|(key, to)| key != shortcut || to.as_deref() != target);
        let pressed = !changed
            && self.last_enabled
            && enabled
            && !self.last_down
            && down
            && shortcut.matches_modifiers(modifiers);
        if changed {
            self.binding = Some((shortcut.clone(), target.map(String::from)));
        }
        self.last_down = down;
        self.last_enabled = enabled;
        pressed && target.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_modifiers_keys_and_reserved_application_shortcuts() {
        assert_eq!(Shortcut::default().virtual_key(), Some(0x78));
        assert_eq!(
            Shortcut {
                key: "F10".into(),
                ..Default::default()
            }
            .virtual_key(),
            Some(0x79)
        );
        for key in [
            "F1",
            "F9",
            "F10",
            "F24",
            "A",
            "9",
            "SPACE",
            "PAGEUP",
            "DELETE",
            "ESC",
            "CTRL",
            "CAPSLOCK",
            "NUMPAD1",
            "SEMICOLON",
        ] {
            let s = Shortcut {
                key: key.into(),
                ..Default::default()
            };
            assert!(s.validate().is_ok(), "{key}");
        }
        for key in [
            "F12",
            "F0",
            "F25",
            "F09",
            "WIN",
            "RWIN",
            "NUMPADENTER",
            "a",
            "",
        ] {
            let s = Shortcut {
                key: key.into(),
                ..Default::default()
            };
            assert!(s.validate().is_err(), "{key}");
        }
        for shift in [false, true] {
            let s = Shortcut {
                key: "S".into(),
                ctrl: true,
                shift,
                ..Default::default()
            };
            assert!(s.validate().is_err());
            assert!(Shortcut { alt: true, ..s }.validate().is_ok());
        }
        for (key, ctrl) in [("F4", false), ("TAB", false), ("DELETE", true)] {
            assert!(Shortcut {
                key: key.into(),
                ctrl,
                alt: true,
                shift: false
            }
            .validate()
            .is_err());
        }
    }
    #[test]
    fn primary_modifier_can_trigger_alone_and_still_requires_exact_other_modifiers() {
        let key = Shortcut {
            key: "RSHIFT".into(),
            ..Default::default()
        };
        let modifiers = Modifiers {
            shift: true,
            ..Default::default()
        };
        let mut edges = ShortcutEdges::default();
        assert!(!edges.update(&key, Some("aim"), false, Modifiers::default(), true));
        assert!(edges.update(&key, Some("aim"), true, modifiers, true));
        assert!(!edges.update(&key, Some("aim"), true, modifiers, true));
        assert!(!key.matches_modifiers(Modifiers {
            ctrl: true,
            ..modifiers
        }));
    }
    #[test]
    fn uses_one_real_press_and_exact_modifiers() {
        let mut e = ShortcutEdges::default();
        let key = Shortcut {
            key: "F10".into(),
            ctrl: true,
            shift: true,
            alt: false,
        };
        let m = Modifiers {
            ctrl: true,
            shift: true,
            ..Default::default()
        };
        assert!(!e.update(&key, Some("menu"), false, m, true));
        assert!(e.update(&key, Some("menu"), true, m, true));
        assert!(!e.update(&key, Some("menu"), true, m, true));
        e.update(&key, Some("menu"), false, m, true);
        assert!(!e.update(&key, Some("menu"), true, Modifiers { alt: true, ..m }, true));
        assert!(
            !e.update(&key, Some("menu"), true, m, true),
            "releasing an extra modifier is not a new main-key press"
        );
        e.update(&key, Some("menu"), false, m, true);
        assert!(e.update(&key, Some("menu"), true, m, true));
    }
    #[test]
    fn capture_reload_new_targets_and_output_start_do_not_replay_a_held_key() {
        let mut e = ShortcutEdges::default();
        let key = Shortcut::default();
        let m = Modifiers::default();
        assert!(!e.update(&key, Some("menu"), true, m, true));
        e.update(&key, Some("menu"), false, m, true);
        assert!(!e.update(&key, Some("menu"), true, m, false));
        assert!(!e.update(&key, Some("menu"), true, m, true));
        e.update(&key, Some("menu"), false, m, true);
        assert!(e.update(&key, Some("menu"), true, m, true));
        assert!(!e.update(&key, Some("other"), true, m, true));
        let changed = Shortcut {
            key: "F10".into(),
            ..Default::default()
        };
        assert!(!e.update(&changed, Some("other"), true, m, true));
        e.update(&changed, Some("other"), false, m, true);
        assert!(e.update(&changed, Some("other"), true, m, true));
        e.update(&changed, None, false, m, true);
        assert!(!e.update(&changed, None, true, m, true));
    }
}

#[cfg(test)]
mod pad_tests {
    use super::*;
    fn binding(input: &str) -> PadShortcut {
        PadShortcut {
            input: input.into(),
            target: "aim".into(),
        }
    }
    #[test]
    fn pad_validation_reuses_input_chords_and_reserves_navigation() {
        for input in [
            "south",
            "left_shoulder+right_trigger",
            "left_stick_up+right_paddle1",
            "left_stick_outer",
            "left_stick_inner+touch2",
        ] {
            assert!(validate_pad_input(input).is_ok(), "{input}");
        }
        for input in [
            "",
            "back+start",
            "back+start+south",
            "left_stick_inner+left_stick_outer",
            "left_stick_x",
            "dpad_up+dpad_down",
            "south+south",
        ] {
            assert!(validate_pad_input(input).is_err(), "{input}");
        }
    }
    #[test]
    fn pad_edges_require_a_new_press_after_capture_snapshot_or_target_change() {
        let mut edges = PadShortcutEdges::default();
        let mut buttons = [false; 26];
        let axes = [0.; 6];
        let mut b = binding("south");
        assert!(!edges.update(Some(&b), &buttons, &axes, &[], true, true));
        buttons[0] = true;
        assert!(edges.update(Some(&b), &buttons, &axes, &[], true, false));
        assert!(!edges.update(Some(&b), &buttons, &axes, &[], true, false));
        assert!(!edges.update(Some(&b), &buttons, &axes, &[], false, true));
        assert!(!edges.update(Some(&b), &buttons, &axes, &[], true, true));
        b.target = "other".into();
        assert!(!edges.update(Some(&b), &buttons, &axes, &[], true, false));
        buttons[0] = false;
        assert!(!edges.update(Some(&b), &buttons, &axes, &[], true, false));
        buttons[0] = true;
        assert!(edges.update(Some(&b), &buttons, &axes, &[], true, false));
    }
    #[test]
    fn analog_chords_have_hysteresis_and_one_edge_while_held() {
        let mut edges = PadShortcutEdges::default();
        let mut buttons = [false; 26];
        let mut axes = [0.; 6];
        let b = binding("left_shoulder+right_trigger");
        edges.update(Some(&b), &buttons, &axes, &[], true, true);
        buttons[9] = true;
        assert!(!edges.update(Some(&b), &buttons, &axes, &[], true, false));
        axes[5] = 0.61;
        assert!(edges.update(Some(&b), &buttons, &axes, &[], true, false));
        for value in [0.59, 0.51, 0.63] {
            axes[5] = value;
            assert!(!edges.update(Some(&b), &buttons, &axes, &[], true, false));
        }
        axes[5] = 0.49;
        assert!(!edges.update(Some(&b), &buttons, &axes, &[], true, false));
        axes[5] = 0.61;
        assert!(edges.update(Some(&b), &buttons, &axes, &[], true, false));
    }
    #[test]
    fn pad_setting_round_trips_and_legacy_settings_disable_it() {
        let settings = crate::storage::Settings {
            shortcut_pad: Some("left_shoulder+right_shoulder".into()),
            ..Default::default()
        };
        settings.validate().unwrap();
        let encoded = toml::to_string(&settings).unwrap();
        let loaded: crate::storage::Settings = toml::from_str(&encoded).unwrap();
        assert_eq!(loaded.shortcut_pad, settings.shortcut_pad);
        assert!(serde_json::from_str::<crate::storage::Settings>("{}")
            .unwrap()
            .shortcut_pad
            .is_none());
    }
}
