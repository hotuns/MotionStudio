use motion_core::{
    engine::{Engine, InputEvent},
    jsm_import::{import, import_files, JsmFile},
    profile::GamepadTarget,
    storage::parse_profile,
    timing::SampleTime,
};
fn convert(text: &str) -> motion_core::jsm_import::JsmImport {
    let p = import(text, "test.txt").unwrap();
    Engine::new(p.profile.clone()).unwrap();
    p
}
#[test]
fn imported_flick_uses_jsm_release_curve_and_retains_its_duration() {
    let p = convert("GYRO_SENS=2\nRIGHT_STICK_MODE=FLICK\nFLICK_TIME=0.15");
    assert!(p.profile.gyro.motion.flick.enabled);
    assert_eq!(p.profile.gyro.motion.flick.release_ease, 1.);
    assert_eq!(p.profile.gyro.motion.flick.smooth_ms, 150.);
}

#[test]
fn per_side_stick_deadzone_commands_feed_native_source_settings() {
    let p = convert("LEFT_STICK_DEADZONE_INNER = 0.18\nRIGHT_STICK_DEADZONE_INNER = 0.23");
    assert_eq!(p.profile.input.deadzone_left, Some(0.18));
    assert_eq!(p.profile.input.deadzone_right, Some(0.23));
    assert_eq!(p.skipped, 0);
}
#[test]
fn basic_buttons_wheel_side_buttons_and_studio_names_migrate() {
    let p=convert("\u{feff}# JSM_STUDIO_GAME {\"gameName\":\"测试游戏\"}\r\n# JSM_STUDIO_ACTION {\"button\":\"S\",\"key\":\"SPACE\",\"zh\":\"跳跃\",\"gesture\":\"press\"}\r\nRESET_MAPPINGS\r\nS = SPACE # comment\r\nN = SCROLLDOWN\nL = BMOUSE\nR = FMOUSE\n+ = ESC\n- = TAB");
    assert_eq!(p.profile.name, "测试游戏");
    assert_eq!(p.profile.actions.len(), 6);
    assert_eq!(p.profile.actions[0].name, "跳跃");
    assert_eq!(p.profile.actions[1].output, "wheel:down");
    assert_eq!(p.profile.actions[2].output, "mouse:x1");
    assert_eq!(p.profile.actions[3].output, "mouse:x2");
    assert!(!p.profile.gyro.enabled);
    assert_eq!(p.skipped, 0);
}
#[test]
fn tap_hold_toggle_and_instant_are_never_mistaken_for_held_chords() {
    let p = convert("S = SPACE\nE = C Z\nW = ^R\nN = !G\nUP = CTRL\\ K\\\nDOWN = F12");
    assert_eq!(p.profile.actions.len(), 2);
    assert_eq!(p.profile.actions[1].output, "key:CTRL+key:K");
    assert_eq!(p.skipped, 4);
    assert!(p
        .lines
        .iter()
        .find(|l| l.source == "E = C Z")
        .unwrap()
        .message
        .contains("轻按"));
}

#[test]
fn caps_lock_names_and_chords_import_as_a_normal_keyboard_key() {
    let p = convert("S = CAPS_LOCK\nE = CAPSLOCK\nN = CTRL\\ CAPS_LOCK\\");
    assert_eq!(p.skipped, 0);
    assert_eq!(p.profile.actions.len(), 3);
    assert_eq!(p.profile.actions[0].output, "key:CAPSLOCK");
    assert_eq!(p.profile.actions[1].output, "key:CAPSLOCK");
    assert_eq!(p.profile.actions[2].output, "key:CTRL+key:CAPSLOCK");
}

#[test]
fn extended_keyboard_names_keep_numpad_and_right_modifiers_distinct() {
    for (source, expected) in [
        ("ESC", "ESC"),
        ("HOME", "HOME"),
        ("SCREENSHOT", "PRINTSCREEN"),
        ("N0", "NUMPAD0"),
        ("N9", "NUMPAD9"),
        ("ADD", "NUMPADADD"),
        ("DECIMAL", "NUMPADDECIMAL"),
        ("RCONTROL", "RCTRL"),
        ("RSHIFT", "RSHIFT"),
        ("RWINDOWS", "RWIN"),
        ("NUM_LOCK", "NUMLOCK"),
        ("SCROLL_LOCK", "SCROLLLOCK"),
        ("OPENBRACKET", "LEFTBRACKET"),
        ("F24", "F24"),
    ] {
        let p = convert(&format!("S = {source}"));
        assert_eq!(p.skipped, 0, "{source}");
        assert_eq!(p.profile.actions[0].output, format!("key:{expected}"));
    }
}
#[test]
fn input_chords_report_semantic_differences_and_double_press_is_skipped() {
    let p = convert("L,S = SPACE\nUP*RIGHT = K\nL+R = M\nS,S = G\nL,GYRO_SENS = 2");
    assert_eq!(p.profile.actions.len(), 3);
    assert_eq!(p.profile.actions[0].input, "left_shoulder+south");
    assert_eq!(p.approximate, 2);
    assert_eq!(p.skipped, 2);
}
#[test]
fn last_definition_and_reset_remove_previous_effects_even_if_replacement_is_unsupported() {
    let p=convert("S=SPACE\nS=F12\nE=Q\nRESET_MAPPINGS\nN=SCROLLUP\nN=NONE\nW=R\nGYRO_SENS=5\nRESET_MAPPINGS\nS=ENTER");
    assert_eq!(p.profile.actions.len(), 1);
    assert_eq!(p.profile.actions[0].output, "key:ENTER");
    assert!(!p.profile.gyro.enabled);
    let p = convert("S=SPACE\nS=F12");
    assert!(p.profile.actions.is_empty());
    assert_eq!(p.lines[0].status, "overridden");
}
#[test]
fn calibration_sensitivity_axes_and_hold_activation_are_translated_in_units() {
    let p=convert("REAL_WORLD_CALIBRATION=80\nIN_GAME_SENS=2\nGYRO_SENS=2 1\nGYRO_ON=ZL\nGYRO_AXIS_Y=INVERTED\nGYRO_SMOOTH_TIME=0.02\nGYRO_SPACE=LOCAL");
    let g = &p.profile.gyro;
    assert!(g.enabled);
    assert_eq!(g.counts_per_degree, 40.);
    assert_eq!(g.sensitivity, 2.);
    assert_eq!(g.vertical_ratio, 0.5);
    assert!(g.invert_y);
    assert_eq!(g.activation, "left_trigger_soft");
    assert_eq!(g.smoothing_ms, 20.);
}
#[test]
fn a_later_gyro_sens_assignment_sets_both_limits_and_independent_max_is_preserved() {
    let p=convert("MIN_GYRO_SENS=1\nMAX_GYRO_SENS=4\nGYRO_SENS=2\nMAX_GYRO_SENS=6\nMIN_GYRO_THRESHOLD=10\nMAX_GYRO_THRESHOLD=80");
    assert_eq!(p.profile.gyro.sensitivity, 2.);
    assert_eq!(p.profile.gyro.acceleration.fast_multiplier, 3.);
    assert_eq!(p.profile.gyro.acceleration.start_dps, 10.);
    assert_eq!(p.profile.gyro.acceleration.full_dps, 80.);
}
#[test]
fn invalid_numbers_do_not_discard_other_supported_actions_or_enable_gyro() {
    let p=convert("GYRO_SENS=NaN\nREAL_WORLD_CALIBRATION=Infinity\nIN_GAME_SENS=0\nTRIGGER_THRESHOLD=2\nS=SPACE\nGYRO_SMOOTH_TIME=0.125");
    assert_eq!(p.profile.actions.len(), 1);
    assert!(!p.profile.gyro.enabled);
    assert_eq!(p.profile.gyro.smoothing_ms, 0.);
    assert!(p.skipped >= 4);
}
#[test]
fn gyro_can_be_disabled_without_losing_standalone_buttons() {
    let p = convert("GYRO_SENS=2\nGYRO_ON=NONE\nS=SPACE\nRIGHT_STICK_MODE=AIM");
    assert!(!p.profile.gyro.enabled);
    assert!(!p.profile.gyro.motion.flick.stick_aim);
    assert_eq!(p.profile.actions.len(), 1);
}
#[test]
fn external_commands_are_reported_and_never_followed_or_executed() {
    let p=convert("C:/missing/other.txt\nRECONNECT_CONTROLLERS\nSLEEP 10\nS=\"RUN SOMETHING\"\nAUTOLOAD=ON\nTELEMETRY_PORT=8974\nS=SPACE");
    assert_eq!(p.profile.actions.len(), 1);
    assert!(p.skipped >= 3);
    assert_eq!(p.lines.iter().filter(|l| l.status == "ignored").count(), 2);
}
fn native(target: &str, prefix: &str) -> String {
    let buttons = if prefix == "X" {
        [
            "UP=X_UP",
            "DOWN=X_DOWN",
            "LEFT=X_LEFT",
            "RIGHT=X_RIGHT",
            "L=X_LB",
            "R=X_RB",
            "W=X_X",
            "S=X_A",
            "N=X_Y",
            "E=X_B",
            "L3=X_LS",
            "R3=X_RS",
            "-=X_BACK",
            "+=X_START",
            "HOME=X_GUIDE",
        ]
    } else {
        [
            "UP=PS_UP",
            "DOWN=PS_DOWN",
            "LEFT=PS_LEFT",
            "RIGHT=PS_RIGHT",
            "L=PS_L1",
            "R=PS_R1",
            "W=PS_SQUARE",
            "S=PS_CROSS",
            "N=PS_TRIANGLE",
            "E=PS_CIRCLE",
            "L3=PS_L3",
            "R3=PS_R3",
            "-=PS_SHARE",
            "+=PS_OPTIONS",
            "HOME=PS_HOME",
        ]
    };
    format!("VIRTUAL_CONTROLLER={target}\n{}\nLEFT_STICK_MODE=LEFT_STICK\nRIGHT_STICK_MODE=RIGHT_STICK\nZL_MODE={}\nZR_MODE={}",buttons.join("\n"),if prefix=="X"{"X_LT"}else{"PS_L2"},if prefix=="X"{"X_RT"}else{"PS_R2"})
}
#[test]
fn standard_native_layouts_migrate_but_partial_or_remapped_pads_do_not_gain_extra_automapping() {
    assert_eq!(
        convert(&native("XBOX", "X")).profile.virtual_gamepad,
        GamepadTarget::Xbox360
    );
    assert_eq!(
        convert(&native("DS4", "PS")).profile.virtual_gamepad,
        GamepadTarget::DualShock4
    );
    assert_eq!(
        convert("VIRTUAL_CONTROLLER=XBOX\nS=X_A")
            .profile
            .virtual_gamepad,
        GamepadTarget::None
    );
    assert_eq!(
        convert(&(native("XBOX", "X") + "\nS=X_B"))
            .profile
            .virtual_gamepad,
        GamepadTarget::None
    );
}
#[test]
fn no_gyro_button_and_gyro_off_choose_the_last_gate() {
    let p = convert("GYRO_SENS=2\nGYRO_ON=ZL\nNO_GYRO_BUTTON\nGYRO_OFF=R3");
    assert_eq!(p.profile.gyro.activation, "always");
    assert_eq!(
        p.profile.gyro.pause_input.as_deref(),
        Some("right_stick_click")
    );
}
#[test]
fn bound_gyro_actions_use_existing_gates_without_running_jsm_commands() {
    let p = convert("GYRO_SENS=2\nGYRO_ON=NONE\nL=^GYRO_ON\nR=GYRO_TRACKBALL");
    assert!(p.profile.gyro.enabled);
    assert_eq!(p.profile.gyro.activation, "left_shoulder");
    assert!(matches!(
        p.profile.gyro.activation_mode,
        motion_core::profile::GyroActivationMode::Toggle
    ));
    assert_eq!(
        p.profile.gyro.motion.trackball.horizontal_input.as_deref(),
        Some("right_shoulder")
    );
    assert_eq!(
        p.profile.gyro.motion.trackball.vertical_input.as_deref(),
        Some("right_shoulder")
    );
    assert!(p.profile.actions.is_empty());
}
#[test]
fn unsupported_gyro_output_gate_or_axes_cannot_fall_back_to_unconditional_mouse_motion() {
    for setting in [
        "GYRO_OUTPUT=PS_MOTION",
        "GYRO_ON=LEFT_STICK",
        "MOUSE_X_FROM_GYRO_AXIS=X",
        "REAL_WORLD_CALIBRATION=NaN",
    ] {
        let p = convert(&format!("GYRO_SENS=2\n{setting}\nS=SPACE"));
        assert!(!p.profile.gyro.enabled, "{setting}");
        assert_eq!(p.profile.actions.len(), 1);
        assert!(p.warnings.iter().any(|w| w.contains("已关闭体感")));
    }
}
#[test]
fn fork_curves_and_one_euro_settings_are_converted_to_existing_algorithms() {
    let p=convert("GYRO_SENS=2\nMAX_GYRO_SENS=4\nACCEL_CURVE=POWER\nACCEL_POWER_VREF=0.02\nACCEL_POWER_EXPONENT=0.7\nONE_EURO_FILTER\nONE_EURO_MIN_CUTOFF=4\nONE_EURO_SPEED_COEFF=0.5");
    assert!(matches!(
        p.profile.gyro.acceleration.curve,
        motion_core::motion_config::Curve::Power
    ));
    assert_eq!(p.profile.gyro.acceleration.power_reference, 0.02);
    assert_eq!(p.profile.gyro.acceleration.power_exponent, 0.7);
    assert!(matches!(
        p.profile.gyro.motion.smoothing,
        motion_core::motion_config::Smoothing::OneEuro
    ));
    assert_eq!(p.profile.gyro.motion.one_euro_cutoff, 4.);
    assert_eq!(p.profile.gyro.motion.one_euro_beta, 0.5);
}
#[test]
fn size_limits_empty_input_and_binary_text_are_rejected() {
    assert!(import("# just a comment", "a.txt").is_err());
    assert!(import("\0S=SPACE", "a.txt").is_err());
    assert!(import(&"x".repeat(512 * 1024 + 1), "a.txt").is_err());
}

fn pull(ms: u64, axis: u8, value: f32) -> InputEvent {
    InputEvent::Axis {
        time: SampleTime {
            host_ns: ms * 1_000_000,
            sensor_ns: 0,
        },
        index: axis,
        value: (value * f32::from(i16::MAX)).round() as i16,
    }
}

#[test]
fn imported_triggers_are_editable_and_follow_native_coexisting_and_exclusive_modes() {
    let report = convert("TRIGGER_THRESHOLD = .25\nZL_MODE = NO_SKIP\nZR_MODE = NO_SKIP_EXCLUSIVE\nZL = Q\nZLF = E\nZR = RMOUSE\nZRF = LMOUSE");
    assert_eq!(report.skipped + report.approximate, 0);
    assert_eq!(
        report
            .profile
            .actions
            .iter()
            .map(|a| a.input.as_str())
            .collect::<Vec<_>>(),
        [
            "left_trigger_soft",
            "left_trigger_full",
            "right_trigger_soft",
            "right_trigger_full"
        ]
    );
    let saved = toml::to_string_pretty(&report.profile).unwrap();
    assert!(!saved.contains("[jsm]"));
    let mut engine = Engine::new(parse_profile(&saved).unwrap()).unwrap();
    assert!(engine.process(&pull(10, 4, 0.15)).held.is_empty());
    assert!(engine.process(&pull(15, 4, 0.4)).held.contains("key:Q"));
    let both = engine.process(&pull(20, 4, 1.));
    assert!(both.held.contains("key:Q") && both.held.contains("key:E"));
    engine.reset();
    assert!(engine
        .process(&pull(25, 5, 0.4))
        .held
        .contains("mouse:right"));
    let full = engine.process(&pull(30, 5, 1.));
    assert!(!full.held.contains("mouse:right") && full.held.contains("mouse:left"));
    let soft = engine.process(&pull(35, 5, 0.4));
    assert!(soft.held.contains("mouse:right") && !soft.held.contains("mouse:left"));
    assert!(engine.process(&pull(40, 5, 0.)).held.is_empty());
}

#[test]
fn skip_timer_and_full_pull_use_calibrated_analog_travel_even_with_digital_output() {
    let mut profile = convert("TRIGGER_THRESHOLD = .25\nTRIGGER_SKIP_DELAY = 20\nZR_MODE = MAY_SKIP\nZR = RMOUSE\nZRF = LMOUSE").profile;
    profile.input.digital_triggers = true;
    profile.input.rtrig_max = 0.8;
    let mut engine = Engine::new(profile).unwrap();
    assert!(engine.process(&pull(10, 5, 0.6)).held.is_empty());
    assert!(engine.advance(0.004, 15_000_000).held.is_empty());
    let quick = engine.process(&pull(20, 5, 0.81));
    assert!(!quick.held.contains("mouse:right") && quick.held.contains("mouse:left"));
    engine.reset();
    assert!(engine.process(&pull(30, 5, 0.6)).held.is_empty());
    let after_wait = engine.advance(0.021, 51_000_000);
    assert!(after_wait.held.contains("mouse:right") && !after_wait.held.contains("mouse:left"));
    let full = engine.process(&pull(56, 5, 0.81));
    assert!(full.held.contains("mouse:right") && full.held.contains("mouse:left"));
    engine.reset();
    assert!(engine.process_snapshot(&pull(60, 5, 1.)).held.is_empty());
    assert!(engine.advance(0.004, 65_000_000).held.is_empty());
    assert!(engine.process(&pull(70, 5, 0.6)).held.is_empty());
    assert!(engine.process(&pull(74, 5, 0.)).held.is_empty());
    engine.process(&pull(78, 5, 0.6));
    assert!(engine.process(&pull(82, 5, 1.)).held.contains("mouse:left"));
}

#[test]
fn a_full_binding_with_no_full_mode_is_reported_and_stays_inactive() {
    let report = convert("ZRF = LMOUSE");
    assert_eq!(report.approximate, 1);
    assert_eq!(report.profile.actions[0].input, "right_trigger_full");
    let mut engine = Engine::new(report.profile).unwrap();
    engine.process(&pull(10, 5, 0.4));
    assert!(engine.process(&pull(14, 5, 1.)).held.is_empty());
}

#[test]
fn converts_legacy_profile_and_uses_only_supplied_static_includes_in_order() {
    let old = r#"
schema_version = 1
name = "Previously saved"
virtual_gamepad = "none"
[jsm]
passthrough = true
[[jsm.files]]
name = "Main.txt"
text = '''
ZR_MODE = NO_FULL
GyroConfigs/Aim.txt
Unknown.txt
ZR = BMOUSE
'''
[[jsm.files]]
name = "Aim.txt"
text = '''
ZR_MODE = NO_SKIP
ZR = RMOUSE
ZRF = LMOUSE
'''
"#;
    assert!(parse_profile(old).is_err());
    let report = import_files(&[JsmFile {
        name: "old.toml".into(),
        text: old.into(),
    }])
    .unwrap();
    assert_eq!(report.profile.name, "Previously saved");
    assert_eq!(report.skipped, 1);
    assert_eq!(report.profile.actions[1].output, "mouse:x1");
    assert!(report
        .lines
        .iter()
        .any(|line| { line.file == "Aim.txt" && line.line == 3 && line.status == "converted" }));
    let mut engine = Engine::new(report.profile).unwrap();
    assert!(engine.process(&pull(10, 5, 0.4)).held.contains("mouse:x1"));
    let full = engine.process(&pull(15, 5, 1.));
    assert!(full.held.contains("mouse:x1") && full.held.contains("mouse:left"));
}
