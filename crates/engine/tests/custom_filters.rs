use motion_core::{
    custom_filters::{FilterConfig, Stage},
    engine::{Engine, InputEvent},
    motion_config::MotionMode,
    profile::{GamepadTarget, GyroOutput, Profile, TimestampSource},
    storage::parse_profile,
    timing::SampleTime,
};

fn profile() -> Profile {
    let mut p = parse_profile(include_str!("../../../examples/gyro-mouse.toml")).unwrap();
    p.gyro.activation = "always".into();
    p.gyro.smoothing_ms = 0.;
    p.gyro.noise_threshold = 0.;
    p.gyro.timestamp_source = TimestampSource::Sensor;
    p
}
// Studio's stick-direction rotation still has raw, before- and after-filter stages.
fn scripted_profile() -> Profile {
    let mut p = profile();
    p.virtual_gamepad = GamepadTarget::Xbox360;
    p.gyro.output = GyroOutput::RightStick;
    p.gyro.motion.mode = MotionMode::StickRotation;
    p
}
fn scripted_engine(p: Profile) -> Engine {
    let mut engine = Engine::new(p).unwrap();
    engine.process(&InputEvent::Axis {
        time: SampleTime { host_ns: 1_000_000, sensor_ns: 0 }, index: 3, value: -16384,
    });
    engine
}
fn filter(source: &str, stage: Stage) -> FilterConfig {
    FilterConfig {
        id: "custom".into(),
        name: "Custom".into(),
        stage,
        source: source.into(),
        ..Default::default()
    }
}
fn gyro(engine: &mut Engine, ms: u64, yaw: f32) -> f64 {
    let before = engine.motion.telemetry.rotation_offset;
    engine.process(&InputEvent::Gyro {
        time: SampleTime {
            host_ns: ms * 1_000_000,
            sensor_ns: ms * 1_000_000,
        },
        value: [0., -yaw.to_radians(), 0.],
    });
    if engine.profile.gyro.motion.mode == MotionMode::StickRotation {
        f64::from((engine.motion.telemetry.rotation_offset - before).abs())
    } else {
        engine.frame.mouse_delta[0]
    }
}
fn total(mut p: Profile) -> f64 {
    p.gyro.counts_per_degree = 1.;
    let mut engine = scripted_engine(p);
    (1..=100).map(|n| gyro(&mut engine, n * 2, 10.)).sum()
}
#[test]
fn each_timed_sample_runs_once_and_idle_frames_do_not_replay_filters() {
    let mut p = scripted_profile();
    p.gyro.motion.custom_filters=vec![filter("fn process(s,p){if s.dt <= 0.0 || s.host_ns != s.sensor_ns {throw \"clock\";}[s.x*2.0,s.y*2.0,s.z]}",Stage::BeforeBuiltin)];
    assert!((total(p.clone()) / total(scripted_profile()) - 2.).abs() < 0.001);
    let mut engine = scripted_engine(p);
    gyro(&mut engine, 2, 10.);
    gyro(&mut engine, 4, 10.);
    assert_eq!(engine.motion.custom_filters.statuses()[0].calls, 1);
    for n in 1..10 {
        engine.advance(0.001, 4_000_000 + n * 1_000_000);
    }
    assert_eq!(engine.motion.custom_filters.statuses()[0].calls, 1);
}
#[test]
fn insertion_points_respect_the_existing_builtin_noise_processing() {
    let mut base = scripted_profile();
    base.gyro.noise_threshold = 5.;
    let baseline = total(base.clone());
    let source = "fn process(s,p){[s.x*2.0,s.y*2.0,s.z*2.0]}";
    for (stage, ratio) in [
        (Stage::RawGyro, 3.),
        (Stage::BeforeBuiltin, 3.),
        (Stage::AfterBuiltin, 2.),
    ] {
        let mut p = base.clone();
        p.gyro.motion.custom_filters = vec![filter(source, stage)];
        assert!((total(p) / baseline - ratio).abs() < 0.01, "{stage:?}");
    }
}
#[test]
fn runtime_fault_bypasses_without_losing_existing_button_outputs() {
    let mut p = scripted_profile();
    p.actions = parse_profile(include_str!("../../../examples/keyboard-mouse.toml"))
        .unwrap()
        .actions;
    p.gyro.motion.custom_filters = vec![filter("fn process(s,p){loop{}}", Stage::AfterBuiltin)];
    let mut engine = scripted_engine(p);
    engine.process(&InputEvent::Button {
        time: SampleTime {
            host_ns: 1_000_000,
            sensor_ns: 1_000_000,
        },
        index: 0,
        down: true,
    });
    let held = engine.frame.held.clone();
    assert!(!held.is_empty());
    gyro(&mut engine, 2, 10.);
    gyro(&mut engine, 4, 10.);
    let displacement = gyro(&mut engine, 6, 10.);
    assert_eq!(engine.frame.held, held);
    let status = engine.motion.custom_filters.statuses();
    assert!(status[0].bypassed);
    assert_eq!(status[0].calls, 1);
    assert!(displacement > 0.);
}
#[test]
fn unchanged_layers_keep_state_and_invalid_layers_do_not_partly_apply() {
    for mut p in [profile(), scripted_profile()] {
        p.gyro.motion.custom_filters = vec![filter(
            "fn process(s,p){if !this.contains(\"n\"){this.n=0.0;}this.n+=10.0;[this.n,s.y,s.z]}",
            Stage::BeforeBuiltin,
        )];
        let mut engine = scripted_engine(p.clone());
        gyro(&mut engine, 2, 10.);
        let first = gyro(&mut engine, 4, 10.);
        p.gyro.sensitivity *= 2.;
        engine.apply_layer(p.clone()).unwrap();
        let second = gyro(&mut engine, 6, 10.);
        assert!((second / first - 4.).abs() < 0.01, "first {first}, second {second}");
        assert_eq!(engine.motion.custom_filters.statuses()[0].calls, 2);
        let mut invalid = p.clone();
        invalid.gyro.motion.custom_filters[0].source = "invalid".into();
        invalid.gyro.sensitivity = 8.;
        assert!(engine.apply_layer(invalid).is_err());
        assert_eq!(engine.profile.gyro.sensitivity, p.gyro.sensitivity);
    }
}
#[test]
fn angular_mouse_and_stick_apply_all_stages_once_per_sensor_sample() {
    for output in [GyroOutput::Mouse, GyroOutput::RightStick] {
        let mut p = profile();
        p.gyro.output = output;
        p.virtual_gamepad = GamepadTarget::Xbox360;
        p.gyro.counts_per_degree = 1.;
        p.gyro.stick_full_scale_dps = 120.;
        p.gyro.sensitivity = 2.;
        p.gyro.vertical_ratio = 0.5;
        p.gyro.invert_y = true;
        let raw = filter("fn process(s,p){[s.x, s.y+s.z, s.z*2.0]}", Stage::RawGyro);
        let mut before = filter(r#"fn process(s,p){
            if abs(s.raw_z-10.0)>0.001 || abs(s.x-20.0)>0.001 || abs(s.y+10.0)>0.001 {throw "projection";}
            [s.x+5.0, s.y*2.0, 0.0]
        }"#, Stage::BeforeBuiltin);
        before.id = "projected".into();
        let mut after = filter(r#"fn process(s,p){
            if abs(s.x-25.0)>0.001 || abs(s.y-20.0)>0.001 || s.sensor_ns != 4000000 {throw "post-filter units";}
            [s.x+5.0, s.y+10.0, 0.0]
        }"#, Stage::AfterBuiltin);
        after.id = "final".into();
        p.gyro.motion.custom_filters = vec![after, before, raw];

        let mut engine = Engine::new(p).unwrap();
        gyro(&mut engine, 2, 10.);
        gyro(&mut engine, 4, 10.);
        if output == GyroOutput::Mouse {
            let [x, y] = engine.frame.mouse_delta;
            assert!((x - 0.12).abs() < 1e-5 && (y - 0.06).abs() < 1e-5);
        }
        for ms in [4, 5, 6] {
            let frame = engine.advance_output(0.001, ms * 1_000_000);
            assert_eq!(frame.mouse_delta, [0.; 2]);
            if output == GyroOutput::RightStick {
                let pad = frame.pad.unwrap();
                assert!((f32::from(pad.right_x) / 32767. - 0.5).abs() < 0.0001);
                assert!((f32::from(pad.right_y) / 32767. + 0.25).abs() < 0.0001);
            }
        }
        for status in engine.motion.custom_filters.statuses() {
            assert!(!status.bypassed, "{:?}", status.error);
            assert_eq!(status.calls, 1);
        }
    }
}

#[test]
fn pausing_gyro_gates_a_script_generated_rate_independently_of_flick() {
    let mut p = profile();
    p.gyro.pause_input = Some("south".into());
    p.gyro.motion.flick.enabled = true;
    p.gyro.motion.custom_filters = vec![filter("fn process(s,p){[60.0, 0.0, 0.0]}", Stage::AfterBuiltin)];
    let mut engine = Engine::new(p).unwrap();
    gyro(&mut engine, 2, 0.);
    assert!(gyro(&mut engine, 4, 0.) > 0.);
    let time = |ms| SampleTime { host_ns: ms * 1_000_000u64, sensor_ns: ms * 1_000_000u64 };
    engine.process(&InputEvent::Button { time: time(5), index: 0, down: true });
    assert_eq!(gyro(&mut engine, 6, 10.), 0.);
    engine.process(&InputEvent::Axis { time: time(7), index: 2, value: i16::MAX });
    engine.advance_output(0.002, time(8).host_ns);
    assert!(engine.advance_output(0.002, time(10).host_ns).mouse_delta[0] > 0.);
    assert_eq!(engine.motion.custom_filters.statuses()[0].calls, 1);
}

#[test]
fn game_angle_measurement_bypasses_all_custom_processing() {
    let mut p = profile();
    p.gyro.motion.custom_filters = vec![filter(
        "fn process(s,p){[s.x*10.0,s.y*10.0,s.z*10.0]}",
        Stage::RawGyro,
    )];
    p.gyro.motion.custom_filters[0].requires_accel = true;
    let mut engine = Engine::new(p).unwrap();
    engine.begin_measurement("yaw").unwrap();
    for n in 1..=101 {
        gyro(&mut engine, n * 2, 100.);
    }
    assert_eq!(engine.motion.custom_filters.statuses()[0].calls, 0);
    assert!((engine.motion.telemetry.measured_degrees - 20.).abs() < 0.05);
}
#[test]
fn old_profiles_default_to_an_empty_chain_and_scripts_roundtrip() {
    assert!(profile().gyro.motion.custom_filters.is_empty());
    let mut p = profile();
    p.gyro.motion.custom_filters = vec![filter("fn process(s,p){[s.x,s.y,s.z]}", Stage::RawGyro)];
    let stored = toml::to_string_pretty(&p).unwrap();
    let decoded = parse_profile(&stored).unwrap();
    assert_eq!(
        p.gyro.motion.custom_filters,
        decoded.gyro.motion.custom_filters
    );
}

#[test]
fn declared_accelerometer_dependency_waits_for_fresh_calibrated_accel() {
    let mut p = profile();
    let mut f=filter("fn process(s,p){if !s.accel_fresh || abs(s.accel_z+1.0)>0.01 {throw \"accel frame\";}[s.x,s.y,s.z]}",Stage::BeforeBuiltin);
    f.requires_accel = true;
    p.gyro.motion.custom_filters = vec![f];
    assert!(p.gyro.needs_accel());
    let mut engine = Engine::new(p).unwrap();
    gyro(&mut engine, 2, 10.);
    gyro(&mut engine, 4, 10.);
    assert_eq!(engine.motion.custom_filters.statuses()[0].calls, 0);
    engine.process(&InputEvent::Accel {
        time: SampleTime {
            host_ns: 5_000_000,
            sensor_ns: 5_000_000,
        },
        value: [0., 9.80665, 0.],
    });
    assert!(gyro(&mut engine, 6, 10.) > 0.);
    let status = engine.motion.custom_filters.statuses();
    assert_eq!(status[0].calls, 1);
    assert!(!status[0].bypassed, "{:?}", status[0].error);
}

#[test]
fn inactive_layer_scripts_are_validated_before_any_layer_is_activated() {
    let p = profile();
    let mut value = serde_json::to_value(&p).unwrap();
    let f = filter("invalid code", Stage::AfterBuiltin);
    value["layers"] = serde_json::json!([{"id":"hold","name":"Hold","input":"left_shoulder","settings":{"gyro":{"motion":{"custom_filters":[f]}}}}]);
    let p: Profile = serde_json::from_value(value).unwrap();
    assert!(p.validate().is_err());
}

#[test]
fn large_layer_script_payloads_are_rejected_before_exceeding_ipc_limits() {
    let mut p = profile();
    p.gyro.motion.custom_filters = (0..4)
        .map(|index| {
            let mut f = filter(&format!("//{}", "x".repeat(32766)), Stage::RawGyro);
            f.id = format!("filter{index}");
            f.enabled = false;
            f
        })
        .collect();
    assert!(p.validate().is_ok());
    let mut value = serde_json::to_value(&p).unwrap();
    value["layers"] = serde_json::json!([{"id":"hold","name":"Hold","input":"left_shoulder","settings":{"gyro":{"motion":{"custom_filters":p.gyro.motion.custom_filters}}}}]);
    let p: Profile = serde_json::from_value(value).unwrap();
    assert!(p.validate().unwrap_err().to_string().contains("256 KiB"));
}
