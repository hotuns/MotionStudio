//! Shared native control, guarded inspection and engine authorization.
use super::policy::{self, Driver, DriverState};
use anyhow::{bail, ensure, Result};
use flexinput_hidmaestro::hidhide::HidHide;
use std::{path::Path, time::Duration};
use windows_sys::Win32::{Foundation::*, System::Threading::*};

pub(super) struct Gate(HANDLE);
impl Gate {
    fn lock(timeout: u32) -> Result<Option<Self>> {
        let name: Vec<u16> = "Local\\MotionStudio.HidHide.MappingLease"
            .encode_utf16()
            .chain([0])
            .collect();
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        ensure!(!handle.is_null(), "无法取得隐藏规则控制权");
        let result = unsafe { WaitForSingleObject(handle, timeout) };
        if matches!(result, WAIT_OBJECT_0 | WAIT_ABANDONED) {
            return Ok(Some(Self(handle)));
        }
        unsafe {
            CloseHandle(handle);
        }
        ensure!(result == WAIT_TIMEOUT, "无法确认隐藏规则控制权");
        Ok(None)
    }
    pub(super) fn acquire() -> Result<Self> {
        Self::lock(1500)?.ok_or_else(|| anyhow::anyhow!("另一隐藏会话仍在运行或恢复，请稍后重试"))
    }
}
impl Drop for Gate {
    fn drop(&mut self) {
        unsafe {
            ReleaseMutex(self.0);
            CloseHandle(self.0);
        }
    }
}

#[derive(Debug)]
pub struct InspectionError {
    pub code: Option<u32>,
    pub message: String,
}
fn read_state(driver: &HidHide) -> Result<DriverState, String> {
    let state = driver.checked_state()?;
    Ok(DriverState {
        active: state.active,
        inverse: state.inverse,
        hidden: state.blacklist,
        apps: state.whitelist,
    })
}

pub(super) struct RealDriver(HidHide);
impl RealDriver {
    pub(super) fn open() -> Result<Self> {
        let mut last = 0;
        for _ in 0..5 {
            match HidHide::open_checked() {
                Ok(driver) => return Ok(Self(driver)),
                Err(code) => last = code,
            }
            if last != 32 {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        bail!("无法访问 HidHide（Windows {last}）。请确认已安装 HidHide，并关闭官方配置工具后重试；也可停止后关闭自动隐藏")
    }
}
impl Driver for RealDriver {
    fn read(&mut self) -> Result<DriverState> {
        read_state(&self.0).map_err(anyhow::Error::msg)
    }
    fn set_hidden(&mut self, paths: &[String]) -> Result<()> {
        ensure!(self.0.set_blacklist(paths), "HidHide 拒绝更新实体隐藏规则");
        Ok(())
    }
    fn set_apps(&mut self, paths: &[String]) -> Result<()> {
        ensure!(self.0.set_whitelist(paths), "HidHide 拒绝登记引擎读取权限");
        Ok(())
    }
    fn set_active(&mut self, active: bool) -> Result<()> {
        ensure!(self.0.set_active(active), "HidHide 拒绝更新启用状态");
        Ok(())
    }
}

pub fn authorize_engine(path: &Path) -> Result<()> {
    let engine = native_image_path(path)?;
    let _gate = Gate::acquire()?;
    policy::authorize_engine(&mut RealDriver::open()?, &engine)
}

fn guarded_read<G>(
    gate: impl FnOnce() -> Result<Option<G>>,
    read: impl FnOnce() -> Result<DriverState, InspectionError>,
) -> Result<Option<DriverState>, InspectionError> {
    let Some(_gate) = gate().map_err(|error| InspectionError {
        code: None,
        message: error.to_string(),
    })?
    else {
        return Ok(None);
    };
    read().map(Some)
}

/// None means a mapping/recovery lease owns control: do not open another handle.
/// Only GET requests are sent; enumeration and path conversion happen after close.
pub fn inspect() -> Result<Option<DriverState>, InspectionError> {
    guarded_read(
        || Gate::lock(0),
        || {
            let driver = HidHide::open_checked().map_err(|code| InspectionError {
                code: Some(code),
                message: format!("Windows {code}"),
            })?;
            read_state(&driver).map_err(|message| InspectionError {
                code: None,
                message,
            })
        },
    )
}

/// Match the NT image path recorded by the engine's existing recovery companion.
/// This opens only the executable file, never a controller or the control device.
pub fn native_image_path(path: &Path) -> Result<String> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{GetFinalPathNameByHandleW, VOLUME_NAME_NT};
    let file = std::fs::File::open(path)?;
    let mut buffer = vec![0u16; 32768];
    let count = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle(),
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            VOLUME_NAME_NT,
        )
    } as usize;
    ensure!(
        count > 0 && count < buffer.len(),
        "无法确认引擎的原生文件路径"
    );
    Ok(String::from_utf16(&buffer[..count])?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn active_lease_skips_driver_access_entirely() {
        let result = guarded_read(
            || Ok(None::<()>),
            || panic!("must not compete with the mapping lease"),
        );
        assert!(result.unwrap().is_none());
    }
    #[test]
    fn coordination_failure_is_not_a_missing_driver_or_an_empty_policy() {
        let result = guarded_read::<()>(
            || anyhow::bail!("mutex unavailable"),
            || panic!("must not read without ownership"),
        );
        assert_eq!(result.unwrap_err().message, "mutex unavailable");
    }
    #[test]
    fn failed_native_read_preserves_failure_and_releases_the_gate() {
        use std::cell::Cell;
        struct MockGate<'a>(&'a Cell<bool>);
        impl Drop for MockGate<'_> {
            fn drop(&mut self) {
                self.0.set(true);
            }
        }
        let released = Cell::new(false);
        let result = guarded_read(
            || Ok(Some(MockGate(&released))),
            || {
                Err(InspectionError {
                    code: Some(5),
                    message: "denied".into(),
                })
            },
        );
        assert_eq!(result.unwrap_err().code, Some(5));
        assert!(released.get());
    }
}
