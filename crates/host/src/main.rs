#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "console")]
mod calibration;
mod guardian;
mod runtime;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let result = if args
        .get(1)
        .is_some_and(|s| s == "--hidmaestro-output" || s == "--hidmaestro-install")
    {
        (|| -> anyhow::Result<()> {
            anyhow::ensure!(args.len() == 4, "Invalid HIDMaestro helper arguments");
            let pid = args[2].parse::<u32>()?;
            motion_core::hidmaestro::run_helper(pid, &args[3], args[1] == "--hidmaestro-install")
        })()
    } else if args.get(1).is_some_and(|s| s == "--hidhide-lease") {
        (|| -> anyhow::Result<()> {
            anyhow::ensure!(args.len() == 4, "Invalid visibility helper arguments");
            #[cfg(windows)]
            motion_core::visibility::run_helper(args[2].parse()?, &args[3])?;
            Ok(())
        })()
    } else if args.iter().any(|a| a == "--guardian") {
        guardian::run()
    } else {
        let dll = args
            .windows(2)
            .find(|a| a[0] == "--sdl")
            .map(|a| std::path::PathBuf::from(&a[1]))
            .unwrap_or_else(|| std::env::current_exe().unwrap().with_file_name("SDL3.dll"));
        if args.iter().any(|a| a == "--self-test") {
            self_test(&dll)
        } else if args.iter().any(|a| a == "--list") {
            (|| -> anyhow::Result<()> {
                let mut sdl = motion_core::sdl::Sdl::load(&dll)?;
                println!("{}", serde_json::to_string_pretty(&sdl.devices()?)?);
                Ok(())
            })()
        } else {
            std::panic::catch_unwind(|| runtime::run(&dll)).unwrap_or_else(|_| {
                Err(anyhow::anyhow!(
                    "Engine panicked; output guardian will release held keys"
                ))
            })
        }
    };
    if let Err(error) = result {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}
fn self_test(dll: &std::path::Path) -> anyhow::Result<()> {
    use motion_core::{
        engine::{Engine, InputEvent},
        profile::TimestampSource,
        storage::parse_profile,
    };
    let mut profile = parse_profile(include_str!("../../../examples/gyro-gamepad.toml"))?;
    profile.gyro.activation = "always".into();
    profile.gyro.timestamp_source = TimestampSource::Sensor;
    // Two sessions in the same process exercise teardown, capture reset and
    // pinned delayed imports across the same boundary used by reconnect.
    for _ in 0..2 {
        let mut sdl = motion_core::sdl::Sdl::load(dll)?;
        let trace = sdl.synthetic_trace()?;
        let mut touch_profile = profile.clone();
        touch_profile.gyro.enabled = false;
        touch_profile.virtual_gamepad = motion_core::profile::GamepadTarget::None;
        touch_profile.touchpad.enabled = true;
        let mut touch_engine = Engine::new(touch_profile)?;
        let mut touch_total = 0.;
        for event in &trace {
            if matches!(event, InputEvent::Touch { .. }) {
                touch_total += touch_engine.process(event).mouse_delta[0];
            }
        }
        anyhow::ensure!(
            (touch_total - 140.).abs() < 0.001,
            "Native SDL touch to trackpad path failed"
        );
        let mut engine = Engine::new(profile.clone())?;
        let mut stick_samples = 0;
        for event in trace {
            let frame = engine.process(&event);
            if matches!(event, InputEvent::Gyro { .. })
                && frame.pad.is_some_and(|pad| pad.right_x > 0)
            {
                anyhow::ensure!(
                    frame.mouse_delta == [0.0; 2],
                    "Right stick emitted mouse movement"
                );
                stick_samples += 1;
            }
        }
        anyhow::ensure!(
            engine.gyro_events == 3 && stick_samples == 2,
            "FlexInput SDL sensor to RWS stick path failed"
        );
    }
    println!(r#"{{"ok":true,"gyro_events":3,"touch_events":3,"system_output":false}}"#);
    Ok(())
}
