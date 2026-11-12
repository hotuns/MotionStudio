use motion_core::{
    profile::{GamepadTarget, GyroOutput, Profile},
    storage::{parse_profile, Store, SwitchTarget},
    switching::{CursorEdges, SwitchHistory},
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
struct Sandbox(PathBuf);
#[test]
fn output_backend_defaults_migrate_locally_and_do_not_enter_game_profiles() {
    use motion_core::{output::OutputBackend, storage::Settings};
    let mut settings: Settings =
        serde_json::from_str(r#"{"version":1,"selected_profile":"keyboard"}"#).unwrap();
    assert_eq!(settings.output_backend, OutputBackend::HidMaestro);
    let tmp = Sandbox::new();
    let store = Store::open(tmp.0.clone()).unwrap();
    settings.output_backend = OutputBackend::Legacy;
    store.save_settings(&settings).unwrap();
    assert_eq!(
        store.settings().unwrap().output_backend,
        OutputBackend::Legacy
    );
    assert!(serde_json::from_str::<Settings>(r#"{"output_backend":"fake_driver"}"#).is_err());
    let profile = store.load("keyboard").unwrap().profile;
    assert!(!toml::to_string(&profile)
        .unwrap()
        .contains("output_backend"));
}
impl Sandbox {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("motion-test-{}", uuid::Uuid::new_v4())))
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn atomic_saves_keep_valid_backup_and_invalid_edits_are_rejected() {
    let tmp = Sandbox::new();
    let store = Store::open(tmp.0.clone()).unwrap();
    let mut profile = store.load("gyro").unwrap();
    let original = profile.profile.name.clone();
    profile.profile.name = "重命名的游戏".into();
    store.save(&profile).unwrap();
    assert_eq!(store.load("gyro").unwrap().profile.name, "重命名的游戏");
    profile.profile.gyro.sensitivity = f32::NAN;
    assert!(store.save(&profile).is_err());
    assert_eq!(store.load("gyro").unwrap().profile.name, "重命名的游戏");
    assert_eq!(store.restore_backup("gyro").unwrap().profile.name, original);
    assert!(store.profile_path("../outside").is_err());
}

#[test]
fn legacy_settings_keep_f9_and_custom_shortcuts_persist_and_reject_reserved_keys() {
    use motion_core::{shortcut::Shortcut, storage::Settings};
    let mut settings: Settings =
        serde_json::from_str(r#"{"version":1,"shortcut_profile":"gamepad"}"#).unwrap();
    assert_eq!(settings.shortcut_key, Some(Shortcut::default()));
    settings.validate().unwrap();
    let tmp = Sandbox::new();
    let store = Store::open(tmp.0.clone()).unwrap();
    settings.shortcut_key = Some(Shortcut {
        key: "F10".into(),
        ctrl: true,
        alt: false,
        shift: true,
    });
    store.save_settings(&settings).unwrap();
    assert_eq!(
        store.settings().unwrap().shortcut_key,
        settings.shortcut_key
    );
    settings.shortcut_key.as_mut().unwrap().key = "F12".into();
    assert!(store.save_settings(&settings).is_err());
    assert_eq!(store.settings().unwrap().shortcut_key.unwrap().key, "F10");
    settings.shortcut_key = None;
    settings.shortcut_pad = Some("left_stick_outer".into());
    store.save_settings(&settings).unwrap();
    let loaded = store.settings().unwrap();
    assert_eq!(loaded.shortcut_key, None);
    assert_eq!(loaded.shortcut_pad, settings.shortcut_pad);
    assert!(motion_core::profile::valid_output("key:F9"));
    assert!(!motion_core::profile::valid_output("key:F12"));
}

#[test]
fn shortcut_rules_round_trip_and_an_empty_list_replaces_legacy_shortcuts() {
    use motion_core::{
        shortcut::{Shortcut, ShortcutRule, ShortcutTrigger},
        storage::Settings,
    };
    let mut settings: Settings = serde_json::from_str(
        r#"{"version":1,"shortcut_profile":"gyro","shortcut_pad":"left_shoulder+south"}"#,
    )
    .unwrap();
    let mut rules = settings.effective_shortcut_rules();
    assert_eq!(
        rules,
        vec![
            ShortcutRule {
                trigger: ShortcutTrigger::Keyboard {
                    key: Shortcut::default(),
                },
                target: "gyro".into(),
                return_on_repeat: true,
            },
            ShortcutRule {
                trigger: ShortcutTrigger::Gamepad {
                    input: "left_shoulder+south".into(),
                },
                target: "gyro".into(),
                return_on_repeat: true,
            },
        ]
    );
    let legacy_rule: ShortcutRule = serde_json::from_str(
        r#"{"trigger":{"kind":"gamepad","input":"left_shoulder+south"},"target":"gyro"}"#,
    )
    .unwrap();
    assert_eq!(legacy_rule, rules[1]);
    rules[1].target = "gamepad".into();
    rules[1].return_on_repeat = false;
    settings.shortcut_rules = Some(rules.clone());
    let tmp = Sandbox::new();
    let store = Store::open(tmp.0.clone()).unwrap();
    store.save_settings(&settings).unwrap();
    assert_eq!(store.settings().unwrap().effective_shortcut_rules(), rules);

    settings.shortcut_rules = Some(vec![]);
    store.save_settings(&settings).unwrap();
    assert!(store.settings().unwrap().effective_shortcut_rules().is_empty());
}

#[test]
fn shortcut_rules_reject_equivalent_keyboard_and_controller_triggers() {
    use motion_core::shortcut::{validate_shortcut_rules, Shortcut, ShortcutRule, ShortcutTrigger};
    let keyboard = |target: &str, ctrl| ShortcutRule {
        trigger: ShortcutTrigger::Keyboard {
            key: Shortcut {
                key: "CTRL".into(),
                ctrl,
                ..Default::default()
            },
        },
        target: target.into(),
        return_on_repeat: true,
    };
    let pad = |target: &str, input: &str| ShortcutRule {
        trigger: ShortcutTrigger::Gamepad { input: input.into() },
        target: target.into(),
        return_on_repeat: true,
    };
    assert!(validate_shortcut_rules(&[keyboard("gyro", false), keyboard("gamepad", true)]).is_err());
    assert!(validate_shortcut_rules(&[
        pad("gyro", "left_shoulder+south"),
        pad("gamepad", "south + left_shoulder"),
    ])
    .is_err());
    assert!(validate_shortcut_rules(&[keyboard("gyro", false), pad("gamepad", "south")]).is_ok());
}

#[test]
fn profiles_share_as_one_document_and_empty_library_stays_empty() {
    let tmp = Sandbox::new();
    let store = Store::open(tmp.0.clone()).unwrap();
    for entry in store.list().unwrap().0 {
        let encoded = toml::to_string_pretty(&entry.profile).unwrap();
        let decoded = parse_profile(&encoded).unwrap();
        assert_eq!(decoded.name, entry.profile.name);
        assert_eq!(decoded.actions.len(), entry.profile.actions.len());
        store.delete(&entry.id).unwrap();
    }
    assert!(Store::open(tmp.0.clone())
        .unwrap()
        .list()
        .unwrap()
        .0
        .is_empty());
}

#[test]
fn existing_virtual_gamepad_booleans_migrate_on_save_and_in_ipc() {
    let tmp = Sandbox::new();
    let store = Store::open(tmp.0.clone()).unwrap();
    let path = store.profile_path("gamepad").unwrap();
    let legacy = "schema_version = 1\nname = \"Legacy\"\nvirtual_gamepad = true\n";
    std::fs::write(&path, legacy).unwrap();
    let profile = store.load("gamepad").unwrap();
    assert_eq!(profile.profile.virtual_gamepad, GamepadTarget::Xbox360);
    assert_eq!(profile.profile.gyro.output, GyroOutput::Mouse);
    store.save(&profile).unwrap();
    assert!(std::fs::read_to_string(&path)
        .unwrap()
        .contains("virtual_gamepad = \"xbox_360\""));
    assert_eq!(
        std::fs::read_to_string(path.with_extension("toml.bak")).unwrap(),
        legacy
    );
    assert_eq!(
        parse_profile(&legacy.replace("true", "false"))
            .unwrap()
            .virtual_gamepad,
        GamepadTarget::None
    );
    assert!(parse_profile(&legacy.replace("true", "\"dualsense\"")).is_err());
    let json =
        serde_json::json!({ "schema_version": 1, "name": "Legacy IPC", "virtual_gamepad": true });
    let decoded: Profile = serde_json::from_value(json).unwrap();
    assert_eq!(
        serde_json::to_value(decoded).unwrap()["virtual_gamepad"],
        "xbox_360"
    );
}

#[test]
fn advanced_gyro_profile_round_trips_and_validates_cross_field_settings() {
    let text = include_str!("../../../examples/gyro-gamepad.toml");
    let mut profile = parse_profile(&text.replace("xbox_360", "dualshock_4")).unwrap();
    assert_eq!(profile.virtual_gamepad, GamepadTarget::DualShock4);
    assert_eq!(profile.gyro.output, GyroOutput::RightStick);
    profile.gyro.pause_input = Some("east".into());
    profile.gyro.acceleration.fast_multiplier = 2.0;
    let round_trip = parse_profile(&toml::to_string_pretty(&profile).unwrap()).unwrap();
    assert_eq!(round_trip.gyro.pause_input.as_deref(), Some("east"));
    assert_eq!(round_trip.gyro.acceleration.fast_multiplier, 2.0);
    profile.gyro.pause_input = Some(profile.gyro.activation.clone());
    assert!(profile.validate().is_err());
    profile.gyro.pause_input = None;
    profile.gyro.acceleration.full_dps = profile.gyro.acceleration.start_dps;
    assert!(profile.validate().is_err());
    profile.gyro.acceleration = Default::default();
    profile.virtual_gamepad = GamepadTarget::None;
    assert!(profile.validate().is_err());
}
#[test]
fn corrupt_profile_is_preserved_and_reported_and_legacy_jsm_is_rejected() {
    let tmp = Sandbox::new();
    let store = Store::open(tmp.0.clone()).unwrap();
    let path = store.profile_path("gyro").unwrap();
    std::fs::write(&path, "broken = [").unwrap();
    let (entries, warnings) = store.list().unwrap();
    assert!(!entries.iter().any(|p| p.id == "gyro"));
    assert_eq!(warnings.len(), 1);
    assert_eq!(std::fs::read_to_string(path).unwrap(), "broken = [");
    assert!(parse_profile("RESET_MAPPINGS\nGYRO_SENS = 1 1").is_err());
}
#[test]
fn setting_backup_recovers_without_silently_overwriting_corruption() {
    let tmp = Sandbox::new();
    let store = Store::open(tmp.0.clone()).unwrap();
    let mut settings = store.settings().unwrap();
    store.save_settings(&settings).unwrap();
    settings.cursor_enabled = true;
    store.save_settings(&settings).unwrap();
    std::fs::write(tmp.0.join("settings.json"), "invalid").unwrap();
    assert!(store.settings().is_err());
    assert!(!store.restore_settings().unwrap().cursor_enabled);
}
#[test]
fn cursor_edges_debounce_and_do_not_trigger_at_startup() {
    let mut edges = CursorEdges::default();
    let now = Instant::now();
    assert_eq!(edges.update(true, now), None);
    assert_eq!(edges.update(true, now + Duration::from_millis(251)), None);
    assert_eq!(edges.update(false, now + Duration::from_millis(300)), None);
    assert_eq!(edges.update(true, now + Duration::from_millis(400)), None);
    assert_eq!(edges.update(false, now + Duration::from_millis(500)), None);
    assert_eq!(
        edges.update(false, now + Duration::from_millis(751)),
        Some(false)
    );
    assert_eq!(edges.update(false, now + Duration::from_millis(900)), None);
}
#[test]
fn cursor_delay_zero_confirms_the_first_changed_sample_without_a_startup_edge() {
    let mut edges = CursorEdges::default();
    let now = Instant::now();
    assert_eq!(edges.update_with_delay(true, now, Duration::ZERO), None);
    assert_eq!(
        edges.update_with_delay(false, now + Duration::from_millis(1), Duration::ZERO),
        Some(false)
    );
    assert_eq!(
        edges.update_with_delay(false, now + Duration::from_millis(2), Duration::ZERO),
        None
    );
    assert_eq!(
        edges.update_with_delay(true, now + Duration::from_millis(3), Duration::ZERO),
        Some(true)
    );
}
#[test]
fn cursor_delay_uses_configured_threshold_and_discards_transient_changes() {
    let mut edges = CursorEdges::default();
    let now = Instant::now();
    let delay = Duration::from_millis(100);
    assert_eq!(edges.update_with_delay(true, now, delay), None);
    assert_eq!(edges.update_with_delay(true, now + delay, delay), None);
    assert_eq!(
        edges.update_with_delay(false, now + Duration::from_millis(110), delay),
        None
    );
    assert_eq!(
        edges.update_with_delay(true, now + Duration::from_millis(150), delay),
        None
    );
    assert_eq!(
        edges.update_with_delay(false, now + Duration::from_millis(180), delay),
        None
    );
    assert_eq!(
        edges.update_with_delay(false, now + Duration::from_millis(279), delay),
        None
    );
    assert_eq!(
        edges.update_with_delay(false, now + Duration::from_millis(280), delay),
        Some(false)
    );
}
#[test]
fn cursor_delay_settings_preserve_old_default_and_validate_bounds() {
    let mut settings: motion_core::storage::Settings =
        serde_json::from_str(r#"{"version":1}"#).unwrap();
    assert_eq!(settings.cursor_debounce_ms, 250);
    for value in [0, 1, 100, 250, 5000] {
        settings.cursor_debounce_ms = value;
        settings.validate().unwrap();
        let roundtrip: motion_core::storage::Settings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(roundtrip.cursor_debounce_ms, value);
    }
    settings.cursor_debounce_ms = 5001;
    assert!(settings.validate().is_err());
}
#[test]
fn independent_empty_targets_and_restore_preserve_origin_across_switches() {
    let mut history = SwitchHistory::default();
    assert_eq!(history.apply("combat", &SwitchTarget::None), None);
    assert_eq!(
        history.apply("combat", &SwitchTarget::Profile("menu".into())),
        Some("menu".into())
    );
    assert_eq!(
        history.apply("menu", &SwitchTarget::Profile("cursor".into())),
        Some("cursor".into())
    );
    assert_eq!(
        history.apply("cursor", &SwitchTarget::Restore),
        Some("combat".into())
    );
    assert_eq!(history.apply("combat", &SwitchTarget::Restore), None);
    assert_eq!(history.toggle("combat", "menu"), Some("menu".into()));
    assert_eq!(history.toggle("menu", "menu"), Some("combat".into()));
}
#[test]
fn ipc_rejects_truncated_and_oversized_messages() {
    use motion_core::protocol::{read_packet, Request, MAX_MESSAGE};
    let mut input = std::io::Cursor::new(b"{\"id\":1,\"type\":\"stop\"}\n");
    assert!(read_packet::<Request>(&mut input).unwrap().is_some());
    assert!(read_packet::<Request>(&mut input).unwrap().is_none());
    let mut input = std::io::Cursor::new(b"{\"id\":1,\"type\":\"stop\"}");
    assert!(read_packet::<Request>(&mut input).is_err());
    let mut input = std::io::Cursor::new(vec![b' '; MAX_MESSAGE + 1]);
    assert!(read_packet::<Request>(&mut input).is_err());
}
