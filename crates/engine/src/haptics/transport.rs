//! Uses only the already verified, process-pinned SDL DLL's HID API.
//! The separate Windows HID handle receives its own reports; it never drains
//! SDL's input queue or changes its EF/gyro setup. All I/O stays on the worker.
use super::protocol::Layout;
use anyhow::{ensure, Context, Result};
use libloading::Library;
use std::ffi::{c_char, c_void, CString};

pub trait Transport {
    fn layout(&self) -> &Layout;
    fn write(&mut self, frame: &[u8; 32]) -> Result<()>;
    fn read(&mut self, timeout_ms: i32) -> Result<Vec<u8>>;
}
pub type Factory = Box<dyn FnOnce() -> Result<Box<dyn Transport>> + Send>;

#[derive(Clone, Copy)]
struct Api {
    open: unsafe extern "C" fn(*const c_char) -> *mut c_void,
    close: unsafe extern "C" fn(*mut c_void),
    write: unsafe extern "C" fn(*mut c_void, *const u8, usize) -> i32,
    read: unsafe extern "C" fn(*mut c_void, *mut u8, usize, i32) -> i32,
    descriptor: unsafe extern "C" fn(*mut c_void, *mut u8, usize) -> i32,
}
struct Hid {
    api: Api,
    handle: *mut c_void,
    layout: Layout,
}
impl Drop for Hid {
    fn drop(&mut self) {
        unsafe { (self.api.close)(self.handle) };
    }
}
impl Transport for Hid {
    fn layout(&self) -> &Layout {
        &self.layout
    }
    fn write(&mut self, frame: &[u8; 32]) -> Result<()> {
        let bytes = self.layout.wrap(frame);
        let sent = unsafe { (self.api.write)(self.handle, bytes.as_ptr(), bytes.len()) };
        ensure!(
            sent == bytes.len() as i32,
            "APEX HID write failed or was incomplete ({sent})"
        );
        Ok(())
    }
    fn read(&mut self, timeout_ms: i32) -> Result<Vec<u8>> {
        let mut bytes = [0u8; 65];
        let count =
            unsafe { (self.api.read)(self.handle, bytes.as_mut_ptr(), bytes.len(), timeout_ms) };
        ensure!(count >= 0, "APEX HID connection lost");
        Ok(self.layout.unwrap(&bytes[..count as usize]).to_vec())
    }
}

pub fn factory(
    library: &'static Library,
    path: &str,
    vendor: u16,
    product: u16,
) -> Result<Factory> {
    ensure!(
        !cfg!(test)
            && std::env::var("SDL_JOYSTICK_HIDAPI").as_deref() != Ok("0")
            && std::env::var_os("MOTION_STUDIO_DISABLE_PHYSICAL").is_none(),
        "Physical haptic backend is disabled"
    );
    ensure!(
        vendor == 0x37d7 && matches!(product, 0x2502 | 0x2401),
        "APEX audio haptics requires a supported APEX 6 Pro USB/receiver interface"
    );
    ensure!(
        path.starts_with(r"\\?\") && path.len() < 2048,
        "Select the physical APEX vendor HID interface"
    );
    let path = CString::new(path).context("Invalid HID path")?;
    // No LoadLibrary here: these addresses belong to Sdl::load's verified DLL,
    // kept resident for the process lifetime. No additional SDL runtime.
    let api = unsafe {
        Api {
            open: *library.get(b"SDL_hid_open_path\0")?,
            close: *library.get(b"SDL_hid_close\0")?,
            write: *library.get(b"SDL_hid_write\0")?,
            read: *library.get(b"SDL_hid_read_timeout\0")?,
            descriptor: *library.get(b"SDL_hid_get_report_descriptor\0")?,
        }
    };
    Ok(Box::new(move || {
        let handle = unsafe { (api.open)(path.as_ptr()) };
        ensure!(!handle.is_null(), "Cannot open selected APEX HID interface");
        let mut hid = Hid {
            api,
            handle,
            layout: Layout {
                output_id: 0,
                output_size: 32,
                input_ids: vec![0],
            },
        };
        let mut bytes = [0u8; 4096];
        let count = unsafe { (api.descriptor)(handle, bytes.as_mut_ptr(), bytes.len()) };
        ensure!(
            count > 0 && count as usize <= bytes.len(),
            "Cannot read APEX HID report descriptor"
        );
        hid.layout = Layout::parse(&bytes[..count as usize])?;
        Ok(Box::new(hid))
    }))
}
