//! Original FlexInput ViGEm gamepad classes and helpers, c69f53ad (MIT).
//! Original gamepad section, with checked flush and player-index API exposure.
//! Keyboard/Enigo stays outside this entry; see docs/flexinput-reuse.md.
use std::sync::{Arc,Mutex};
use std::os::windows::io::AsRawHandle;
use vigem_client::{Client,XButtons,XGamepad,Xbox360Wired};
use flexinput_core::Signal;
use crate::{layouts,SinkPin,SourcePin,VirtualDevice};
fn instance_label(base_id: &str, base_name: &str, instance: usize) -> (String, String) {
    if instance == 0 {
        (base_id.to_string(), base_name.to_string())
    } else {
        (format!("{base_id}.{instance}"), format!("{base_name} #{}", instance + 1))
    }
}

// ── XInput ────────────────────────────────────────────────────────────────────

/// Rumble state written by the ViGEm notification thread, read by poll_outputs().
#[derive(Default, Clone, Copy)]
struct XRumble { large: u8, small: u8, sequence: u64 }

pub struct VirtualXInput {
    id: String,
    display_name: String,
    /// None when ViGEmBus driver is not installed.
    target: Option<Xbox360Wired<Client>>,
    polled_feedback_sequence: u64,
    thumb_lx: i16,
    thumb_ly: i16,
    thumb_rx: i16,
    thumb_ry: i16,
    left_trigger: u8,
    right_trigger: u8,
    buttons: u16,
    /// Latest rumble values received from the game via ViGEm notifications.
    rumble: Arc<Mutex<XRumble>>,
    /// Join handle kept alive so the thread lives as long as this struct.
    _notif_thread: Option<std::thread::JoinHandle<()>>,
}

impl VirtualXInput {
    pub fn player_index(&mut self) -> Option<u32> {
        self.target.as_mut()?.get_user_index().ok().filter(|index| *index < 4)
    }

    pub fn flush_checked(&mut self) -> Result<(), String> {
        let target = self.target.as_mut().ok_or("ViGEm Xbox target is disconnected")?;
        let report = XGamepad {
            buttons: XButtons { raw: self.buttons },
            left_trigger: self.left_trigger,
            right_trigger: self.right_trigger,
            thumb_lx: self.thumb_lx,
            thumb_ly: self.thumb_ly,
            thumb_rx: self.thumb_rx,
            thumb_ry: self.thumb_ry,
        };
        target.update(&report).map_err(|error| format!("ViGEm Xbox report: {error}"))
    }

    pub fn new(instance: usize) -> Self {
        let (id, display_name) = instance_label("virtual.xinput", "Virtual XInput Controller", instance);
        let rumble = Arc::new(Mutex::new(XRumble::default()));
        let mut notif_thread = None;

        let target = Client::connect().ok().and_then(|client| {
            let mut t = Xbox360Wired::new(client, vigem_client::TargetId::XBOX360_WIRED);
            t.plugin().ok()?;
            t.wait_ready().ok()?;

            // request_notification() must be called before t is moved.
            // spawn_thread() takes ownership of the request and loops internally,
            // calling the callback each time the game sends XInputSetState.
            let rumble2 = Arc::clone(&rumble);
            if let Ok(req) = t.request_notification() {
                notif_thread = Some(req.spawn_thread(move |_, data| {
                    if let Ok(mut r) = rumble2.lock() {
                        r.large = data.large_motor;
                        r.small = data.small_motor;
                        r.sequence = r.sequence.wrapping_add(1);
                    }
                }));
            }

            Some(t)
        });

        Self {
            id, display_name,
            target,
            polled_feedback_sequence: 0,
            thumb_lx: 0, thumb_ly: 0,
            thumb_rx: 0, thumb_ry: 0,
            left_trigger: 0, right_trigger: 0,
            buttons: 0,
            rumble,
            _notif_thread: notif_thread,
        }
    }
}

impl VirtualDevice for VirtualXInput {
    fn id(&self) -> &str { &self.id }
    fn display_name(&self) -> &str { &self.display_name }
    fn sink_pins(&self) -> &'static [SinkPin] { layouts::XINPUT_SINK_PINS }
    fn is_connected(&self) -> bool { self.target.is_some() }

    fn send(&mut self, pin: &str, value: Signal) {
        match pin {
            "left_stick" => if let Signal::Vec2(v) = value {
                self.thumb_lx = float_to_i16(v.x);
                self.thumb_ly = float_to_i16(v.y);
            },
            "right_stick" => if let Signal::Vec2(v) = value {
                self.thumb_rx = float_to_i16(v.x);
                self.thumb_ry = float_to_i16(v.y);
            },
            "dpad" => if let Signal::Vec2(v) = value {
                set_bit(&mut self.buttons, XButtons::LEFT,  v.x < -0.5);
                set_bit(&mut self.buttons, XButtons::RIGHT, v.x >  0.5);
                set_bit(&mut self.buttons, XButtons::UP,    v.y >  0.5);
                set_bit(&mut self.buttons, XButtons::DOWN,  v.y < -0.5);
            },
            "left_stick_x"  => self.thumb_lx = float_to_i16(value.as_float()),
            "left_stick_y"  => self.thumb_ly = float_to_i16(value.as_float()),
            "right_stick_x" => self.thumb_rx = float_to_i16(value.as_float()),
            "right_stick_y" => self.thumb_ry = float_to_i16(value.as_float()),
            "left_trigger"  => self.left_trigger  = float_to_u8(value.as_float()),
            "right_trigger" => self.right_trigger = float_to_u8(value.as_float()),
            "btn_south"  => set_bool_bit(&mut self.buttons, XButtons::A,      &value),
            "btn_east"   => set_bool_bit(&mut self.buttons, XButtons::B,      &value),
            "btn_west"   => set_bool_bit(&mut self.buttons, XButtons::X,      &value),
            "btn_north"  => set_bool_bit(&mut self.buttons, XButtons::Y,      &value),
            "btn_lb"     => set_bool_bit(&mut self.buttons, XButtons::LB,     &value),
            "btn_rb"     => set_bool_bit(&mut self.buttons, XButtons::RB,     &value),
            "btn_ls"     => set_bool_bit(&mut self.buttons, XButtons::LTHUMB, &value),
            "btn_rs"     => set_bool_bit(&mut self.buttons, XButtons::RTHUMB, &value),
            "btn_start"  => set_bool_bit(&mut self.buttons, XButtons::START,  &value),
            "btn_back"   => set_bool_bit(&mut self.buttons, XButtons::BACK,   &value),
            "btn_guide"  => set_bool_bit(&mut self.buttons, XButtons::GUIDE,  &value),
            "dpad_up"    => set_bool_bit(&mut self.buttons, XButtons::UP,     &value),
            "dpad_down"  => set_bool_bit(&mut self.buttons, XButtons::DOWN,   &value),
            "dpad_left"  => set_bool_bit(&mut self.buttons, XButtons::LEFT,   &value),
            "dpad_right" => set_bool_bit(&mut self.buttons, XButtons::RIGHT,  &value),
            _ => {}
        }
    }

    fn flush(&mut self) {
        let _ = self.flush_checked();
    }

    fn reset_outputs(&mut self) {
        self.buttons = 0;
        self.left_trigger = 0;
        self.right_trigger = 0;
        self.thumb_lx = 0;
        self.thumb_ly = 0;
        self.thumb_rx = 0;
        self.thumb_ry = 0;
        self.flush();
    }

    fn source_pins(&self) -> &'static [SourcePin] { layouts::XINPUT_SOURCE_PINS }

    fn feedback_sequence(&self) -> u64 { self.polled_feedback_sequence }
    fn poll_outputs(&mut self) -> Vec<(&'static str, Signal)> {
        let r = self.rumble.lock().map(|r| *r).unwrap_or_default();
        self.polled_feedback_sequence=r.sequence;
        vec![
            ("rumble_strong", Signal::Float(r.large as f32 / 255.0)),
            ("rumble_weak",   Signal::Float(r.small as f32 / 255.0)),
        ]
    }
}

// ── DS4 (custom implementation with gyro support via DS4ReportEx IOCTL) ────────

/// Win32 types and functions for synchronous overlapped IOCTL calls.
mod vigem_ioctl {
    use std::ffi::c_void;

    // CTL_CODE(0x2A, fn, METHOD_BUFFERED=0, FILE_WRITE_DATA=2)
    // = (0x2A << 16) | (2 << 14) | (fn << 2) | 0
    pub const IOCTL_PLUGIN_TARGET: u32        = 0x2AA004; // fn=0x801
    pub const IOCTL_UNPLUG_TARGET: u32        = 0x2AA008; // fn=0x802
    pub const IOCTL_WAIT_DEVICE_READY: u32    = 0x2AA010; // fn=0x804
    pub const IOCTL_DS4_SUBMIT_REPORT: u32        = 0x002AA80C; // BUSENUM_W_IOCTL(0x801+0x202)
    // DS4 rumble/lightbar notification from game — BUSENUM_W_IOCTL(0x801+0x203)
    pub const IOCTL_DS4_REQUEST_NOTIFICATION: u32 = 0x002AA810;

    pub const DS4_TARGET_KIND: i32 = 2;
    pub const DS4_VID: u16         = 0x054C;
    pub const DS4_PID: u16         = 0x05C4;

    /// Minimal OVERLAPPED for synchronous overlapped I/O (Offset fields zeroed).
    #[repr(C)]
    pub struct Overlapped {
        pub internal:      usize,
        pub internal_high: usize,
        pub offset:        u32,
        pub offset_high:   u32,
        pub event:         *mut c_void,
    }

    // ── ViGEm lifecycle structures ─────────────────────────────────────────────

    #[repr(C)]
    pub struct PluginTarget {
        pub size:   u32,
        pub serial: u32,
        pub kind:   i32,
        pub vid:    u16,
        pub pid:    u16,
    }

    /// Shared layout for Unplug and WaitDeviceReady (just size + serial).
    #[repr(C)]
    pub struct LifecycleTarget {
        pub size:   u32,
        pub serial: u32,
    }

    // ── DS4 basic report (same layout as vigem_client's DS4Report) ─────────────

    #[repr(C)]
    pub struct DS4Report {
        pub lx: u8, pub ly: u8, pub rx: u8, pub ry: u8,
        pub buttons: u16,
        pub special: u8,
        pub lt: u8, pub rt: u8,
    }

    #[repr(C)]
    pub struct DS4Submit {
        pub size:   u32,
        pub serial: u32,
        pub report: DS4Report,
    }

    // ── DS4 extended report (with gyro/accel) ─────────────────────────────────
    // ViGEmBus defines these structs with #pragma pack(1) (pshpack1.h).
    // DS4_SUBMIT_REPORT_EX = 72 bytes, DS4_REPORT_EX = 63 bytes, DS4_TOUCH = 9 bytes.
    // Must use #[repr(C, packed)] to match — natural alignment adds padding and the
    // driver rejects the IOCTL when Size != sizeof(DS4_SUBMIT_REPORT_EX).

    #[repr(C, packed)]
    pub struct DS4Touch {
        pub counter: u8,
        /// bit 7 = finger-up (inactive), bits 6:0 = tracking ID
        pub track1:  u8,
        pub pos1:    [u8; 3],
        pub track2:  u8,
        pub pos2:    [u8; 3],
    }
    impl Default for DS4Touch {
        fn default() -> Self {
            Self { counter: 0, track1: 0x80, pos1: [0; 3], track2: 0x80, pos2: [0; 3] }
        }
    }

    // DS4_REPORT_EX in C is a union { struct { ... }; UCHAR ReportBuffer[63]; }.
    // The named fields sum to 60 bytes; the union size is 63 (the array arm wins).
    // We model this as a flat 63-byte array and project field accessors via methods.
    #[repr(C, packed)]
    pub struct DS4ReportEx {
        pub buf: [u8; 63],
    }

    // RESERVED: setters for the extended (gyro/accel/touchpad) DS4 report.
    // The `IOCTL_DS4_SUBMIT_REPORT_EX` path is scaffolded but never enabled —
    // `VirtualDS4::has_extended` is hardwired false pending bus-version
    // detection — so the basic `DS4Report` submit is the only live path today.
    #[allow(dead_code)]
    impl DS4ReportEx {
        pub fn set_lx(&mut self, v: u8) { self.buf[0] = v; }
        pub fn set_ly(&mut self, v: u8) { self.buf[1] = v; }
        pub fn set_rx(&mut self, v: u8) { self.buf[2] = v; }
        pub fn set_ry(&mut self, v: u8) { self.buf[3] = v; }
        pub fn set_buttons(&mut self, v: u16) {
            self.buf[4] = v as u8; self.buf[5] = (v >> 8) as u8;
        }
        pub fn set_special(&mut self, v: u8) { self.buf[6] = v; }
        pub fn set_lt(&mut self, v: u8) { self.buf[7] = v; }
        pub fn set_rt(&mut self, v: u8) { self.buf[8] = v; }
        // buf[9..10] = wTimestamp (leave 0)
        pub fn set_battery(&mut self, v: u8) { self.buf[11] = v; }
        pub fn set_gyro_x(&mut self, v: i16) {
            let b = v.to_le_bytes(); self.buf[12] = b[0]; self.buf[13] = b[1];
        }
        pub fn set_gyro_y(&mut self, v: i16) {
            let b = v.to_le_bytes(); self.buf[14] = b[0]; self.buf[15] = b[1];
        }
        pub fn set_gyro_z(&mut self, v: i16) {
            let b = v.to_le_bytes(); self.buf[16] = b[0]; self.buf[17] = b[1];
        }
        pub fn set_accel_x(&mut self, v: i16) {
            let b = v.to_le_bytes(); self.buf[18] = b[0]; self.buf[19] = b[1];
        }
        pub fn set_accel_y(&mut self, v: i16) {
            let b = v.to_le_bytes(); self.buf[20] = b[0]; self.buf[21] = b[1];
        }
        pub fn set_accel_z(&mut self, v: i16) {
            let b = v.to_le_bytes(); self.buf[22] = b[0]; self.buf[23] = b[1];
        }
        // buf[24..28] = _bUnknown1[5], buf[29] = bBatteryLvlSpecial, buf[30..31] = _bUnknown2
        pub fn set_touch_n(&mut self, v: u8) { self.buf[32] = v; }
        // DS4_TOUCH at offset 33 (current) and 42/51 (previous[0/1])
        // DS4_TOUCH layout: counter(1) track1(1) pos1[3] track2(1) pos2[3] = 9 bytes
        pub fn set_touch_cur(&mut self, counter: u8, track1: u8, pos1: [u8;3], track2: u8, pos2: [u8;3]) {
            let b = &mut self.buf[33..42];
            b[0]=counter; b[1]=track1; b[2]=pos1[0]; b[3]=pos1[1]; b[4]=pos1[2];
            b[5]=track2;  b[6]=pos2[0]; b[7]=pos2[1]; b[8]=pos2[2];
        }
        pub fn clear_touch_prev(&mut self) {
            // track bits 7 = finger-up (inactive)
            self.buf[42] = 0; self.buf[43] = 0x80; self.buf[44..47].fill(0);
            self.buf[47] = 0x80; self.buf[48..51].fill(0);
            self.buf[51] = 0; self.buf[52] = 0x80; self.buf[53..56].fill(0);
            self.buf[56] = 0x80; self.buf[57..60].fill(0);
        }
    }

    #[repr(C, packed)]
    pub struct DS4ExSubmit {
        pub size:   u32,
        pub serial: u32,
        pub report: DS4ReportEx,
    }

    const _: () = {
        assert!(std::mem::size_of::<DS4Touch>()    ==  9);
        assert!(std::mem::size_of::<DS4ReportEx>() == 63);
        assert!(std::mem::size_of::<DS4ExSubmit>() == 71);
    };

    // ── DS4 notification (rumble + lightbar from game) ────────────────────────
    // Real DS4_REQUEST_NOTIFICATION layout (16 bytes, observed from raw driver output):
    //   Size(4) + SerialNo(4) + SmallMotor(1) + LargeMotor(1) + R(1) + G(1) + B(1) + pad[3]
    // No padding between SerialNo and DS4_OUTPUT_REPORT — driver writes motors at offset 8.
    #[repr(C)]
    pub struct DS4NotificationBuffer {
        pub size:        u32,
        pub serial:      u32,
        pub small_motor: u8,
        pub large_motor: u8,
        pub lightbar_r:  u8,
        pub lightbar_g:  u8,
        pub lightbar_b:  u8,
        pub _tail:       [u8; 3], // pad to 16 bytes (matches driver-reported Size)
    }

    /// Submit a non-blocking overlapped IOCTL (does not wait for completion).
    /// The caller must keep `overlapped` pinned until polled.
    pub unsafe fn ioctl_async(
        device:     *mut c_void,
        overlapped: *mut Overlapped,
        code:       u32,
        buf:        *mut c_void,
        size:       u32,
    ) {
        let mut n = 0u32;
        DeviceIoControl(device, code, buf, size, buf, size, &mut n, overlapped);
    }

    /// Non-blocking poll of a pending overlapped operation.
    /// Returns Some(()) if completed, None if still pending, Err(code) on error/abort.
    ///
    /// RESERVED: the notification thread currently blocks on
    /// `GetOverlappedResult(bWait=TRUE)` with its own duplicated handle;
    /// this poll variant is kept for a possible non-blocking redesign.
    #[allow(dead_code)]
    pub unsafe fn poll_overlapped(
        device:     *mut c_void,
        overlapped: *mut Overlapped,
    ) -> Result<Option<()>, u32> {
        let mut n = 0u32;
        if GetOverlappedResult(device, overlapped, &mut n, 0 /* bWait=FALSE */) != 0 {
            Ok(Some(()))
        } else {
            let err = GetLastError();
            const ERROR_IO_INCOMPLETE: u32 = 996;
            if err == ERROR_IO_INCOMPLETE { Ok(None) } else { Err(err) }
        }
    }

    // ── Win32 kernel32 imports ─────────────────────────────────────────────────

    #[link(name = "kernel32")]
    extern "system" {
        pub fn CreateEventW(
            attrs:         *mut c_void,
            manual_reset:  i32,
            initial_state: i32,
            name:          *const u16,
        ) -> *mut c_void;

        pub fn CloseHandle(handle: *mut c_void) -> i32;

        pub fn DeviceIoControl(
            device:      *mut c_void,
            code:        u32,
            in_buf:      *const c_void,
            in_size:     u32,
            out_buf:     *mut c_void,
            out_size:    u32,
            returned:    *mut u32,
            overlapped:  *mut Overlapped,
        ) -> i32;

        pub fn GetOverlappedResult(
            file:       *mut c_void,
            overlapped: *mut Overlapped,
            transferred: *mut u32,
            wait:        i32,
        ) -> i32;

        pub fn GetLastError() -> u32;
    }

    /// Submit a synchronous IOCTL on an overlapped device handle.
    /// Returns true on success.
    pub unsafe fn ioctl(
        device: *mut c_void,
        event:  *mut c_void,
        code:   u32,
        buf:    *const c_void,
        size:   u32,
    ) -> bool {
        let mut ovl = Overlapped {
            internal: 0, internal_high: 0, offset: 0, offset_high: 0, event,
        };
        let mut n = 0u32;
        if DeviceIoControl(device, code, buf, size, std::ptr::null_mut(), 0, &mut n, &mut ovl) != 0 {
            return true;
        }
        // Only pending operations own an asynchronous completion. Preserve an
        // immediate failure's Win32 error instead of querying an unqueued IOCTL.
        const ERROR_IO_PENDING: u32 = 997;
        if GetLastError() != ERROR_IO_PENDING { return false; }
        GetOverlappedResult(device, &mut ovl, &mut n, 1 /* bWait=TRUE */) != 0
    }
}

// ── DS4 button bit masks ───────────────────────────────────────────────────────
mod ds4_btn {
    pub const SQUARE:   u16 = 0x0010;
    pub const CROSS:    u16 = 0x0020;
    pub const CIRCLE:   u16 = 0x0040;
    pub const TRIANGLE: u16 = 0x0080;
    pub const L1:       u16 = 0x0100;
    pub const R1:       u16 = 0x0200;
    pub const L2_DIG:   u16 = 0x0400;
    pub const R2_DIG:   u16 = 0x0800;
    pub const SHARE:    u16 = 0x1000;
    pub const OPTIONS:  u16 = 0x2000;
    pub const L3:       u16 = 0x4000;
    pub const R3:       u16 = 0x8000;
    pub const PS:       u8  = 0x01;
    pub const TOUCHPAD: u8  = 0x02;
}

/// Rumble + lightbar values received from a game via DS4 notification IOCTL.
#[derive(Default, Clone, Copy)]
struct DS4Rumble {
    large: u8, small: u8,
    r: u8, g: u8, b: u8,
}

/// Shared state between the DS4 notification thread and poll_outputs() reader.
struct DS4NotifShared {
    sequence: u64,
    rumble: DS4Rumble,
    stop:   bool,
}

pub struct VirtualDS4 {
    polled_feedback_sequence: u64,
    id:           String,
    display_name: String,
    /// Keeps the ViGEm bus connection alive; raw handle borrowed via `dev`.
    _client:      Option<Client>,
    /// Raw handle from `_client`; null when ViGEmBus is unavailable.
    dev:          *mut std::ffi::c_void,
    serial:       u32,
    /// Windows event for overlapped I/O (submit); null when not connected.
    event:        *mut std::ffi::c_void,
    /// Whether `IOCTL_DS4_SUBMIT_REPORT_EX` is supported by installed ViGEmBus.
    has_extended: bool,
    // Report state
    lx: u8, ly: u8, rx: u8, ry: u8,
    lt: u8, rt: u8,
    buttons:  u16,
    special:  u8,
    dpad:     [bool; 4],
    // Gyro / accelerometer (float in [-1, 1], converted to i16 in flush)
    gyro_x:  f32, gyro_y:  f32, gyro_z:  f32,
    accel_x: f32, accel_y: f32, accel_z: f32,
    /// Rumble/lightbar feedback from game — updated by background notification thread.
    notif_shared: Arc<Mutex<DS4NotifShared>>,
    _notif_thread: Option<std::thread::JoinHandle<()>>,
}

// Raw pointer fields are not Send by default; declare it safe because the
// handles are owned exclusively by this struct and accessed on one thread.
unsafe impl Send for VirtualDS4 {}

impl VirtualDS4 {
    fn make_notif_buf(serial: u32) -> Box<vigem_ioctl::DS4NotificationBuffer> {
        Box::new(vigem_ioctl::DS4NotificationBuffer {
            size:        16,
            serial,
            small_motor: 0, large_motor: 0,
            lightbar_r:  0, lightbar_g:  0, lightbar_b: 0,
            _tail:       [0; 3],
        })
    }

    // RESERVED: overlapped-struct builder for the non-blocking notification
    // design (pairs with `vigem_ioctl::poll_overlapped`); the current
    // notification thread blocks with its own handle and doesn't need it.
    #[allow(dead_code)]
    fn make_notif_ovl() -> Box<vigem_ioctl::Overlapped> {
        Box::new(vigem_ioctl::Overlapped {
            internal: 0, internal_high: 0, offset: 0, offset_high: 0,
            event: unsafe { vigem_ioctl::CreateEventW(std::ptr::null_mut(), 0, 0, std::ptr::null()) },
        })
    }

    pub fn new(instance: usize) -> Self {
        let (id, display_name) = instance_label("virtual.ds4", "Virtual DualShock 4", instance);
        let notif_shared = Arc::new(Mutex::new(DS4NotifShared {
            sequence: 0,
            rumble: DS4Rumble::default(),
            stop:   false,
        }));
        let mut out = VirtualDS4 {
            id, display_name,
            polled_feedback_sequence: 0,
            _client: None, dev: std::ptr::null_mut(),
            serial: 0, event: std::ptr::null_mut(), has_extended: false,
            lx: 0x80, ly: 0x80, rx: 0x80, ry: 0x80,
            lt: 0, rt: 0, buttons: 0, special: 0, dpad: [false; 4],
            gyro_x: 0.0, gyro_y: 0.0, gyro_z: 0.0,
            accel_x: 0.0, accel_y: 0.0, accel_z: 0.0,
            notif_shared,
            _notif_thread: None,
        };

        let client = match Client::connect() {
            Ok(c) => c,
            Err(_) => return out,
        };
        let dev = client.as_raw_handle() as *mut std::ffi::c_void;
        let event = unsafe {
            vigem_ioctl::CreateEventW(std::ptr::null_mut(), 0, 0, std::ptr::null())
        };
        if event.is_null() { return out; }

        // Find a free serial number and plug in the DS4 target.
        let mut serial = 1u32;
        loop {
            let plug = vigem_ioctl::PluginTarget {
                size:   std::mem::size_of::<vigem_ioctl::PluginTarget>() as u32,
                serial,
                kind:   vigem_ioctl::DS4_TARGET_KIND,
                vid:    vigem_ioctl::DS4_VID,
                pid:    vigem_ioctl::DS4_PID,
            };
            if unsafe { vigem_ioctl::ioctl(
                dev, event, vigem_ioctl::IOCTL_PLUGIN_TARGET,
                &plug as *const _ as _, std::mem::size_of_val(&plug) as u32,
            ) } { break; }
            serial += 1;
            if serial > 65535 {
                unsafe { vigem_ioctl::CloseHandle(event); }
                return out;
            }
        }

        // Wait until the virtual controller is enumerated and ready.
        let wait = vigem_ioctl::LifecycleTarget {
            size: std::mem::size_of::<vigem_ioctl::LifecycleTarget>() as u32,
            serial,
        };
        unsafe { vigem_ioctl::ioctl(
            dev, event, vigem_ioctl::IOCTL_WAIT_DEVICE_READY,
            &wait as *const _ as _, std::mem::size_of_val(&wait) as u32,
        ); }

        out._client      = Some(client);
        out.dev          = dev;
        out.serial       = serial;
        out.event        = event;
        out.has_extended = false;

        eprintln!("[VirtualDS4] plugged in serial={serial}");

        // Spawn a notification thread with its OWN duplicated ViGEm handle.
        // Sharing the main handle blocks all subsequent submit IOCTLs behind the
        // long-pending notification IOCTL, throttling DS4 polling to a crawl and
        // also stalling physical-device HID writes on the same I/O thread.
        if let Some(client_ref) = out._client.as_ref() {
            if let Ok(notif_client) = client_ref.try_clone() {
                let notif_dev = notif_client.as_raw_handle() as *mut std::ffi::c_void;
                let shared = Arc::clone(&out.notif_shared);
                let dev_ptr = SendPtr(notif_dev);
                let handle = std::thread::Builder::new()
                    .name(format!("ds4-notif-{instance}"))
                    .spawn(move || {
                        // Move the duplicated client into the thread so its handle
                        // stays open for the lifetime of the notification loop.
                        let _hold = notif_client;
                        ds4_notif_thread(dev_ptr, serial, shared);
                    })
                    .ok();
                out._notif_thread = handle;
            } else {
                eprintln!("[VirtualDS4] failed to duplicate ViGEm handle — feedback disabled");
            }
        }

        out
    }

}

/// Raw-pointer wrapper marked Send so it can cross to the notification thread.
/// SAFETY: the ViGEm device handle is shared between the main I/O thread (issuing
/// DS4_SUBMIT_REPORT) and the notification thread (waiting on DS4_REQUEST_NOTIFICATION).
/// Windows kernel I/O on a HANDLE is thread-safe for distinct overlapped operations.
struct SendPtr(*mut std::ffi::c_void);
unsafe impl Send for SendPtr {}

/// Dedicated thread that blocks on DS4 notification IOCTLs. Sleeps in the kernel
/// until the game sends rumble/lightbar, then updates `shared.rumble` and loops.
fn ds4_notif_thread(dev_ptr: SendPtr, serial: u32, shared: Arc<Mutex<DS4NotifShared>>) {
    let dev = dev_ptr.0;
    let mut buf = VirtualDS4::make_notif_buf(serial);
    let event = unsafe {
        vigem_ioctl::CreateEventW(std::ptr::null_mut(), 0, 0, std::ptr::null())
    };
    if event.is_null() { return; }
    let mut ovl = vigem_ioctl::Overlapped {
        internal: 0, internal_high: 0, offset: 0, offset_high: 0, event,
    };
    loop {
        // Check stop flag before each blocking call.
        if shared.lock().map(|s| s.stop).unwrap_or(true) { break; }

        // Reset overlapped state and reissue the request.
        ovl.internal = 0;
        ovl.internal_high = 0;
        unsafe {
            vigem_ioctl::ioctl_async(
                dev, &mut ovl as *mut _,
                vigem_ioctl::IOCTL_DS4_REQUEST_NOTIFICATION,
                buf.as_mut() as *mut _ as _,
                16,
            );
        }
        // Block until completion (bWait=TRUE) — kernel parks this thread, no CPU spin.
        let mut n = 0u32;
        let ok = unsafe {
            vigem_ioctl::GetOverlappedResult(dev, &mut ovl as *mut _, &mut n, 1)
        };
        if ok == 0 {
            let err = unsafe { vigem_ioctl::GetLastError() };
            // ERROR_OPERATION_ABORTED (995) fires when the target is unplugged.
            if err != 995 {
                eprintln!("[VirtualDS4 notif-thread] error winerr={err:#010x}, exiting");
            }
            break;
        }

        let new_rumble = DS4Rumble {
            large: buf.large_motor,
            small: buf.small_motor,
            r:     buf.lightbar_r,
            g:     buf.lightbar_g,
            b:     buf.lightbar_b,
        };
        if let Ok(mut s) = shared.lock() {
            if s.stop { break; }
            let changed = (new_rumble.large, new_rumble.small, new_rumble.r, new_rumble.g, new_rumble.b)
                != (s.rumble.large, s.rumble.small, s.rumble.r, s.rumble.g, s.rumble.b);
            if changed {
                eprintln!("[VirtualDS4] notification: large={} small={} rgb=({},{},{})",
                    new_rumble.large, new_rumble.small, new_rumble.r, new_rumble.g, new_rumble.b);
            }
            s.rumble = new_rumble;
            s.sequence = s.sequence.wrapping_add(1);
        }
    }
    unsafe { vigem_ioctl::CloseHandle(event); }
}

impl Drop for VirtualDS4 {
    fn drop(&mut self) {
        // Signal the notification thread to exit on its next wake-up.
        if let Ok(mut s) = self.notif_shared.lock() { s.stop = true; }
        if self.dev.is_null() { return; }
        let unplug = vigem_ioctl::LifecycleTarget {
            size:   std::mem::size_of::<vigem_ioctl::LifecycleTarget>() as u32,
            serial: self.serial,
        };
        unsafe {
            // Unplugging aborts any pending notification IOCTL with ERROR_OPERATION_ABORTED,
            // unblocking the notification thread so it can observe `stop` and exit.
            vigem_ioctl::ioctl(
                self.dev, self.event, vigem_ioctl::IOCTL_UNPLUG_TARGET,
                &unplug as *const _ as _, std::mem::size_of_val(&unplug) as u32,
            );
            vigem_ioctl::CloseHandle(self.event);
        }
    }
}

impl VirtualDevice for VirtualDS4 {
    fn id(&self)           -> &str { &self.id }
    fn display_name(&self) -> &str { &self.display_name }
    fn sink_pins(&self)    -> &'static [SinkPin] { layouts::DS4_SINK_PINS }
    fn is_connected(&self) -> bool { !self.dev.is_null() }

    fn send(&mut self, pin: &str, value: Signal) {
        match pin {
            "left_stick" => if let Signal::Vec2(v) = value {
                self.lx = ds4_axis_x(v.x); self.ly = ds4_axis_y(v.y);
            },
            "right_stick" => if let Signal::Vec2(v) = value {
                self.rx = ds4_axis_x(v.x); self.ry = ds4_axis_y(v.y);
            },
            "dpad" => if let Signal::Vec2(v) = value {
                self.dpad[0] = v.y >  0.5; self.dpad[1] = v.x >  0.5;
                self.dpad[2] = v.y < -0.5; self.dpad[3] = v.x < -0.5;
            },
            "left_stick_x"  => self.lx = ds4_axis_x(value.as_float()),
            "left_stick_y"  => self.ly = ds4_axis_y(value.as_float()),
            "right_stick_x" => self.rx = ds4_axis_x(value.as_float()),
            "right_stick_y" => self.ry = ds4_axis_y(value.as_float()),
            "left_trigger"  => self.lt = float_to_u8(value.as_float()),
            "right_trigger" => self.rt = float_to_u8(value.as_float()),
            "btn_south"    => set_bool_bit(&mut self.buttons, ds4_btn::CROSS,    &value),
            "btn_east"     => set_bool_bit(&mut self.buttons, ds4_btn::CIRCLE,   &value),
            "btn_west"     => set_bool_bit(&mut self.buttons, ds4_btn::SQUARE,   &value),
            "btn_north"    => set_bool_bit(&mut self.buttons, ds4_btn::TRIANGLE, &value),
            "btn_lb"       => set_bool_bit(&mut self.buttons, ds4_btn::L1,       &value),
            "btn_rb"       => set_bool_bit(&mut self.buttons, ds4_btn::R1,       &value),
            "btn_lt_dig"   => set_bool_bit(&mut self.buttons, ds4_btn::L2_DIG,   &value),
            "btn_rt_dig"   => set_bool_bit(&mut self.buttons, ds4_btn::R2_DIG,   &value),
            "btn_ls"       => set_bool_bit(&mut self.buttons, ds4_btn::L3,       &value),
            "btn_rs"       => set_bool_bit(&mut self.buttons, ds4_btn::R3,       &value),
            "btn_start"    => set_bool_bit(&mut self.buttons, ds4_btn::OPTIONS,  &value),
            "btn_back"     => set_bool_bit(&mut self.buttons, ds4_btn::SHARE,    &value),
            "btn_guide"    => set_bool_u8(&mut self.special, ds4_btn::PS,        &value),
            "btn_touchpad" => set_bool_u8(&mut self.special, ds4_btn::TOUCHPAD,  &value),
            "dpad_up"    => self.dpad[0] = value.as_bool(),
            "dpad_right" => self.dpad[1] = value.as_bool(),
            "dpad_down"  => self.dpad[2] = value.as_bool(),
            "dpad_left"  => self.dpad[3] = value.as_bool(),
            "gyro_x"  => self.gyro_x  = value.as_float(),
            "gyro_y"  => self.gyro_y  = value.as_float(),
            "gyro_z"  => self.gyro_z  = value.as_float(),
            "accel_x" => self.accel_x = value.as_float(),
            "accel_y" => self.accel_y = value.as_float(),
            "accel_z" => self.accel_z = value.as_float(),
            _ => {}
        }
    }

    fn flush(&mut self) {
        let _ = self.flush_checked();
    }

    fn reset_outputs(&mut self) {
        let center = ds4_axis_x(0.0); // 127 — neutral stick position
        self.lx = center; self.ly = center;
        self.rx = center; self.ry = center;
        self.lt = 0; self.rt = 0;
        self.buttons = 0;
        self.special = 0;
        self.dpad = [false; 4];
        self.gyro_x = 0.0; self.gyro_y = 0.0; self.gyro_z = 0.0;
        self.accel_x = 0.0; self.accel_y = 0.0; self.accel_z = 0.0;
        self.flush();
    }

    fn source_pins(&self) -> &'static [SourcePin] { layouts::DS4_SOURCE_PINS }

    fn feedback_sequence(&self) -> u64 { self.polled_feedback_sequence }
    fn poll_outputs(&mut self) -> Vec<(&'static str, Signal)> {
        if self.dev.is_null() { return vec![]; }
        let (sequence,r) = self.notif_shared.lock().map(|s| (s.sequence,s.rumble)).unwrap_or_default();
        self.polled_feedback_sequence=sequence;
        vec![
            ("rumble_strong", Signal::Float(r.large as f32 / 255.0)),
            ("rumble_weak",   Signal::Float(r.small as f32 / 255.0)),
            ("lightbar_r",    Signal::Float(r.r     as f32 / 255.0)),
            ("lightbar_g",    Signal::Float(r.g     as f32 / 255.0)),
            ("lightbar_b",    Signal::Float(r.b     as f32 / 255.0)),
        ]
    }
}

impl VirtualDS4 {
    pub fn flush_checked(&mut self) -> Result<(), String> {
        if self.dev.is_null() { return Err("ViGEm DS4 target is disconnected".into()); }
        let dpad    = encode_dpad(self.dpad[0], self.dpad[1], self.dpad[2], self.dpad[3]);
        let buttons = (self.buttons & !0xF) | dpad;

        let sub = vigem_ioctl::DS4Submit {
            size:   std::mem::size_of::<vigem_ioctl::DS4Submit>() as u32,
            serial: self.serial,
            report: vigem_ioctl::DS4Report {
                lx: self.lx, ly: self.ly, rx: self.rx, ry: self.ry,
                buttons, special: self.special, lt: self.lt, rt: self.rt,
            },
        };
        let accepted = unsafe {
            vigem_ioctl::ioctl(
                self.dev, self.event, vigem_ioctl::IOCTL_DS4_SUBMIT_REPORT,
                &sub as *const _ as _, std::mem::size_of_val(&sub) as u32,
            )
        };
        if accepted { return Ok(()); }
        let error = std::io::Error::last_os_error();
        // ViGEmBus SubmitReportImpl returns STATUS_NO_MORE_ENTRIES when no
        // USB IN request is waiting. The official client treats this as nonfatal.
        // The report was NOT cached: the host must resubmit the retained pins on
        // its next output tick, even if their values have not changed.
        const ERROR_NO_MORE_ITEMS: i32 = 259;
        if error.raw_os_error() == Some(ERROR_NO_MORE_ITEMS) {
            Ok(())
        } else {
            Err(format!("ViGEm DS4 report: {error}"))
        }
    }
}

fn float_to_i16(f: f32) -> i16 {
    (f.clamp(-1.0, 1.0) * 32767.0) as i16
}

fn float_to_u8(f: f32) -> u8 {
    (f.clamp(0.0, 1.0) * 255.0) as u8
}

fn ds4_axis_x(f: f32) -> u8 {
    ((f.clamp(-1.0, 1.0) + 1.0) * 0.5 * 255.0) as u8
}

fn ds4_axis_y(f: f32) -> u8 {
    ((-f.clamp(-1.0, 1.0) + 1.0) * 0.5 * 255.0) as u8
}

fn encode_dpad(up: bool, right: bool, down: bool, left: bool) -> u16 {
    match (up, right, down, left) {
        (true,  false, false, false) => 0,
        (true,  true,  false, false) => 1,
        (false, true,  false, false) => 2,
        (false, true,  true,  false) => 3,
        (false, false, true,  false) => 4,
        (false, false, true,  true ) => 5,
        (false, false, false, true ) => 6,
        (true,  false, false, true ) => 7,
        _                            => 8,
    }
}

fn set_bit(bits: &mut u16, mask: u16, on: bool) {
    if on { *bits |= mask; } else { *bits &= !mask; }
}

fn set_bool_bit(bits: &mut u16, mask: u16, value: &Signal) {
    // Universal coercion: any Float >= 0.5 → true, any Bool → its value, etc.
    set_bit(bits, mask, value.as_bool());
}

fn set_bool_u8(bits: &mut u8, mask: u8, value: &Signal) {
    if value.as_bool() { *bits |= mask; } else { *bits &= !mask; }
}


#[cfg(test)]
mod feedback_snapshot_tests {
    use super::*;
    #[test]
    fn command_sequence_is_atomic_with_the_polled_levels() {
        // No constructor, bus connection, target or notification thread.
        let rumble=Arc::new(Mutex::new(XRumble{large:128,small:64,sequence:1}));
        let mut pad=VirtualXInput{id:"offline".into(),display_name:"offline".into(),target:None,polled_feedback_sequence:0,thumb_lx:0,thumb_ly:0,thumb_rx:0,thumb_ry:0,left_trigger:0,right_trigger:0,buttons:0,rumble:rumble.clone(),_notif_thread:None};
        let first=pad.poll_outputs();assert_eq!(pad.feedback_sequence(),1);
        rumble.lock().unwrap().sequence=2;
        assert_eq!(pad.feedback_sequence(),1,"callback cannot advance an older snapshot's sequence");
        assert_eq!(pad.poll_outputs(),first);assert_eq!(pad.feedback_sequence(),2,"same-level commands are fresh");
    }
}
