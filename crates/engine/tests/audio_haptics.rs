use motion_core::{layers, profile::GamepadTarget, storage::parse_profile};

#[test]
fn old_profiles_default_off_and_dualsense_settings_round_trip() {
    let mut profile = parse_profile(include_str!("../../../examples/blank.toml")).unwrap();
    assert!(!profile.feedback.audio_haptics_enabled);
    assert_eq!(profile.feedback.audio_haptics_gain, 1.);
    profile.virtual_gamepad = GamepadTarget::DualSense;
    profile.gamepad_bindings = None;
    profile.materialize_gamepad_bindings();
    assert!(profile
        .gamepad_bindings
        .as_ref()
        .unwrap()
        .iter()
        .any(|b| b.output == "touchpad"));
    profile.feedback.audio_haptics_enabled = true;
    profile.feedback.audio_haptics_gain = 1.25;
    let loaded = parse_profile(&toml::to_string(&profile).unwrap()).unwrap();
    assert_eq!(loaded.virtual_gamepad, GamepadTarget::DualSense);
    assert_eq!(loaded.feedback, profile.feedback);
}

#[test]
fn layers_can_change_gain_but_cannot_switch_hardware_modes() {
    let mut profile = parse_profile(include_str!("../../../examples/blank.toml")).unwrap();
    profile.virtual_gamepad = GamepadTarget::DualSense;
    profile.feedback.audio_haptics_enabled = true;
    profile.layers.push(
        serde_json::from_value(serde_json::json!({
            "id":"quiet", "name":"Quiet haptics", "input":"left_trigger",
            "settings":{"feedback":{"audio_haptics_gain":0.5}}
        }))
        .unwrap(),
    );
    profile.validate().unwrap();
    let resolved = layers::resolve(&profile, &[0]).unwrap();
    assert_eq!(resolved.feedback.audio_haptics_gain, 0.5);
    assert!(resolved.feedback.audio_haptics_enabled);
    profile.layers[0].settings["feedback"]["audio_haptics_enabled"] = false.into();
    assert!(profile.validate().is_err());
}
