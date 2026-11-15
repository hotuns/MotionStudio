//! Studio lifecycle adapter over the pinned FlexInput composite USB backend.
use crate::{
    engine::PadState,
    feedback::FeedbackFrame,
    output::{GamepadBackend, GamepadStatus, Sink},
    profile::GamepadTarget,
};
use anyhow::{ensure, Result};
use flexinput_virtual::{composite_device::CompositeDevice, VirtualDevice};

const KEY: &str = "motion-studio.dualsense";
pub fn probe() -> Result<()> {
    ensure!(
        std::env::var_os("MOTION_STUDIO_DISABLE_PHYSICAL").is_none()
            && std::env::var("SDL_JOYSTICK_HIDAPI").as_deref() != Ok("0"),
        "Native output is disabled for this session"
    );
    ensure!(flexinput_usbip::vhci::is_available(), "DualSense audio output requires the usbip-win2 virtual USB host controller. Install it before starting this profile.");
    Ok(())
}
pub struct DualSenseSink {
    device: CompositeDevice,
    protected: Vec<String>,
}
impl DualSenseSink {
    pub fn new(pcm: Option<flexinput_usbip::PcmSink>) -> Result<Self> {
        probe()?;
        let device = CompositeDevice::attach_existing(
            KEY,
            flexinput_usbip::profiles::DUALSENSE_COMPOSITE_JSON,
            pcm,
        )
        .map_err(anyhow::Error::msg)?;
        let protected = crate::visibility::wait_for_composite(&flexinput_usbip::serial_for(KEY))?;
        ensure!(
            device.host_connected(),
            "Virtual DualSense USB connection was lost during startup"
        );
        Ok(Self { device, protected })
    }
}
impl Sink for DualSenseSink {
    fn feedback(&mut self) -> Option<FeedbackFrame> {
        Some(FeedbackFrame::read(&mut self.device))
    }
    fn poll(&mut self) -> Result<()> {
        ensure!(
            self.device.host_connected(),
            "Virtual DualSense USB connection lost"
        );
        Ok(())
    }
    fn gamepad_status(&self) -> Option<GamepadStatus> {
        Some(GamepadStatus {
            target: GamepadTarget::DualSense,
            backend: GamepadBackend::UsbIp,
            connected: self.device.host_connected(),
            player_index: None,
            protected_devices: self.protected.clone(),
        })
    }
    fn protected_devices(&self) -> Vec<String> {
        self.protected.clone()
    }
    fn gamepad(&mut self, pad: PadState) -> Result<()> {
        self.poll()?;
        crate::flex_output::write_pad(&mut self.device, pad);
        self.device.flush();
        Ok(())
    }
    fn idle_gamepad(&mut self) -> Result<()> {
        self.poll()?;
        self.device.flush();
        Ok(())
    }
    fn button(&mut self, _: &str, _: bool) -> Result<()> {
        anyhow::bail!("DualSense requires a keyboard/mouse companion sink")
    }
    fn mouse(&mut self, _: i32, _: i32) -> Result<()> {
        anyhow::bail!("DualSense requires a keyboard/mouse companion sink")
    }
}
impl Drop for DualSenseSink {
    fn drop(&mut self) {
        self.device.reset_outputs();
    }
}
