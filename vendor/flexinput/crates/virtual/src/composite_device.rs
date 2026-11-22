//! `VirtualDevice` backend for the composite USB personas: a virtual DualSense
//! (or Edge) with its USB audio interfaces beside the HID one, served by
//! `flexinput-usbip` and attached through a USB/IP host controller:
//! usbip-win2's on Windows, the kernel's vhci-hcd on Linux. To a game it is a
//! wired pad with a speaker / haptics output and a microphone, so the haptic
//! waveforms it authors stream to us.
//!
//! The HID side is the plain profile's: the same encoder builds the input
//! report and the same decode reads the game's output report. What differs is
//! the transport — an in-process USB/IP server instead of HIDMaestro's UMDF
//! driver and shared memory (Windows) or UHID (Linux) — and that nothing
//! outlives the app: the device is served from this process, so it can't
//! persist past it.

use std::sync::{Arc, Mutex};

use flexinput_core::haptic_pcm::HapticConverter;
use flexinput_core::Signal;
use flexinput_hidmaestro::encode::{encode_report_into, PinState};
use flexinput_hidmaestro::Profile;
use flexinput_usbip::{vhci, EmulatedDevice, OutputKind};

use crate::apply_pin;
use crate::sony_feedback::SonyFeedback;
use crate::{layouts, SinkPin, SourcePin};

/// The DualSense's USB output report.
const OUTPUT_REPORT_ID: u8 = 0x02;

#[derive(Default)]
struct SensorStream {
    ticks: u32,
    sequence: u8,
    last_ns: Option<u64>,
    acceleration: glam::Vec3,
    active: bool,
    clear_pending: bool,
    dropped: u64,
}

pub struct CompositeDevice {
    id: String,
    display_name: String,
    profile: Profile,
    pins: PinState,
    report_buf: Vec<u8>,
    usb: Option<EmulatedDevice>,
    /// The host controller port the device is attached on.
    vport: Option<i32>,
    rumble: (f32, f32),
    sony: SonyFeedback,
    /// The haptic waveform the game streams (output channels 3/4), measured
    /// into per-side amplitude and frequency on the pacing thread.
    haptics: Arc<Mutex<HapticConverter>>,
    feedback_sequence: u64,
    sensors: Option<SensorStream>,
}

impl CompositeDevice {
    /// Studio owns setup and teardown. Only attach through an already-installed
    /// host controller; do not install drivers or sweep other sessions.
    pub fn attach_existing(id: &str, profile_json: &str, pcm: Option<flexinput_usbip::PcmSink>) -> Result<Self, String> {
        if !vhci::is_available() {
            return Err("虚拟 DualSense 需要 usbip-win2 虚拟 USB 主控制器，请在「设置与组件」检查组件。".into());
        }
        let mut dev = Self::detached(id, "DualSense", profile_json).ok_or("Invalid DualSense profile")?;
        let usb = EmulatedDevice::new(profile_json, id, 0).map_err(|e| e.to_string())?;
        usb.set_pcm_sink(pcm);
        encode_report_into(&dev.profile, &dev.pins.state(), &mut dev.report_buf);
        usb.submit_input(&dev.report_buf);
        let port = vhci::attach(flexinput_usbip::HOST, usb.port(), usb.busid()).map_err(|e| e.to_string())?;
        dev.vport = Some(port);
        dev.usb = Some(usb);
        Ok(dev)
    }

    pub fn host_connected(&self) -> bool {
        self.usb.as_ref().is_some_and(EmulatedDevice::is_connected)
    }

    /// Stage one calibrated sample in FlexInput's canonical axes. The USB
    /// persona's 0x05 calibration advertises 16 counts/(deg/s) and 8192/g.
    pub fn set_imu_sample(&mut self, timestamp_ns: u64, gyro_dps: [f32; 3], accel_g: [f32; 3]) {
        let stream = self.sensors.get_or_insert_with(SensorStream::default);
        // DualSense's 32-bit sensor clock runs at 3 MHz. Quantize the absolute
        // endpoints so fractional ticks are recovered over consecutive samples.
        let ticks = |ns: u64| u128::from(ns) * 3 / 1000;
        let step = stream.last_ns
            .filter(|&last| timestamp_ns.checked_sub(last).is_some_and(|dt| dt <= 50_000_000))
            .map_or(1, |last| (ticks(timestamp_ns) - ticks(last)).max(1) as u32);
        stream.ticks = stream.ticks.wrapping_add(step);
        stream.last_ns = Some(timestamp_ns);
        stream.acceleration = glam::Vec3::from_array(accel_g);
        stream.active = true;
        self.write_imu_pins(gyro_dps, accel_g);
    }

    /// Prime a stream or clear live motion before a pause/target handoff.
    pub fn clear_imu(&mut self, active: bool) {
        if !active && self.sensors.is_none() { return; }
        let stream = self.sensors.get_or_insert_with(SensorStream::default);
        stream.ticks = stream.ticks.wrapping_add(1);
        stream.last_ns = None;
        stream.active = active;
        stream.clear_pending = true;
        let gravity = stream.acceleration.try_normalize().unwrap_or(glam::Vec3::Z);
        self.write_imu_pins([0.0; 3], gravity.to_array());
    }

    fn write_imu_pins(&mut self, gyro_dps: [f32; 3], accel_g: [f32; 3]) {
        // PinState performs the Sony axis/sign conversion, and encode_report_into
        // scales normalized pins to signed 16-bit values.
        for (name, value) in ["gyro_x", "gyro_y", "gyro_z"].into_iter().zip(gyro_dps) {
            self.pins.set(name, value * (16.0 / 32767.0), false);
        }
        for (name, value) in ["accel_x", "accel_y", "accel_z"].into_iter().zip(accel_g) {
            self.pins.set(name, value * (8192.0 / 32767.0), false);
        }
    }

    fn encode_input(&mut self) {
        encode_report_into(&self.profile, &self.pins.state(), &mut self.report_buf);
        if let Some(stream) = &mut self.sensors {
            let layout = &self.profile.extended;
            if let Some(byte) = layout.sequence_num.and_then(|i| self.report_buf.get_mut(i)) {
                *byte = stream.sequence;
            }
            if let Some(field) = layout.sensor_timestamp_u32
                .and_then(|i| self.report_buf.get_mut(i..))
                .and_then(|tail| tail.first_chunk_mut::<4>())
            {
                *field = stream.ticks.to_le_bytes();
            }
            stream.sequence = stream.sequence.wrapping_add(1);
        }
    }

    pub fn sensor_queue_drops(&self) -> Option<u64> {
        self.sensors.as_ref().map(|stream| stream.dropped)
    }

    /// A render/output tick alone does not create another physical sensor sample.
    pub fn flush_idle(&mut self) {
        if !self.sensors.as_ref().is_some_and(|stream| stream.active) {
            crate::VirtualDevice::flush(self);
        }
    }

    /// Build the persona `profile_json` describes and attach it, setting the
    /// host controller up first if it isn't (see [`ensure_host_controller`]).
    /// Blocking: call from the device-ops worker. `None` only for an invalid
    /// bundled profile; a failed attach returns a disconnected device, with
    /// the reason logged.
    pub fn create(
        id: impl Into<String>,
        display_name: impl Into<String>,
        profile_json: &str,
        index: u8,
    ) -> Option<Self> {
        let mut dev = Self::detached(id, display_name, profile_json)?;
        if let Err(e) = dev.connect(profile_json, index) {
            eprintln!("[usbip] {} not attached: {e}", dev.id);
        }
        Some(dev)
    }

    /// The device, not yet served or attached.
    fn detached(id: impl Into<String>, display_name: impl Into<String>, profile_json: &str) -> Option<Self> {
        let profile = Profile::from_json(profile_json).ok()?;
        Some(CompositeDevice {
            id: id.into(),
            display_name: display_name.into(),
            report_buf: vec![0u8; profile.report.byte_size()],
            profile,
            pins: PinState::new(),
            usb: None,
            vport: None,
            rumble: (0.0, 0.0),
            sony: SonyFeedback::default(),
            haptics: Arc::new(Mutex::new(HapticConverter::new())),
            feedback_sequence: 0,
            sensors: None,
        })
    }

    fn connect(&mut self, profile_json: &str, index: u8) -> Result<(), String> {
        ensure_host_controller()?;
        // Before the server exports anything, or the driver's re-attach loop
        // for a dead session's device could import this one first.
        // No global sweep in Studio: an attachment may belong to another app.
        let usb = EmulatedDevice::new(profile_json, &self.id, index).map_err(|e| e.to_string())?;
        let haptics = Arc::clone(&self.haptics);
        usb.set_pcm_sink(Some(Box::new(move |pcm: &[u8]| haptics.lock().unwrap().push_pcm(pcm))));
        // A neutral report waits for the host's first read.
        encode_report_into(&self.profile, &self.pins.state(), &mut self.report_buf);
        usb.submit_input(&self.report_buf);
        let vport = vhci::attach(flexinput_usbip::HOST, usb.port(), usb.busid()).map_err(|e| e.to_string())?;
        self.usb = Some(usb);
        self.vport = Some(vport);
        Ok(())
    }

    /// The HID-side pins: the Edge adds its back and Fn buttons.
    fn is_edge(&self) -> bool {
        self.profile.id.contains("dualsense-edge")
    }
}

/// A host controller this process can attach to. Windows: usbip-win2,
/// installed through the elevated helper when missing (one UAC, and the USB
/// root hubs re-enumerate once). Linux: the kernel's vhci-hcd, loaded and
/// handed to the user through pkexec the first time (see `linux::access`).
fn ensure_host_controller() -> Result<(), String> {
    #[cfg(windows)]
    if !vhci::is_available() {
        return Err("usbip-win2 host controller is not installed".into());
    }
    #[cfg(target_os = "linux")]
    crate::linux::access::ensure_vhci()?;
    Ok(())
}

impl crate::VirtualDevice for CompositeDevice {
    fn id(&self) -> &str {
        &self.id
    }

    fn display_name(&self) -> &str {
        &self.display_name
    }

    fn sink_pins(&self) -> &'static [SinkPin] {
        if self.is_edge() { layouts::DUALSENSE_EDGE_SINK_PINS } else { layouts::DUALSENSE_SINK_PINS }
    }

    fn send(&mut self, pin: &str, value: Signal) {
        apply_pin(&mut self.pins, pin, value);
    }

    fn flush(&mut self) {
        self.encode_input();
        if let Some(usb) = &self.usb {
            if let Some(stream) = self.sensors.as_mut().filter(|s| s.active || s.clear_pending) {
                let lost = usb.submit_sensor_input(&self.report_buf, stream.clear_pending);
                stream.dropped = stream.dropped.saturating_add(lost as u64);
                stream.clear_pending = false;
            } else {
                usb.submit_input(&self.report_buf);
            }
        }
    }

    fn reset_outputs(&mut self) {
        self.pins.reset();
        self.clear_imu(false);
        self.flush();
    }

    fn is_connected(&self) -> bool {
        self.vport.is_some()
    }

    fn source_pins(&self) -> &'static [SourcePin] {
        layouts::DUALSENSE_USB_SOURCE_PINS
    }

    fn poll_outputs(&mut self) -> Vec<(&'static str, Signal)> {
        let frames: Vec<Vec<u8>> = self
            .usb
            .as_ref()
            .map(|u| u.take_outputs())
            .unwrap_or_default()
            .into_iter()
            .filter(|f| f.kind == OutputKind::Output && f.report_id == OUTPUT_REPORT_ID)
            .map(|f| f.data)
            .collect();
        self.feedback_sequence = self.feedback_sequence.wrapping_add(frames.len() as u64);
        let mut out = self.sony.poll(&self.profile, &mut self.rumble, frames.iter().map(Vec::as_slice));
        let hd = self.haptics.lock().unwrap().params();
        out.extend([
            ("hd_l_amp", Signal::Float(hd.l_amp)),
            ("hd_l_freq", Signal::Float(hd.l_freq)),
            ("hd_r_amp", Signal::Float(hd.r_amp)),
            ("hd_r_freq", Signal::Float(hd.r_freq)),
        ]);
        out
    }

    fn inject_rumble_ping(&mut self, strong: f32, weak: f32) {
        self.rumble = (strong, weak);
    }
    fn feedback_sequence(&self) -> u64 { self.feedback_sequence }
}

impl Drop for CompositeDevice {
    fn drop(&mut self) {
        // Plug out first: the driver closes the socket before it unplugs the
        // device, so the server sees the connection end cleanly.
        if let Some(p) = self.vport.take() {
            if let Err(e) = vhci::detach(p) {
                eprintln!("[usbip] detach of {} failed: {e}", self.id);
            }
        }
        self.usb = None;
    }
}

#[cfg(test)]
mod sensor_tests {
    use super::*;
    use crate::VirtualDevice;
    use flexinput_usbip::profiles::DUALSENSE_COMPOSITE_JSON;

    fn axes(report: &[u8]) -> [i16; 6] {
        std::array::from_fn(|i| i16::from_le_bytes([report[16 + 2*i], report[17 + 2*i]]))
    }

    fn timestamp(report: &[u8]) -> u32 {
        u32::from_le_bytes(report[28..32].try_into().unwrap())
    }

    // A consumer reads the actual USB/IP device over loopback. No host
    // controller, physical controller or driver attachment is involved.
    #[test]
    fn usb_host_reads_calibration_ordered_samples_and_the_pause_report() {
        use flexinput_usbip::protocol::*;
        use std::io::{Read, Write};
        use std::net::TcpStream;
        use std::time::Duration;

        let mut dev = CompositeDevice::detached("sensor-wire", "PS5", DUALSENSE_COMPOSITE_JSON).unwrap();
        let usb = EmulatedDevice::new(DUALSENSE_COMPOSITE_JSON, "sensor-wire", 0).unwrap();
        let mut host = TcpStream::connect((flexinput_usbip::HOST, usb.port())).unwrap();
        host.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let mut import = [0; 8 + BUS_ID_SIZE];
        write_op_common(&mut import, OP_REQ_IMPORT, 0);
        import[8..8 + usb.busid().len()].copy_from_slice(usb.busid().as_bytes());
        host.write_all(&import).unwrap();
        let mut imported = [0; 8 + USB_DEVICE_SIZE];
        host.read_exact(&mut imported).unwrap();
        assert_eq!(&imported[4..8], &ST_OK.to_be_bytes());
        dev.usb = Some(usb);

        let mut read = |seqnum, ep, setup, size| {
            let cmd = Command { command: CMD_SUBMIT, seqnum, direction: 1, ep, setup,
                transfer_buffer_length: size, number_of_packets: NON_ISOCH, ..Default::default() };
            host.write_all(&cmd.to_bytes()).unwrap();
            let mut header = [0; HEADER_SIZE];
            host.read_exact(&mut header).unwrap();
            assert_eq!(&header[20..24], &0_i32.to_be_bytes());
            assert_eq!(&header[24..28], &size.to_be_bytes());
            let mut payload = vec![0; size as usize];
            host.read_exact(&mut payload).unwrap();
            payload
        };
        let calibration = read(1, 0, [0xA1, 1, 5, 3, 3, 0, 41, 0], 41);
        let field = |i: usize| i16::from_le_bytes([calibration[i], calibration[i+1]]) as f32;
        let gyro_per_dps = (field(7) - field(9)) / (field(19) + field(21));
        let accel_per_g = (field(23) - field(25)) / 2.0;
        dev.send("btn_south", Signal::Bool(true));
        for (ns, pitch) in [(1_000_000, 45.), (3_000_000, -90.)] {
            dev.set_imu_sample(ns, [0., pitch, 0.], [0., 0., 1.]);
            dev.flush();
            dev.flush_idle();
        }
        let first = read(2, 4, [0; 8], 64);
        let second = read(3, 4, [0; 8], 64);
        assert_eq!(first[0], 1);
        assert_eq!(first[8] & 0x20, 0x20);
        assert_eq!(f32::from(axes(&first)[0]) / gyro_per_dps, 45.);
        assert_eq!(f32::from(axes(&second)[0]) / gyro_per_dps, -90.);
        assert_eq!(f32::from(axes(&second)[4]) / accel_per_g, 1.);
        assert_eq!(timestamp(&second).wrapping_sub(timestamp(&first)), 6000);
        dev.set_imu_sample(4_000_000, [0., 180., 0.], [0., 0., 1.]);
        dev.flush();
        dev.clear_imu(true);
        dev.flush();
        assert_eq!(axes(&read(4, 4, [0; 8], 64)), [0, 0, 0, 0, 8192, 0]);
    }

    #[test]
    fn input_report_carries_calibrated_six_axes_and_sample_time_with_gamepad_controls() {
        let mut dev = CompositeDevice::detached("sensor-test", "PS5", DUALSENSE_COMPOSITE_JSON).unwrap();
        dev.send("btn_south", Signal::Bool(true));
        dev.set_imu_sample(1_000_250, [10., 20., -30.], [0.25, 0.5, 1.]);
        dev.flush();
        let first = dev.report_buf.clone();
        assert_eq!((first.len(), first[0], first[8] & 0x20), (64, 1, 0x20));
        assert_eq!(axes(&first), [320, 480, -160, -4096, 8192, -2048]);
        dev.flush_idle();
        assert_eq!(dev.report_buf, first, "idle must not replay a moving sample");
        dev.send("btn_south", Signal::Bool(false));
        dev.flush();
        assert_eq!(dev.report_buf[8] & 0x20, 0);
        assert_eq!(timestamp(&dev.report_buf), timestamp(&first), "a button edge has no sensor delta");
        dev.set_imu_sample(2_000_750, [-10., -20., 30.], [0., 0., 1.]);
        dev.flush();
        assert_eq!(timestamp(&dev.report_buf) - timestamp(&first), 3002);
        assert_eq!(dev.report_buf[7], first[7].wrapping_add(2));
        assert_eq!(axes(&dev.report_buf), [-320, -480, 160, 0, 8192, 0]);
    }

    #[test]
    fn clear_keeps_gravity_and_resumes_with_a_fresh_wrapping_clock() {
        let mut dev = CompositeDevice::detached("sensor-test", "PS5", DUALSENSE_COMPOSITE_JSON).unwrap();
        dev.clear_imu(true);
        dev.flush();
        assert_eq!(axes(&dev.report_buf), [0, 0, 0, 0, 8192, 0]);
        dev.set_imu_sample(900_000_000, [12., 1., 0.], [0., -2., 0.]);
        dev.sensors.as_mut().unwrap().ticks = u32::MAX - 2;
        dev.set_imu_sample(900_001_000, [12., 1., 0.], [0., -2., 0.]);
        dev.flush();
        assert_eq!(timestamp(&dev.report_buf), 0);
        dev.reset_outputs();
        assert_eq!(axes(&dev.report_buf), [0, 0, 0, 8192, 0, 0]);
        let paused = timestamp(&dev.report_buf);
        dev.set_imu_sample(1_000, [0., 1., 0.], [0., 0., 1.]);
        dev.flush();
        assert_eq!(timestamp(&dev.report_buf), paused+1, "reconnect can restart the physical clock");
    }
}

#[cfg(all(test, feature = "upstream-output-tests"))]
mod tests {
    use super::*;
    use crate::VirtualDevice;
    use flexinput_usbip::profiles::{DUALSENSE_COMPOSITE_JSON, DUALSENSE_EDGE_COMPOSITE_JSON};

    // A composite pad carries the plain pad's pins, so switching a node between
    // the two keeps every wire.
    #[test]
    fn composite_pins_match_the_plain_pads() {
        for (plain, usb) in crate::USB_AUDIO_VARIANTS {
            let (ps, po, pn) = crate::kind_pin_metadata(plain, 0).unwrap();
            let (us, uo, un) = crate::kind_pin_metadata(usb, 0).unwrap();
            let ids = |p: &[SinkPin]| p.iter().map(|p| p.id).collect::<Vec<_>>();
            let src = |p: &[SourcePin]| p.iter().map(|p| p.id).collect::<Vec<_>>();
            assert_eq!(ids(ps), ids(us), "{usb} sink pins");
            assert_eq!(src(uo)[..po.len()], src(po)[..], "{usb} keeps the plain pad's source pins in order");
            assert_eq!(pn, un, "same display name");
        }
        let edge = CompositeDevice::detached("virtual.usb.dualsense_edge", "E", DUALSENSE_EDGE_COMPOSITE_JSON).unwrap();
        let ds = CompositeDevice::detached("virtual.usb.dualsense", "D", DUALSENSE_COMPOSITE_JSON).unwrap();
        assert!(edge.sink_pins().iter().any(|p| p.id == "btn_paddle_r1"));
        assert!(!ds.sink_pins().iter().any(|p| p.id == "btn_paddle_r1"));
    }

    #[test]
    fn a_detached_device_is_disconnected_and_quiet() {
        let mut d = CompositeDevice::detached("virtual.usb.dualsense", "D", DUALSENSE_COMPOSITE_JSON).unwrap();
        assert!(!d.is_connected());
        d.send("btn_south", Signal::Bool(true));
        d.flush(); // no server: a no-op, not a panic
        let out = d.poll_outputs();
        assert!(out.contains(&("rumble_strong", Signal::Float(0.0))));
        assert_eq!(d.report_buf.len(), 64, "RID-inclusive USB report");
    }

    // The haptic channels of the game's PCM come out as the HD carrier pins.
    #[test]
    fn haptic_pcm_becomes_hd_pins() {
        let mut d = CompositeDevice::detached("virtual.usb.dualsense", "D", DUALSENSE_COMPOSITE_JSON).unwrap();
        // 20 ms of a 160 Hz sine on the left actuator (channel 3) only.
        let mut pcm = Vec::new();
        for n in 0..960 {
            let s = ((n as f32 * 160.0 / 48_000.0) * std::f32::consts::TAU).sin();
            for ch in 0..4 {
                let v = if ch == 2 { (s * 20_000.0) as i16 } else { 0 };
                pcm.extend_from_slice(&v.to_le_bytes());
            }
        }
        d.haptics.lock().unwrap().push_pcm(&pcm);
        let out = d.poll_outputs();
        let get = |p: &str| out.iter().find(|(id, _)| *id == p).map(|(_, s)| s.as_float()).unwrap();
        assert!(get("hd_l_amp") > 0.3, "left actuator driven: {}", get("hd_l_amp"));
        assert!(get("hd_r_amp") < 0.01, "right actuator silent");
        let hz = 80.0 + get("hd_l_freq") * 420.0;
        assert!((hz - 160.0).abs() < 25.0, "carrier near 160 Hz, got {hz}");
    }
}
