//! Original FlexInput output-device contract and pin layouts (c69f53ad, MIT).
//! Studio supplies the lifecycle-owned, fallible device implementation.
pub mod layouts;
pub mod keyboard;
use flexinput_core::{Signal, SignalType};
/// Original lib.rs helper: a signed Float is a number of wheel notches.
pub fn scroll_notches(value: Signal) -> i32 {
    match value {
        Signal::Float(n) => n.round() as i32,
        Signal::Bool(true) => 1,
        _ => 0,
    }
}
pub struct SinkPin {
    pub id: &'static str,
    pub display_name: &'static str,
    pub signal_type: SignalType,
}

pub struct SourcePin {
    pub id: &'static str,
    pub display_name: &'static str,
    pub signal_type: SignalType,
}

/// Static metadata about an available virtual device type (no connections made).
pub struct DeviceKind {
    pub kind_id: &'static str,
    pub display_name: &'static str,
    /// If false, only one instance may be active at a time.
    pub allows_multiple: bool,
}

/// A virtual output device that receives signals from the graph.
pub trait VirtualDevice: Send {
    fn id(&self) -> &str;
    fn display_name(&self) -> &str;
    /// Ordered input pin layout for the canvas sink node.
    fn sink_pins(&self) -> &'static [SinkPin];
    /// Accept one signal value destined for the named pin.
    fn send(&mut self, pin: &str, value: Signal);
    /// Commit the current state to the system (e.g. submit a HID report).
    fn flush(&mut self);
    /// Zero all outputs and flush — called every frame while the tab is bypassed
    /// so lingering inputs (drifting sticks, held keys) are released immediately.
    fn reset_outputs(&mut self) {}
    /// Returns false if the underlying system resource is unavailable
    /// (e.g. ViGEmBus not installed, enigo init failed).
    fn is_connected(&self) -> bool {
        true
    }
    /// Output pins this device exposes back into the graph (e.g. rumble from games).
    /// Empty by default — only devices with feedback signals implement this.
    fn source_pins(&self) -> &'static [SourcePin] {
        &[]
    }
    /// Poll latest output values from the OS/game (e.g. rumble motor speeds).
    /// Returns (pin_id, signal) pairs; called each I/O frame after flush().
    /// Headless lifecycle metadata: new game commands, even at the same level.
    fn feedback_sequence(&self) -> u64 { 0 }
    fn poll_outputs(&mut self) -> Vec<(&'static str, Signal)> {
        vec![]
    }

    /// Inject a synthetic rumble level so the NEXT `poll_outputs` reports it —
    /// used by the UI "ping" on an own-virtual pad to make its feedback flow
    /// through AutoMap to the mapped physical pad exactly as a game's rumble
    /// would. The caller controls duration (it sets the level each tick while
    /// the ping is active and `(0,0)` when it ends), so this sets the level
    /// directly without any auto-decay. Default: no-op (devices with no feedback
    /// don't ping). Only the HIDMaestro companion (XInput) path implements it.
    fn inject_rumble_ping(&mut self, _strong: f32, _weak: f32) {}

    /// Relinquish OS ownership so this device's `Drop` does NOT tear the
    /// underlying virtual node down — the node is intentionally left alive past
    /// app exit for reclaim on next launch. Called on a clean shutdown when the
    /// user enabled "keep virtual controllers alive".
    ///
    /// Default: no-op. Only HIDMaestro devices override it (they own a
    /// helper-created PnP node that survives the helper process). ViGEmBus
    /// targets (XInput/DS4) cannot persist — ViGEmBus auto-removes them when the
    /// creating process exits — so they ignore this and are recreated next run.
    fn persist_on_drop(&mut self) {}
}

pub mod pins;
pub use pins::apply_pin;
#[cfg(windows)]
mod sony_feedback;
#[cfg(windows)]
pub mod hidmaestro_device;
#[cfg(windows)]
pub mod native_gamepads;
