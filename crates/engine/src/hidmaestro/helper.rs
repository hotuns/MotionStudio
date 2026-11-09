use super::*;
use flexinput_hidmaestro::{claims, identity, orchestrator as nodes, OutputSection, Profile};
use std::collections::HashSet;
use transport::OwnerPipe;

fn xusb_ready(diag: &nodes::NodeDiag) -> bool {
    diag.started && diag.problem == 0 && diag.xusb_interfaces + diag.xusb_interfaces_child > 0
}

struct Device {
    index: u32,
    token: String,
    instance: Option<String>,
    input: InputSection,
    _output: Option<OutputSection>,
    neutral: Vec<u8>,
    gip: Option<[u8; 14]>,
    node_handle: Option<nodes::SwdHandle>,
    companion: Option<nodes::SwdHandle>,
    companion_instance: Option<String>,
    _owners: Vec<OwnerPipe>,
    _claim: claims::Claim,
}
impl Device {
    fn create(mut profile: Profile, device_id: &str, target: GamepadTarget) -> Result<Self> {
        if target == GamepadTarget::None {
            let name = if device_id == "motion-studio.keyboard" {
                "Motion Studio Virtual Keyboard"
            } else {
                "Motion Studio Virtual Mouse"
            };
            profile.name = name.into();
            profile.product_string = Some(name.into());
            profile.device_description = Some(name.into());
        }
        let used: HashSet<_> = nodes::list_hidmaestro_devices()
            .iter()
            .map(|d| d.index)
            .collect();
        let (index, claim) = (0..claims::INDEX_LIMIT)
            .filter(|i| !used.contains(i) && !claims::legacy_owner_holds(*i))
            .find_map(|i| claims::claim_index(i).ok().map(|c| (i, c)))
            .context("没有可用的 HIDMaestro 控制器索引")?;
        let token = identity::new_token();
        let owners = transport::owner_pipes(index)?;
        let input = InputSection::create(index, identity::tag(&token))?;
        let (neutral, gip) = if target == GamepadTarget::None {
            (vec![0; profile.input_report_size], None)
        } else {
            reports::pad_report(&profile, PadState::default())
        };
        let mut device = Self {
            index,
            token,
            instance: None,
            input,
            _output: OutputSection::create(index).ok(),
            neutral,
            gip,
            node_handle: None,
            companion: None,
            companion_instance: None,
            _owners: owners,
            _claim: claim,
        };
        claims::add_controller(&device.token, index, device_id);
        identity::write_entry(&device.token, index)
            .map_err(|e| anyhow::anyhow!("HIDMaestro identity: {e}"))?;
        device
            .input
            .write_frame(&device.neutral, device.gip.as_ref());
        let mut ack = DeliveryAck::open(index)?;
        // Release frames contain the complete HID report, including a nonzero RID.
        let mut release = device.neutral.clone();
        if profile.report.report_id != 0 {
            release.insert(0, profile.report.report_id);
        }
        ack.arm_release(&[release]);
        if profile.own_container {
            let (created, handle, _) =
                nodes::create_own_container_node(&profile, index, device_id, &device.token)?;
            device.instance = Some(created.instance_id);
            device.node_handle = Some(handle);
        } else {
            let inf = install::installed_inf_path().context("Missing HIDMaestro INF")?;
            let created = nodes::create_device_node(
                &profile,
                &inf.display().to_string(),
                index,
                device_id,
                &device.token,
            )?;
            device.instance = Some(created.instance_id);
        }
        claims::set_controller_instance(&device.token, device.instance.as_ref().unwrap());
        if profile.requires_xusb_companion {
            let inf = install::installed_xusb_inf_path().context("Missing XInput companion INF")?;
            let (created, handle) = nodes::create_xusb_companion_node(
                &profile,
                &inf.display().to_string(),
                index,
                &device.token,
            )?;
            device.companion = Some(handle);
            device.companion_instance = Some(created);
        }
        let deadline = Instant::now() + Duration::from_secs(6);
        loop {
            if let Some(h) = device.input.driver_half() {
                if h.driver_config_error != 0 || h.companion_config_error != 0 {
                    bail!(
                        "HIDMaestro 配置失败：HID {} / XInput {}",
                        h.driver_config_error,
                        h.companion_config_error
                    );
                }
                if (4..=5).contains(&h.driver_protocol)
                    && (!profile.requires_xusb_companion || h.companion_protocol == 4)
                    && ack.release_on_exit_supported()
                {
                    break;
                }
            }
            if Instant::now() >= deadline {
                bail!("HIDMaestro 驱动未完成版本握手或输出释放保护未就绪");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        device.verify_xusb()?;
        Ok(device)
    }
    fn verify_xusb(&self) -> Result<()> {
        if let Some(instance) = &self.companion_instance {
            let diag = nodes::node_diag(instance);
            anyhow::ensure!(xusb_ready(&diag),
                "虚拟 Xbox 的 XInput 接口未就绪：started={}，problem={}，本设备接口={}，子设备接口={}；请检查 HIDMaestro XInput 组件",
                diag.started,diag.problem,diag.xusb_interfaces,diag.xusb_interfaces_child);
        }
        Ok(())
    }
}
impl Drop for Device {
    fn drop(&mut self) {
        // Reopen to pick up the worker's latest sequence before becoming writer.
        if let Ok(mut input) = InputSection::open(self.index) {
            input.write_frame(&self.neutral, self.gip.as_ref());
            if let Ok(ack) = DeliveryAck::open(self.index) {
                let _ = ack.wait_for(&input, Duration::from_millis(50));
            }
        }
        self.companion.take();
        if self.node_handle.take().is_none() {
            if let Some(instance) = &self.instance {
                let _ = nodes::remove_device_node(instance);
            }
        }
        identity::delete_entry(&self.token);
        claims::remove_controller(&self.token);
    }
}
struct OutputDevices {
    keyboard: Device,
    mouse: Device,
    pad: Option<Device>,
}
impl OutputDevices {
    fn info(&self) -> Devices {
        Devices {
            keyboard: self.keyboard.index,
            mouse: self.mouse.index,
            pad: self.pad.as_ref().map(|p| p.index),
            protected: [&self.keyboard, &self.mouse]
                .into_iter()
                .chain(self.pad.iter())
                .flat_map(|d| {
                    d.instance
                        .iter()
                        .chain(d.companion_instance.iter())
                        .cloned()
                })
                .collect(),
        }
    }
    fn configure(&mut self, target: GamepadTarget) -> Result<()> {
        self.pad.take();
        if let Some(profile) = reports::profile(target) {
            self.pad = Some(Device::create(profile, "motion-studio.gamepad", target)?);
        }
        Ok(())
    }
}
pub(super) fn run(parent_pid: u32, token: &str, installing: bool) -> Result<()> {
    let parent = Process::open(parent_pid)?;
    let mut pipe = Pipe::connect(token, parent_pid)?;
    if !matches!(
        pipe.receive::<Control>(&parent, Duration::from_secs(5))?,
        Control::Begin {}
    ) {
        bail!("Invalid output handshake");
    }
    let result = (|| -> Result<()> {
        if installing {
            // This is the only driver-install entry. It never replaces an
            // installed version or sweeps devices owned by another application.
            match install::driver_state() {
                DriverState::Missing => {
                    flexinput_hidmaestro::deploy::ensure_driver_installed()?;
                }
                DriverState::Complete => {
                    check_versions()?;
                }
                DriverState::Partial { .. } => {
                    probe()?;
                }
            }
            probe()?;
            pipe.send(&Reply::Ok)?;
            return Ok(());
        }
        probe()?;
        let _output_lease = transport::OutputLease::acquire()?;
        claims::take_owner()
            .map_err(|e| anyhow::anyhow!("HIDMaestro ownership claim failed: {e}"))?;
        // Only MS identity tokens are ours. A previous killed helper can leave
        // ROOT gamepad nodes; an index claim is required before touching one.
        for d in nodes::list_hidmaestro_devices()
            .into_iter()
            .filter(|d| identity::is_ours(identity::token_of(&d.instance_id)))
        {
            if claims::legacy_owner_holds(d.index) {
                continue;
            }
            if let Ok(_claim) = claims::claim_index(d.index) {
                let _ = nodes::remove_device_node(&d.instance_id);
                identity::delete_entry(identity::token_of(&d.instance_id));
            }
        }
        let keyboard = Device::create(
            Profile::from_json(flexinput_hidmaestro::profile::presets::KEYBOARD_JSON)?,
            "motion-studio.keyboard",
            GamepadTarget::None,
        )?;
        let mouse = Device::create(
            Profile::from_json(flexinput_hidmaestro::profile::presets::MOUSE_JSON)?,
            "motion-studio.mouse",
            GamepadTarget::None,
        )?;
        let mut devices = OutputDevices {
            keyboard,
            mouse,
            pad: None,
        };
        pipe.send(&Reply::Ready {
            devices: devices.info(),
        })?;
        let mut last = Instant::now();
        loop {
            if !parent.alive() || last.elapsed() >= LEASE_TIMEOUT {
                break;
            }
            match pipe.poll::<Control>()? {
                Some(Control::Heartbeat {}) => {
                    if let Some(pad) = &devices.pad {
                        pad.verify_xusb()?;
                    }
                    pipe.send(&Reply::Ok)?;
                    last = Instant::now();
                }
                Some(Control::Configure { target }) => {
                    devices.configure(target)?;
                    pipe.send(&Reply::Ready {
                        devices: devices.info(),
                    })?;
                    last = Instant::now();
                }
                Some(Control::Shutdown {}) => break,
                Some(Control::Begin {}) => bail!("Duplicate output handshake"),
                None => std::thread::sleep(Duration::from_millis(10)),
            }
        }
        // devices drop before the owner claim/record is relinquished.
        drop(devices);
        Ok(())
    })();
    claims::delete_own_record();
    if let Err(error) = &result {
        let _ = pipe.send(&Reply::Error {
            message: format!("{error:#}"),
        });
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unrelated_xinput_interfaces_never_prove_our_companion_ready() {
        let mut diag = nodes::NodeDiag {
            status: Some(0),
            problem: 0,
            started: true,
            xusb_interfaces: 0,
            xusb_interfaces_child: 0,
            xusb_interfaces_global: 4,
            driver_inf: None,
        };
        assert!(!xusb_ready(&diag));
        diag.xusb_interfaces_child = 1;
        assert!(xusb_ready(&diag));
        diag.problem = 10;
        assert!(!xusb_ready(&diag));
        diag.problem = 0;
        diag.started = false;
        assert!(!xusb_ready(&diag));
    }
}
