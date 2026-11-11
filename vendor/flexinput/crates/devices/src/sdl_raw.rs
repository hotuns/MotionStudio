//! Event-oriented API over the upstream SdlBackend and OpenPad lifecycle.
use super::*;
use crate::raw::{RawDevice, RawEvent, SampleTime};

impl SdlBackend {
    pub fn for_events() -> Result<Self, String> {
        if !crate::raw::claim() {
            return Err("A FlexInput SDL event session is already active".into());
        }
        let mut backend = Self::new();
        backend.raw_claim = true;
        backend.raw_mode = true;
        backend.sdl_all_pads = true;
        crate::raw::select(None);
        if !backend.ensure_init() {
            return Err(sdl3::get_error().to_string());
        }
        Ok(backend)
    }

    /// Only used during discovery, never by the high-rate event drain.
    pub fn event_devices(&mut self) -> Result<Vec<RawDevice>, String> {
        self.sync_open_pads();
        let state = self.state.as_ref().ok_or("SDL unavailable")?;
        let ids = state
            .joystick_subsystem
            .joysticks()
            .map_err(|e| e.to_string())?;
        let mut devices = Vec::with_capacity(ids.len());
        for id in ids {
            let raw_id: u32 = id.into();
            let gp = state.pads.get(&id).map(|p| &p.gamepad);
            let gamepad = state.gamepad_subsystem.is_gamepad(id);
            let mut error = None;
            // Rejected virtual or failed pads remain visible for diagnostics.
            let probe = if gamepad && gp.is_none() {
                match state.gamepad_subsystem.open(id) {
                    Ok(g) => Some(g),
                    Err(e) => {
                        error = Some(e.to_string());
                        None
                    }
                }
            } else {
                None
            };
            let gp = gp.or(probe.as_ref());
            let str_of = |p: *const std::ffi::c_char| {
                if p.is_null() {
                    String::new()
                } else {
                    unsafe { std::ffi::CStr::from_ptr(p) }
                        .to_string_lossy()
                        .into_owned()
                }
            };
            let (name, path, vendor, product) = unsafe {
                use sdl3::sys::joystick::*;
                (
                    str_of(SDL_GetJoystickNameForID(id.into())),
                    str_of(SDL_GetJoystickPathForID(id.into())),
                    SDL_GetJoystickVendorForID(id.into()),
                    SDL_GetJoystickProductForID(id.into()),
                )
            };
            devices.push(RawDevice {
                id: raw_id,
                name,
                path,
                vendor,
                product,
                gamepad,
                gyro: gp.is_some_and(|g| unsafe { g.has_sensor(SensorType::Gyroscope) }),
                accelerometer: gp
                    .is_some_and(|g| unsafe { g.has_sensor(SensorType::Accelerometer) }),
                touchpads:gp.map_or(0,|g|g.touchpads_count()),
                rumble:gp.is_some_and(|g|unsafe{g.has_rumble()}),
                lightbar:gp.is_some_and(|g|unsafe{g.has_led()}),
                buttons: (0..26)
                    .filter(|i| {
                        Button::from_ll(sdl3::sys::gamepad::SDL_GamepadButton(*i))
                            .is_some_and(|b| gp.is_some_and(|g| g.has_button(b)))
                    })
                    .map(|i| i as u8)
                    .collect(),
                virtual_device: unsafe { sdl3::sys::joystick::SDL_IsJoystickVirtual(id.into()) },
                open_error: error,
            });
        }
        // Enumeration is a capability probe. Keep only an explicitly selected
        // handle, rather than holding every user's controller while idle.
        if let Some(state) = self.state.as_mut() {
            state.pads.retain(|id, _| self.raw_selected == Some(*id));
        }
        Ok(devices)
    }

    pub fn select_events(
        &mut self,
        id: u32,
        sensors: bool,
        require_gyro: bool,
    ) -> Result<(), String> {
        crate::raw::select(None);
        let selected = JoystickId::from(id);
        self.raw_selected = Some(selected);
        self.sync_open_pads();
        let state = self.state.as_mut().ok_or("SDL unavailable")?;
        state.pads.retain(|id, _| *id == selected);
        for pad in state.pads.values_mut() {
            let _ = pad.gamepad.sensor_set_enabled(SensorType::Gyroscope, false);
            let _ = pad
                .gamepad
                .sensor_set_enabled(SensorType::Accelerometer, false);
        }
        let pad = state
            .pads
            .get_mut(&selected)
            .ok_or("Selected gamepad is unavailable or is a virtual output")?;
        if require_gyro && !pad.has_gyro {
            return Err("Selected device has no gyro".into());
        }
        if sensors {
            if pad.has_gyro {
                let result = pad.gamepad.sensor_set_enabled(SensorType::Gyroscope, true);
                if require_gyro {
                    result.map_err(|e| e.to_string())?;
                }
            }
            if pad.has_accel {
                pad.gamepad
                    .sensor_set_enabled(SensorType::Accelerometer, true)
                    .map_err(|e| e.to_string())?;
            }
        }
        self.raw_selected = Some(selected);
        crate::raw::select(Some(id));
        Ok(())
    }

    /// Original feedback implementation, addressed only to the selected handle.
    pub fn event_feedback(&mut self, pins: &[(&str, Signal)]) {
        let Some(id)=self.raw_selected.and_then(|id|self.state.as_ref()?.pads.get(&id).map(|p|p.dev_id.clone())) else {return};
        for (pin,value) in pins { <Self as crate::DeviceBackend>::send(self,&id,pin,*value); }
        if let Some(pad)=self.raw_selected.and_then(|id|self.state.as_mut()?.pads.get_mut(&id)) {
            if let Some((r,g,b))=pad.led.due(Instant::now()) { let _=pad.gamepad.set_led(r,g,b); }
        }
    }
    pub fn event_stop_feedback(&mut self) {
        self.event_cancel_lightbar();
        self.event_feedback(&[("rumble_strong",Signal::Float(0.)),("rumble_weak",Signal::Float(0.))]);
    }
    pub fn event_cancel_lightbar(&mut self) {
        if let Some(pad)=self.raw_selected.and_then(|id|self.state.as_mut()?.pads.get_mut(&id)) { pad.led=LedOut::default(); }
    }

    pub fn update_events(&mut self) -> Result<(), String> {
        let state = self.state.as_mut().ok_or("SDL unavailable")?;
        state.gamepad_subsystem.update();
        // The SDL watcher has already translated these synchronously. Drain
        // its unused joystick queue without pumping windows/hotplug messages.
        unsafe {
            use sdl3::sys::events::*;
            SDL_FlushEvents(
                SDL_EVENT_JOYSTICK_AXIS_MOTION.0,
                SDL_EVENT_JOYSTICK_UPDATE_COMPLETE.0,
            );
        }
        if let Some(pad) = self.raw_selected.and_then(|id| state.pads.get_mut(&id)) {
            let g = &pad.gamepad;
            // Reuse the upstream frozen-stream recovery without feeding the
            // latest-value watchdog readings into the mapping engine.
            if g.sensor_enabled(SensorType::Gyroscope)
                || g.sensor_enabled(SensorType::Accelerometer)
            {
                let mut raw = [0.; 6];
                let _ = g.sensor_get_data(SensorType::Gyroscope, &mut raw[..3]);
                let _ = g.sensor_get_data(SensorType::Accelerometer, &mut raw[3..]);
                if pad.imu_watch.observe(raw, Instant::now()) {
                    for ty in [SensorType::Gyroscope, SensorType::Accelerometer] {
                        if g.sensor_enabled(ty) {
                            let _ = g.sensor_set_enabled(ty, false);
                            let _ = g.sensor_set_enabled(ty, true);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Drain one captured event without performing another SDL poll.
    pub fn take_raw_event(&self) -> Result<Option<RawEvent>, String> {
        crate::raw::take()
    }

    pub fn next_raw_event(&mut self) -> Result<Option<RawEvent>, String> {
        if let Some(event) = self.take_raw_event()? {
            return Ok(Some(event));
        }
        self.update_events()?;
        self.take_raw_event()
    }

    pub fn event_snapshot(&self) -> Vec<RawEvent> {
        let Some(pad) = self
            .raw_selected
            .and_then(|id| self.state.as_ref()?.pads.get(&id))
        else {
            return Vec::new();
        };
        let time = SampleTime {
            host_ns: Self::event_now_ns(),
            sensor_ns: 0,
        };
        let mut out = Vec::new();
        for index in 0..26 {
            if let Some(button) = Button::from_ll(sdl3::sys::gamepad::SDL_GamepadButton(index)) {
                out.push(RawEvent::Button {
                    time,
                    index: index as u8,
                    down: pad.gamepad.button(button),
                });
            }
        }
        for (index, axis) in [
            Axis::LeftX,
            Axis::LeftY,
            Axis::RightX,
            Axis::RightY,
            Axis::TriggerLeft,
            Axis::TriggerRight,
        ]
        .into_iter()
        .enumerate()
        {
            out.push(RawEvent::Axis {
                time,
                index: index as u8,
                value: pad.gamepad.axis(axis),
            });
        }
        for touchpad in 0..pad.num_touchpads.min(2) {
            for (finger,contact) in read_touchpad_fingers(self.raw_selected.unwrap(),i32::from(touchpad)).into_iter().enumerate() {
                let (x,y)=contact.unwrap_or((0.,0.));
                out.push(RawEvent::Touch{time,pad:touchpad as u8,finger:finger as u8,active:contact.is_some(),x,y});
            }
        }
        out
    }

    pub fn event_connected(&self) -> bool {
        self.raw_selected
            .and_then(|id| self.state.as_ref()?.pads.get(&id))
            .is_some_and(|p| p.gamepad.connected())
    }
    pub fn set_event_sensors(&mut self, enabled: bool, require_gyro: bool) -> Result<(), String> {
        let pad = self
            .raw_selected
            .and_then(|id| self.state.as_ref()?.pads.get(&id))
            .ok_or("No selected gamepad")?;
        for (has, ty, required) in [
            (pad.has_gyro, SensorType::Gyroscope, require_gyro),
            (pad.has_accel, SensorType::Accelerometer, true),
        ] {
            if has && pad.gamepad.sensor_enabled(ty) != enabled {
                let result = pad.gamepad.sensor_set_enabled(ty, enabled);
                if required {
                    result.map_err(|e| e.to_string())?;
                }
            }
        }
        Ok(())
    }
    pub fn event_sensors_enabled(&self) -> [bool; 2] {
        self.raw_selected
            .and_then(|id| self.state.as_ref()?.pads.get(&id))
            .map_or([false; 2], |p| {
                [
                    p.gamepad.sensor_enabled(SensorType::Gyroscope),
                    p.gamepad.sensor_enabled(SensorType::Accelerometer),
                ]
            })
    }
    pub fn event_now_ns() -> u64 {
        unsafe { sdl3::sys::timer::SDL_GetTicksNS() }
    }
    pub fn event_idle() {
        unsafe {
            sdl3::sys::timer::SDL_DelayPrecise(500_000);
        }
    }
}

impl Drop for SdlBackend {
    fn drop(&mut self) {
        if self.state.is_some() {
            unsafe {
                sdl3::sys::events::SDL_SetEventFilter(None, std::ptr::null_mut());
            }
            crate::raw::select(None);
            self.state.take();
        }
        if self.raw_claim {
            crate::raw::release();
        }
    }
}
