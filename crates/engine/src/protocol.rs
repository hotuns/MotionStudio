use crate::{profile::Profile, sdl::Device};
use anyhow::{bail, Context, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    io::{BufRead, Read},
    path::PathBuf,
};

pub const PROTOCOL_VERSION: u32 = 11;
pub const MAX_MESSAGE: usize = 512 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunConfig {
    pub profile_id: String,
    pub profile: Profile,
    pub profile_file: PathBuf,
    pub device_path: String,
    pub emit: bool,
    #[serde(default = "enabled")]
    pub auto_hide: bool,
    #[serde(default)]
    pub output_backend: crate::output::OutputBackend,
    #[serde(default)]
    pub gamepad_backend: crate::output::GamepadBackend,
    #[serde(default)]
    pub mouse_display_sync: bool,
    #[serde(default)]
    pub bias: [f32; 3],
    #[serde(default)]
    pub sensor_calibration: crate::motion_config::SensorCalibration,
}
fn enabled() -> bool {
    true
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    #[serde(flatten)]
    pub command: Command,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    Start {
        config: Box<RunConfig>,
    },
    Navigate {
        device_path: String,
    },
    Apply {
        profile_id: String,
        profile: Box<Profile>,
        profile_file: PathBuf,
    },
    Stop,
    RecoverVisibility,
    SetBindingCapture {
        active: bool,
    },
    SetPadShortcut {
        binding: Option<crate::shortcut::PadShortcut>,
    },
    Scan,
    Calibrate,
    CalibrateFace {
        axis: usize,
        positive: bool,
    },
    SetSensorCalibration {
        calibration: Box<crate::motion_config::SensorCalibration>,
    },
    BeginMeasure {
        axis: String,
        #[serde(default)]
        expected_revision: Option<u64>,
    },
    EndMeasure {
        #[serde(default)]
        expected_revision: Option<u64>,
    },
    ResetPose,
    ResetFilters,
    Shutdown,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Stats {
    pub gyro_events: u64,
    pub gyro_hz: f64,
    pub stale_events: u64,
    pub invalid_timestamps: u64,
    pub gaps: u64,
    pub process_p99_us: u64,
    pub queue_p99_us: u64,
    pub mouse_counts: [i64; 2],
    #[serde(default)]
    pub output: crate::output::OutputStats,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DisplaySyncStatus {
    pub live: bool,
    pub frame_hz: u32,
    pub reports_per_frame: u32,
    #[serde(default)]
    pub vrr: bool,
    #[serde(default)]
    pub fallback_hz: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Status {
    pub state: String,
    pub message: String,
    pub profile_id: Option<String>,
    pub profile_name: Option<String>,
    pub device: Option<Device>,
    pub emitting: bool,
    #[serde(default)]
    pub binding_capture: bool,
    #[serde(default)]
    pub active_layers: Vec<String>,
    #[serde(default)]
    pub visibility: crate::visibility::VisibilityStatus,
    #[serde(default)]
    pub output_backend: Option<crate::output::OutputBackend>,
    #[serde(default)]
    pub output_gamepad: Option<crate::output::GamepadStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mouse_display_sync: Option<DisplaySyncStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_preview: Option<crate::output::OutputPreview>,
    #[serde(default)]
    pub feedback: crate::feedback::FeedbackStatus,
    pub revision: u64,
    pub reconnects: u64,
    pub reload_error: Option<String>,
    pub buttons: Vec<bool>,
    pub axes: [f32; 6],
    #[serde(default)]
    pub processed_axes: [f32; 6],
    #[serde(default)]
    pub stick_regions: [crate::profile::StickRegion; 2],
    #[serde(default)]
    pub pad_region_capture: PadRegionCapture,
    #[serde(default)]
    pub touches: crate::touchpad::Contacts,
    pub gyro: [f32; 3],
    pub calibration_progress: f32,
    #[serde(default)]
    pub motion: crate::motion::MotionTelemetry,
    pub input_serial: u64,
    #[serde(default)]
    pub pad_shortcut: Option<crate::shortcut::PadShortcut>,
    #[serde(default)]
    pub pad_shortcut_serial: u64,
    #[serde(default)]
    pub pad_shortcut_baseline: u64,
    pub last_input: Option<String>,
    #[serde(default)]
    pub input_chord: Vec<String>,
    #[serde(default)]
    pub ui_navigation: bool,
    #[serde(default)]
    pub navigation_serial: u64,
    #[serde(default)]
    pub navigation_events: Vec<crate::navigation::NavEvent>,
    #[serde(default)]
    pub navigation_held: Vec<crate::navigation::NavCommand>,
    pub stats: Stats,
}
impl Default for Status {
    fn default() -> Self {
        Self {
            state: "idle".into(),
            message: "引擎已就绪".into(),
            profile_id: None,
            profile_name: None,
            device: None,
            emitting: false,
            binding_capture: false,
            active_layers: vec![],
            visibility: Default::default(),
            output_backend: None,
            output_gamepad: None,
            mouse_display_sync: None,
            output_preview: None,
            feedback: Default::default(),
            revision: 0,
            reconnects: 0,
            reload_error: None,
            buttons: vec![false; 26],
            axes: [0.0; 6],
            processed_axes: [0.0; 6],
            stick_regions: Default::default(),
            pad_region_capture: Default::default(),
            touches: Default::default(),
            gyro: [0.0; 3],
            calibration_progress: 0.0,
            motion: Default::default(),
            input_serial: 0,
            pad_shortcut: None,
            pad_shortcut_serial: 0,
            pad_shortcut_baseline: 0,
            last_input: None,
            input_chord: vec![],
            ui_navigation: false,
            navigation_serial: 0,
            navigation_events: vec![],
            navigation_held: vec![],
            stats: Stats::default(),
        }
    }
}

/// Latest physical chord with stick directions replaced by native ring inputs.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PadRegionCapture {
    pub serial: u64,
    pub chord: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Hello {
        protocol: u32,
        version: String,
    },
    Reply {
        id: u64,
        ok: bool,
        error: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        measurement: Option<crate::motion::GameAngleMeasurement>,
    },
    Status {
        status: Box<Status>,
    },
    Devices {
        devices: Vec<Device>,
    },
    Calibration {
        device_path: String,
        bias: [f32; 3],
        sensor: crate::motion_config::SensorCalibration,
    },
    Log {
        message: String,
    },
}
pub fn read_packet<T: DeserializeOwned>(reader: &mut impl BufRead) -> Result<Option<T>> {
    let mut bytes = Vec::new();
    let n = reader
        .take((MAX_MESSAGE + 1) as u64)
        .read_until(b'\n', &mut bytes)?;
    if n == 0 {
        return Ok(None);
    }
    if n > MAX_MESSAGE {
        bail!("IPC message exceeds size limit");
    }
    if bytes.last() != Some(&b'\n') {
        bail!("Incomplete IPC message");
    }
    Ok(Some(
        serde_json::from_slice(&bytes).context("Invalid IPC message")?,
    ))
}
