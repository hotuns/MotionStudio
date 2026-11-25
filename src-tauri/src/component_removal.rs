use crate::{
    components::{self, DriverInfo},
    installers, runtime,
};
use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use uuid::Uuid;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Component {
    HidHide,
    Vigem,
    HidMaestro,
    Usbip,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RemovalMethod {
    Direct,
    Windows,
}

pub(super) enum Removal {
    Msi(Uuid),
    HidMaestro,
    Windows,
}

impl Removal {
    pub fn method(&self) -> RemovalMethod {
        match self {
            Self::Windows => RemovalMethod::Windows,
            Self::Msi(_) | Self::HidMaestro => RemovalMethod::Direct,
        }
    }
}

impl Component {
    pub(super) fn inspect(self) -> (DriverInfo, Option<Removal>) {
        match self {
            Self::HidMaestro => {
                let (status, error) = motion_core::hidmaestro::driver_status();
                let removal = matches!(status.as_str(), "running" | "partial" | "incompatible")
                    .then_some(Removal::HidMaestro);
                (DriverInfo { status, error }, removal)
            }
            Self::Usbip => {
                let services = ["usbip2_ude", "usbip2_filter"].map(components::driver);
                if let Some(info) = services
                    .iter()
                    .find(|info| matches!(info.status.as_str(), "restart_required" | "unsupported"))
                {
                    return (info.clone(), None);
                }
                let available = motion_core::dualsense::host_controller_available();
                let registration = usbip_registered();
                let status = if available {
                    "running"
                } else if matches!(registration, Ok(true))
                    || services
                        .iter()
                        .any(|info| matches!(info.status.as_str(), "running" | "stopped"))
                {
                    "unavailable"
                } else {
                    if let Err(error) = registration {
                        return (
                            DriverInfo {
                                status: "unknown".into(),
                                error: Some(error),
                            },
                            None,
                        );
                    }
                    if let Some(info) = services.iter().find(|info| info.status == "unknown") {
                        return (info.clone(), None);
                    }
                    "missing"
                };
                let removal = (status != "missing").then_some(Removal::Windows);
                (
                    DriverInfo {
                        status: status.into(),
                        error: None,
                    },
                    removal,
                )
            }
            Self::HidHide | Self::Vigem => {
                let mut info = components::driver(if self == Self::HidHide {
                    "HidHide"
                } else {
                    "ViGEmBus"
                });
                if info.status == "restart_required" {
                    return (info, None);
                }
                let removal = match registered_products(self) {
                    Ok(products) if !products.is_empty() => {
                        if info.status == "missing" {
                            info.status = "partial".into();
                            info.error = Some(
                                "Windows 仍登记了安装包，但驱动服务缺失。可先卸载，再重新安装。"
                                    .into(),
                            );
                        }
                        // If multiple versions coexist, let the user choose one in Windows.
                        match products.as_slice() {
                            [id] => Some(Removal::Msi(*id)),
                            _ => Some(Removal::Windows),
                        }
                    }
                    Err(error) if info.status == "missing" => {
                        info.status = "unknown".into();
                        info.error = Some(error);
                        Some(Removal::Windows)
                    }
                    _ if matches!(info.status.as_str(), "missing" | "unsupported") => None,
                    _ => Some(Removal::Windows),
                };
                (info, removal)
            }
        }
    }
}

// The official Inno Setup AppId survives a missing host controller or service.
#[cfg(windows)]
fn usbip_registered() -> Result<bool, String> {
    use windows_sys::Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS},
        System::Registry::{
            RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RRF_SUBKEY_WOW6432KEY,
            RRF_SUBKEY_WOW6464KEY,
        },
    };
    let key = windows_sys::core::w!(
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{199505b0-b93d-4521-a8c7-897818e0205a}_is1"
    );
    let mut result = Ok(false);
    for view in [RRF_SUBKEY_WOW6464KEY, RRF_SUBKEY_WOW6432KEY] {
        let mut size = 0;
        let code = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                key,
                windows_sys::core::w!("DisplayName"),
                RRF_RT_REG_SZ | view,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut size,
            )
        };
        match code {
            ERROR_SUCCESS => return Ok(true),
            ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => {}
            _ => result = Err(std::io::Error::from_raw_os_error(code as i32).to_string()),
        }
    }
    result
}
#[cfg(not(windows))]
fn usbip_registered() -> Result<bool, String> {
    Ok(false)
}

// Both Nefarius setup projects declare their original and current MSI upgrade families.
// Only use the machine product's own installed name. Never execute an ARP UninstallString.
#[cfg(windows)]
fn registered_products(component: Component) -> Result<Vec<Uuid>, String> {
    use windows_sys::Win32::{
        Foundation::{
            ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS, ERROR_UNKNOWN_PRODUCT,
            ERROR_UNKNOWN_PROPERTY,
        },
        System::ApplicationInstallationAndServicing::{
            MsiEnumRelatedProductsW, MsiGetProductInfoExW, INSTALLPROPERTY_INSTALLEDPRODUCTNAME,
            MSIINSTALLCONTEXT_MACHINE,
        },
    };
    let (families, names): (&[&str], &[&str]) = match component {
        Component::HidHide => (
            &[
                "{8822CC70-E2A5-4CB7-8F14-E27101150A1D}",
                "{88FDE3F4-0C3A-4462-9FE0-5C25648A1026}",
            ],
            &["HidHide", "HidHide Drivers", "Nefarius HidHide"],
        ),
        Component::Vigem => (
            &[
                "{67175F6C-AA18-43A7-AE60-2FC3FD10BF79}",
                "{0A4A02DE-0BE3-4BF4-91F0-1EA47AD26881}",
            ],
            &[
                "ViGEm Bus Driver",
                "Virtual Gamepad Emulation Bus Driver",
                "Nefarius Virtual Gamepad Emulation Bus Driver",
            ],
        ),
        _ => return Err("Unknown MSI component".into()),
    };
    let mut products = Vec::new();
    for family in families {
        let family: Vec<u16> = family.encode_utf16().chain(Some(0)).collect();
        for index in 0.. {
            let mut code = [0u16; 39];
            match unsafe { MsiEnumRelatedProductsW(family.as_ptr(), 0, index, code.as_mut_ptr()) } {
                ERROR_NO_MORE_ITEMS => break,
                ERROR_SUCCESS => {}
                error => return Err(std::io::Error::from_raw_os_error(error as i32).to_string()),
            }
            let mut name = [0u16; 128];
            let mut len = name.len() as u32;
            match unsafe {
                MsiGetProductInfoExW(
                    code.as_ptr(),
                    std::ptr::null(),
                    MSIINSTALLCONTEXT_MACHINE,
                    INSTALLPROPERTY_INSTALLEDPRODUCTNAME,
                    name.as_mut_ptr(),
                    &mut len,
                )
            } {
                ERROR_SUCCESS => {}
                ERROR_UNKNOWN_PRODUCT | ERROR_UNKNOWN_PROPERTY | ERROR_MORE_DATA => continue,
                error => return Err(std::io::Error::from_raw_os_error(error as i32).to_string()),
            }
            let name = String::from_utf16_lossy(&name[..len as usize]);
            if names
                .iter()
                .any(|expected| name.eq_ignore_ascii_case(expected))
            {
                let id = Uuid::parse_str(&String::from_utf16_lossy(&code[..38]))
                    .map_err(|e| e.to_string())?;
                if !products.contains(&id) {
                    products.push(id);
                }
            }
        }
    }
    Ok(products)
}

#[cfg(not(windows))]
fn registered_products(_: Component) -> Result<Vec<Uuid>, String> {
    Ok(Vec::new())
}

pub(super) fn uninstall(
    app: &AppHandle,
    state: &crate::AppState,
    kind: Component,
    method: RemovalMethod,
) -> Result<installers::InstallResult, String> {
    let _gate = components::GATE
        .lock()
        .map_err(|_| "Component control lock failed")?;
    let (_, plan) = kind.inspect();
    let plan = plan
        .filter(|plan| plan.method() == method)
        .ok_or("组件的安装状态已变化，请重新检测后再卸载。")?;
    if kind == Component::HidHide {
        let visibility = state
            .client
            .snapshot
            .lock()
            .map_err(crate::text)?
            .status
            .visibility
            .clone();
        if visibility.requested {
            return Err("请先停止映射或观察，再卸载组件。".into());
        }
        let journal = state
            .store
            .root
            .join(motion_core::visibility::SESSION_JOURNAL);
        if visibility.active || journal.try_exists().map_err(crate::text)? {
            state
                .client
                .request(motion_core::protocol::Command::RecoverVisibility)
                .map_err(|error| {
                    format!("卸载前恢复手柄可见性失败。请在「手动管理隐藏规则」中重试。{error}")
                })?;
        }
    }
    match plan {
        Removal::Msi(id) => installers::uninstall_product(id),
        Removal::HidMaestro => {
            let engine = runtime::engine_path(app)?;
            let _verified = installers::verify_engine(&engine)?;
            installers::hidmaestro_result(motion_core::hidmaestro::uninstall_driver(&engine))
        }
        Removal::Windows => {
            open::that("ms-settings:appsfeatures").map_err(|e| e.to_string())?;
            installers::result(0)
        }
    }
}
