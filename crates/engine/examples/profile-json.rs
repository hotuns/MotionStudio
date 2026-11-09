//! Export a validated profile and local defaults for frontend development.
use anyhow::{Context, Result};
fn main() -> Result<()> {
    let path = std::env::args().nth(1).context(
        "Usage: cargo run -p motion-core --example profile-json -- examples/gyro-gamepad.toml",
    )?;
    let profile = motion_core::profile::Profile::load(std::path::Path::new(&path))?;
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "profile":profile,"sensor":motion_core::motion_config::SensorCalibration::default(),
            "telemetry":motion_core::motion::MotionTelemetry::default()
        }))?
    );
    Ok(())
}
