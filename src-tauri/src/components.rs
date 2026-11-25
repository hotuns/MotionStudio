use crate::{
    component_removal::{Component, RemovalMethod},
    runtime,
};
use motion_core::visibility::{policy::DriverState, VisibilityStatus};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};
use tauri::AppHandle;

pub(super) static GATE: Mutex<()> = Mutex::new(());
const OWNED_FILE: &str = "hidhide-motion-rules.json";
const LIMIT: usize = 2 * 1024 * 1024;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DriverInfo {
    pub status: String,
    pub error: Option<String>,
}
#[derive(Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct InventoryDevice {
    present: bool,
    gaming_device: bool,
    description: String,
    product: String,
    device_instance_path: String,
    xusb_device_instance_path: String,
    base_container_device_instance_path: String,
    base_container_device_count: u32,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InventoryGroup {
    friendly_name: String,
    devices: Vec<InventoryDevice>,
}
#[derive(Default)]
struct Snapshot {
    cloak: bool,
    inverse: bool,
    hidden: Vec<String>,
    apps: Vec<String>,
    groups: Vec<InventoryGroup>,
}
#[derive(Default, Deserialize, Serialize)]
#[serde(default)]
struct Owned {
    groups: BTreeMap<String, Vec<String>>,
    apps: Vec<String>,
    added_devices: Vec<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HidHideDevice {
    pub id: String,
    pub name: String,
    pub description: String,
    pub listed: bool,
    pub managed: bool,
    pub xinput_available: bool,
    pub shared_functions: u32,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentsStatus {
    pub supported: bool,
    pub hidhide: DriverInfo,
    pub vigem: DriverInfo,
    pub hidmaestro: DriverInfo,
    pub usbip: DriverInfo,
    pub uninstall_methods: BTreeMap<Component, RemovalMethod>,
    pub cli_available: bool,
    pub control_available: bool,
    pub hidhide_session: bool,
    pub cloak_enabled: bool,
    pub inverse: bool,
    pub engine_allowed: bool,
    pub engine_path: String,
    pub hidden_count: usize,
    pub managed_count: usize,
    pub devices: Vec<HidHideDevice>,
    pub error: Option<String>,
}
fn has(list: &[String], path: &str) -> bool {
    list.iter().any(|p| p.eq_ignore_ascii_case(path))
}
// Tauri can resolve bundled executables with a verbatim Windows path, while
// HidHide's app-list converts them back to DOS/UNC paths. Normalize applications
// only: HID device instance IDs must retain their original spelling.
fn application_path(path: &str) -> String {
    let path = path.replace('/', "\\");
    if path
        .get(..8)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(r"\\?\UNC\"))
    {
        format!(r"\\{}", &path[8..])
    } else if path.starts_with(r"\\?\") || path.starts_with(r"\??\") {
        path[4..].to_string()
    } else {
        path
    }
}
fn has_application(list: &[String], path: &str) -> bool {
    let expected = application_path(path);
    list.iter()
        .any(|entry| application_path(entry).eq_ignore_ascii_case(&expected))
}
fn push_unique(list: &mut Vec<String>, value: String) {
    if !has(list, &value) {
        list.push(value);
    }
}
fn record_hide(owned: &mut Owned, before: &Snapshot, id: &str, paths: &[String]) {
    for path in paths {
        if !has(&before.hidden, path) {
            push_unique(&mut owned.added_devices, path.clone());
        }
        push_unique(
            owned.groups.entry(id.to_string()).or_default(),
            path.clone(),
        );
    }
}
fn restore_plan(owned: &Owned, device_id: Option<&str>) -> (Vec<String>, Vec<String>) {
    let ids: Vec<_> = owned
        .groups
        .keys()
        .filter(|id| device_id.is_none_or(|target| id.eq_ignore_ascii_case(target)))
        .cloned()
        .collect();
    let mut paths = Vec::new();
    for id in &ids {
        if let Some(group) = owned.groups.get(id) {
            for p in group {
                push_unique(&mut paths, p.clone());
            }
        }
    }
    if device_id.is_none() {
        for p in &owned.added_devices {
            push_unique(&mut paths, p.clone());
        }
    }
    paths.retain(|p| {
        has(&owned.added_devices, p)
            && !owned
                .groups
                .iter()
                .any(|(id, group)| !ids.contains(id) && has(group, p))
    });
    (ids, paths)
}

fn parse_list(text: &str, command: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| line.trim().strip_prefix(command))
        .map(|value| value.trim().trim_matches('"').to_string())
        .filter(|p| !p.is_empty())
        .collect()
}
fn parse_snapshot(text: &str) -> Result<Snapshot, String> {
    let lines: Vec<_> = text.lines().map(str::trim).collect();
    let cloak = if lines.contains(&"--cloak-on") {
        true
    } else if lines.contains(&"--cloak-off") {
        false
    } else {
        return Err("HidHide did not return its enabled state".into());
    };
    let inverse = if lines.contains(&"--inv-on") {
        true
    } else if lines.contains(&"--inv-off") {
        false
    } else {
        return Err("HidHide did not return its application mode".into());
    };
    let start = text
        .char_indices()
        .find(|(i, c)| {
            *c == '['
                && text[..*i]
                    .rsplit('\n')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .is_empty()
        })
        .map(|(i, _)| i)
        .ok_or("HidHide did not return a device inventory")?;
    let groups: Vec<InventoryGroup> = serde_json::from_str(&text[start..])
        .map_err(|e| format!("Invalid HidHide inventory: {e}"))?;
    Ok(Snapshot {
        cloak,
        inverse,
        hidden: parse_list(text, "--dev-hide "),
        apps: parse_list(text, "--app-reg "),
        groups,
    })
}
fn physical(device: &InventoryDevice) -> bool {
    let joined = format!(
        "{} {} {}",
        device.device_instance_path, device.base_container_device_instance_path, device.description
    )
    .to_uppercase();
    device.present
        && device.gaming_device
        && device
            .device_instance_path
            .to_uppercase()
            .starts_with("HID\\")
        && !["VIGEM", "VIRTUAL", "HIDMAESTRO", "ROOT\\"]
            .iter()
            .any(|s| joined.contains(s))
}
fn find_device<'a>(snapshot: &'a Snapshot, id: &str) -> Result<&'a InventoryDevice, String> {
    snapshot
        .groups
        .iter()
        .flat_map(|g| &g.devices)
        .find(|d| physical(d) && d.device_instance_path.eq_ignore_ascii_case(id))
        .ok_or_else(|| {
            "The selected physical controller is no longer available. Refresh devices.".into()
        })
}
fn device_paths(device: &InventoryDevice, xinput: bool) -> Result<Vec<String>, String> {
    let mut result = vec![device.device_instance_path.clone()];
    if xinput {
        if device.xusb_device_instance_path.is_empty() {
            return Err("This device has no identified XInput companion node. Use the standard hide option.".into());
        }
        push_unique(&mut result, device.xusb_device_instance_path.clone());
        if device
            .base_container_device_instance_path
            .to_uppercase()
            .starts_with("USB\\")
        {
            push_unique(
                &mut result,
                device.base_container_device_instance_path.clone(),
            );
        }
    }
    Ok(result)
}
fn owned_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(runtime::runtime_dir(app)?.join(OWNED_FILE))
}
fn load_owned(app: &AppHandle) -> Result<Owned, String> {
    match fs::read(owned_path(app)?) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|e| format!("Cannot read Studio HidHide recovery rules: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Owned::default()),
        Err(e) => Err(e.to_string()),
    }
}
fn save_owned(app: &AppHandle, owned: &Owned) -> Result<(), String> {
    let target = owned_path(app)?;
    motion_core::storage::atomic_write(
        &target,
        &serde_json::to_vec_pretty(owned).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
fn cli_path() -> Option<PathBuf> {
    tool_path("HidHideCLI.exe")
}
fn tool_path(executable: &str) -> Option<PathBuf> {
    for env in ["ProgramW6432", "ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(root) = std::env::var_os(env) {
            for vendor in [
                "Nefarius Software Solutions",
                "Nefarius Software Solutions e.U.",
                "Nefarius Software Solutions e.U",
            ] {
                for folder in ["", "x64", "x86"] {
                    let path = PathBuf::from(&root)
                        .join(vendor)
                        .join("HidHide")
                        .join(folder)
                        .join(executable);
                    if path.is_file() {
                        return Some(path);
                    }
                }
            }
        }
    }
    None
}
fn decode(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xff, 0xfe])
        || bytes
            .iter()
            .skip(1)
            .step_by(2)
            .take(30)
            .filter(|b| **b == 0)
            .count()
            > 5
    {
        let bytes = if bytes.starts_with(&[0xff, 0xfe]) {
            &bytes[2..]
        } else {
            bytes
        };
        return String::from_utf16_lossy(
            &bytes
                .as_chunks::<2>()
                .0
                .iter()
                .copied()
                .map(u16::from_le_bytes)
                .collect::<Vec<_>>(),
        );
    }
    String::from_utf8_lossy(bytes)
        .trim_start_matches('\u{feff}')
        .to_string()
}
fn run_cli(path: &Path, args: &[String]) -> Result<String, String> {
    let mut command = Command::new(path);
    command
        .args(args)
        .current_dir(path.parent().ok_or("Invalid HidHide path")?)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("Could not start HidHide CLI: {e}"))?;
    let stdout = child.stdout.take().ok_or("Missing CLI output")?;
    let stderr = child.stderr.take().ok_or("Missing CLI error output")?;
    let read = |stream: Box<dyn Read + Send>| {
        thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = stream.take((LIMIT + 1) as u64).read_to_end(&mut bytes);
            bytes
        })
    };
    let out = read(Box::new(stdout));
    let err = read(Box::new(stderr));
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() < Duration::from_secs(10) => {
                thread::sleep(Duration::from_millis(25))
            }
            result => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = out.join();
                let _ = err.join();
                return Err(format!(
                    "HidHide CLI timed out or could not be read: {result:?}"
                ));
            }
        }
    };
    let output = out.join().map_err(|_| "HidHide output reader failed")?;
    let error = err.join().map_err(|_| "HidHide error reader failed")?;
    if output.len() > LIMIT || error.len() > LIMIT {
        return Err("HidHide output exceeded its limit".into());
    }
    if !status.success() {
        return Err(format!(
            "HidHide CLI failed ({}). Close HidHide Configuration Client and retry. {}",
            status.code().unwrap_or(-1),
            decode(&error).trim()
        ));
    }
    Ok(decode(&output))
}
fn snapshot(path: &Path) -> Result<Snapshot, String> {
    let output = run_cli(
        path,
        &[
            "--cloak-state",
            "--inv-state",
            "--dev-list",
            "--app-list",
            "--dev-gaming",
        ]
        .map(String::from),
    )?;
    if let Ok(value) = parse_snapshot(&output) {
        return Ok(value);
    }
    // Old CLI builds can stop printing at a non-ASCII device description.
    // Never salvage incomplete JSON or treat truncated records as complete devices.
    let flags = run_cli(
        path,
        &["--cloak-state", "--inv-state", "--dev-list", "--app-list"].map(String::from),
    )?;
    let inventory = super::hid_inventory::enumerate()?;
    parse_snapshot(&format!("{flags}\n{inventory}"))
}

#[cfg(windows)]
fn service_pending_removal(service: &str) -> bool {
    use windows_sys::Win32::{
        Foundation::ERROR_SUCCESS,
        System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD},
    };
    // PnP can leave a live kernel driver queryable after unregistering its package.
    let key: Vec<u16> = format!(r"SYSTEM\CurrentControlSet\Services\{service}")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut flag = 0u32;
    let mut size = std::mem::size_of_val(&flag) as u32;
    let result = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            windows_sys::core::w!("DeleteFlag"),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            std::ptr::from_mut(&mut flag).cast(),
            &mut size,
        )
    };
    result == ERROR_SUCCESS && flag == 1
}

#[cfg(windows)]
pub(super) fn driver(service: &str) -> DriverInfo {
    use windows_sys::Win32::{
        Foundation::{GetLastError, ERROR_SERVICE_DOES_NOT_EXIST, ERROR_SERVICE_MARKED_FOR_DELETE},
        System::Services::*,
    };
    let pending_removal = || DriverInfo {
        status: "restart_required".into(),
        error: Some("Windows 正在移除驱动。请重启后重新检测。".into()),
    };
    if service_pending_removal(service) {
        return pending_removal();
    }
    let service: Vec<u16> = service.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let manager = OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT);
        if manager.is_null() {
            return DriverInfo {
                status: "unknown".into(),
                error: Some(std::io::Error::last_os_error().to_string()),
            };
        }
        let handle = OpenServiceW(manager, service.as_ptr(), SERVICE_QUERY_STATUS);
        if handle.is_null() {
            let code = GetLastError();
            CloseServiceHandle(manager);
            return match code {
                ERROR_SERVICE_MARKED_FOR_DELETE => pending_removal(),
                ERROR_SERVICE_DOES_NOT_EXIST => DriverInfo {
                    status: "missing".into(),
                    error: None,
                },
                _ => DriverInfo {
                    status: "unknown".into(),
                    error: Some(format!("Windows error {code}")),
                },
            };
        }
        let mut status: SERVICE_STATUS_PROCESS = std::mem::zeroed();
        let mut needed = 0;
        let ok = QueryServiceStatusEx(
            handle,
            SC_STATUS_PROCESS_INFO,
            &mut status as *mut _ as *mut u8,
            std::mem::size_of_val(&status) as u32,
            &mut needed,
        );
        let error = if ok == 0 {
            Some(std::io::Error::last_os_error().to_string())
        } else {
            None
        };
        CloseServiceHandle(handle);
        CloseServiceHandle(manager);
        DriverInfo {
            status: if ok == 0 {
                "unknown"
            } else if status.dwCurrentState == SERVICE_RUNNING {
                "running"
            } else {
                "stopped"
            }
            .into(),
            error,
        }
    }
}
#[cfg(not(windows))]
pub(super) fn driver(_: &str) -> DriverInfo {
    DriverInfo {
        status: "unsupported".into(),
        error: None,
    }
}

enum HideInspection {
    State(DriverState),
    InUse,
    Failed { code: Option<u32>, message: String },
}
fn inspect_hidhide() -> HideInspection {
    #[cfg(windows)]
    {
        match motion_core::visibility::inspect() {
            Ok(Some(state)) => HideInspection::State(state),
            Ok(None) => HideInspection::InUse,
            Err(error) => HideInspection::Failed {
                code: error.code,
                message: error.message,
            },
        }
    }
    #[cfg(not(windows))]
    {
        HideInspection::Failed {
            code: None,
            message: "HidHide requires Windows".into(),
        }
    }
}
struct HideHealth {
    driver: DriverInfo,
    policy: Option<DriverState>,
    session: bool,
}
// Settings and setup use this one health result. CLI presence is only relevant
// to the optional manual controls, never to automatic mapping readiness.
fn hidhide_health(
    service: DriverInfo,
    visibility: Option<&VisibilityStatus>,
    read: impl FnOnce() -> HideInspection,
) -> HideHealth {
    let report = |status: &str, error: Option<String>, policy, session| HideHealth {
        driver: DriverInfo {
            status: status.into(),
            error,
        },
        policy,
        session,
    };
    let session = visibility.filter(|v| v.requested || v.active);
    if service.status == "restart_required"
        || (session.is_none()
            && matches!(service.status.as_str(), "missing" | "partial" | "unsupported"))
    {
        return HideHealth {
            driver: service,
            policy: None,
            session: session.is_some(),
        };
    }
    if let Some(session) = session {
        return report(
            if session.error.is_some() {
                "unknown"
            } else if session.active {
                "running"
            } else {
                "pending"
            },
            session.error.clone(),
            None,
            true,
        );
    }
    match read() {
        HideInspection::State(state) => {
            let error = state
                .inverse
                .then(|| "HidHide 正在使用反向应用列表，请在官方管理工具中恢复普通模式。".into());
            report(
                if state.inverse {
                    "configuration"
                } else {
                    "running"
                },
                error,
                Some(state),
                false,
            )
        }
        HideInspection::InUse => report(
            "busy",
            Some("隐藏会话正在运行或恢复，请稍后重新检测。".into()),
            None,
            true,
        ),
        HideInspection::Failed { code, message } => {
            let (status, detail) = match code {
                Some(32 | 33) => ("busy", "HidHide 正被其他程序占用，请关闭官方配置工具后重试。".into()),
                Some(5) => ("permission_denied", "Windows 拒绝访问 HidHide 驱动，请检查系统权限或修复驱动安装。".into()),
                Some(2 | 3) if service.status == "stopped" => ("stopped", "HidHide 已安装但驱动尚未运行，请重启 Windows 后重新检测。".into()),
                Some(2 | 3) => ("control_unavailable", "HidHide 已安装但控制接口不可用，请重启 Windows 后重新检测；仍异常时修复驱动安装。".into()),
                _ => ("unknown", format!("HidHide 配置读取失败：{message}")),
            };
            report(status, Some(detail), None, false)
        }
    }
}
fn status_inner(
    app: &AppHandle,
    visibility: Option<&VisibilityStatus>,
) -> Result<ComponentsStatus, String> {
    let engine = application_path(&runtime::engine_path(app)?.to_string_lossy());
    let cli = cli_path();
    let owned = load_owned(app)?;
    let mut uninstall_methods = BTreeMap::new();
    let mut inspect = |kind: Component| {
        let (info, removal) = kind.inspect();
        if let Some(removal) = removal {
            uninstall_methods.insert(kind, removal.method());
        }
        info
    };
    let health = hidhide_health(inspect(Component::HidHide), visibility, inspect_hidhide);
    let mut result = ComponentsStatus {
        supported: cfg!(windows),
        hidhide: health.driver,
        vigem: inspect(Component::Vigem),
        hidmaestro: inspect(Component::HidMaestro),
        usbip: inspect(Component::Usbip),
        uninstall_methods,
        cli_available: cli.is_some(),
        control_available: health.policy.is_some(),
        hidhide_session: health.session,
        cloak_enabled: false,
        inverse: false,
        engine_allowed: false,
        engine_path: engine.clone(),
        hidden_count: 0,
        managed_count: owned.added_devices.len(),
        devices: vec![],
        error: None,
    };
    if let Some(s) = health.policy {
        result.cloak_enabled = s.active;
        result.inverse = s.inverse;
        #[cfg(windows)]
        {
            result.engine_allowed = !s.inverse
                && motion_core::visibility::native_image_path(Path::new(&engine))
                    .is_ok_and(|path| has(&s.apps, &path));
        }
        result.hidden_count = s.hidden.len();
        // Manual inventory is independent of driver access. The native
        // handle and lease guard have already been released here.
        let groups = if cli.is_some() {
            super::hid_inventory::enumerate().and_then(|json| {
                serde_json::from_str::<Vec<InventoryGroup>>(&json).map_err(|e| e.to_string())
            })
        } else {
            Ok(Vec::new())
        };
        let groups = match groups {
            Ok(groups) => groups,
            Err(error) => {
                result.error = Some(error);
                Vec::new()
            }
        };
        for group in &groups {
            for d in group.devices.iter().filter(|d| physical(d)) {
                result.devices.push(HidHideDevice {
                    id: d.device_instance_path.clone(),
                    name: group.friendly_name.clone(),
                    description: if d.product.is_empty() {
                        d.description.clone()
                    } else {
                        d.product.clone()
                    },
                    listed: has(&s.hidden, &d.device_instance_path)
                        || (!d.xusb_device_instance_path.is_empty()
                            && has(&s.hidden, &d.xusb_device_instance_path))
                        || (!d.base_container_device_instance_path.is_empty()
                            && has(&s.hidden, &d.base_container_device_instance_path)),
                    managed: owned.groups.iter().any(|(id, paths)| {
                        id.eq_ignore_ascii_case(&d.device_instance_path)
                            && paths
                                .iter()
                                .any(|p| has(&s.hidden, p) && has(&owned.added_devices, p))
                    }),
                    xinput_available: !d.xusb_device_instance_path.is_empty(),
                    shared_functions: d.base_container_device_count,
                });
            }
        }
    }
    Ok(result)
}
pub fn get_status(
    app: &AppHandle,
    visibility: &VisibilityStatus,
) -> Result<ComponentsStatus, String> {
    let _gate = GATE.lock().map_err(|_| "Component control lock failed")?;
    status_inner(app, Some(visibility))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HidHideRepairResult {
    message: String,
    reboot_required: bool,
}

#[cfg(any(windows, test))]
fn repair_hide_policy(
    health: HideHealth,
    needs_recovery: bool,
    recover: impl FnOnce() -> Result<(), String>,
    authorize: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    if health.driver.status != "running"
        && !(needs_recovery && (health.policy.is_some() || health.session))
    {
        return Err(health.driver.error.unwrap_or_else(|| {
            "HidHide 尚未就绪，请查看组件管理中的检测结果。".into()
        }));
    }
    if needs_recovery {
        recover()?;
    }
    authorize()
}

#[cfg(windows)]
pub fn repair_hidhide(app: &AppHandle, state: &crate::AppState) -> Result<HidHideRepairResult, String> {
    if Component::HidHide.inspect().0.status == "missing" {
        let install = super::installers::install(app, "hidhide")?;
        if install.cancelled || install.reboot_required {
            return Ok(HidHideRepairResult {
                message: if install.cancelled {
                    "已取消 HidHide 修复。"
                } else {
                    "HidHide 已安装，请重启 Windows 后再次运行修复。"
                }.into(),
                reboot_required: install.reboot_required,
            });
        }
    }
    let _gate = GATE.lock().map_err(|_| "Component control lock failed")?;
    let health = hidhide_health(Component::HidHide.inspect().0, None, inspect_hidhide);
    let journal = state.store.root.join(motion_core::visibility::SESSION_JOURNAL);
    let journal_pending = journal.try_exists().map_err(crate::text)?;
    let (engine_ready, navigating, visibility) = {
        let snapshot = state.client.snapshot.lock().map_err(crate::text)?;
        (snapshot.engine_ready, snapshot.status.ui_navigation, snapshot.status.visibility.clone())
    };
    repair_hide_policy(
        health,
        journal_pending || visibility.active || visibility.error.is_some(),
        || {
            if !engine_ready {
                return Err("请先重启引擎，再恢复遗留的自动隐藏规则。".into());
            }
            state.client.request(motion_core::protocol::Command::RecoverVisibility)?;
            if journal.try_exists().map_err(crate::text)? {
                return Err("自动隐藏恢复尚未完成，请再次运行 HidHide 修复。".into());
            }
            Ok(())
        },
        || {
            let path = runtime::engine_path(app)?;
            let _verified = super::installers::verify_engine(&path)?;
            motion_core::visibility::authorize_engine(&path).map_err(|error| format!("{error:#}"))
        },
    )?;
    if engine_ready && !navigating {
        state.client.request(motion_core::protocol::Command::Scan).map_err(|error| {
            format!("HidHide 读取权限已确认，重新检测手柄失败：{error}")
        })?;
    }
    Ok(HidHideRepairResult {
        message: if journal_pending || visibility.active {
            "已恢复上次自动隐藏遗留的规则，并确认引擎读取权限。"
        } else {
            "HidHide 已就绪，引擎读取权限已确认。"
        }.into(),
        reboot_required: false,
    })
}

#[cfg(not(windows))]
pub fn repair_hidhide(_: &AppHandle, _: &crate::AppState) -> Result<HidHideRepairResult, String> {
    Err("HidHide requires Windows".into())
}

pub fn open_tool(kind: &str) -> Result<(), String> {
    if !cfg!(windows) {
        return Err("Component tools require Windows".into());
    }
    let mut command = match kind {
        "hidhide" => {
            let path = tool_path("HidHideClient.exe").ok_or("未找到 HidHide 官方管理工具。自动隐藏不需要此工具；请直接启动映射，由软件管理隐藏与恢复。")?;
            Command::new(path)
        }
        "gamepads" => {
            let system = std::env::var_os("SystemRoot").ok_or("Windows directory was not found")?;
            let mut command = Command::new(PathBuf::from(system).join("System32/control.exe"));
            command.arg("joy.cpl");
            command
        }
        _ => return Err("Unknown component tool".into()),
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command.spawn().map_err(|e| e.to_string())?;
    Ok(())
}
pub fn manage(
    app: &AppHandle,
    action: &str,
    device_id: &str,
    xinput: bool,
) -> Result<ComponentsStatus, String> {
    let _gate = GATE.lock().map_err(|_| "Component control lock failed")?;
    let mut owned = load_owned(app)?;
    if action == "restore" && owned.groups.is_empty() && owned.added_devices.is_empty() {
        return status_inner(app, None);
    }
    let cli = cli_path().ok_or("Install HidHide first, then refresh")?;
    let before = snapshot(&cli)?;
    let mut args = Vec::new();
    match action {
        "hide" | "allow" => {
            if before.inverse {
                return Err("HidHide uses an inverted application list. Restore normal mode in the official client before using quick controls; Studio will not change existing mode.".into());
            }
            let engine = runtime::engine_path(app)?;
            if !engine.is_file() {
                return Err("Motion Studio 引擎文件缺失，请重新安装".into());
            }
            let engine = application_path(&engine.to_string_lossy());
            let paths = if action == "hide" {
                device_paths(find_device(&before, device_id)?, xinput)?
            } else {
                Vec::new()
            };
            let engines = vec![engine.clone()];
            for path in engines {
                if !has_application(&before.apps, &path) && !has_application(&owned.apps, &path) {
                    owned.apps.push(path.clone());
                }
                args.extend(["--app-reg".into(), path]);
            }
            if action == "hide" {
                record_hide(&mut owned, &before, device_id, &paths);
                for p in &paths {
                    args.extend(["--dev-hide".into(), p.clone()]);
                }
                args.push("--cloak-on".into());
            }
            // Persist recovery information before any write to the driver.
            save_owned(app, &owned)?;
            run_cli(&cli, &args)?;
            let after = snapshot(&cli)?;
            if after.inverse || !has_application(&after.apps, &engine) {
                return Err("HidHide 尚未确认引擎访问权限，请刷新或恢复本应用的规则。".into());
            }
            if action == "hide" && (!after.cloak || paths.iter().any(|p| !has(&after.hidden, p))) {
                return Err("HidHide did not confirm the requested hide state. Refresh or restore Studio rules.".into());
            }
        }
        "show" | "restore" => {
            let (ids, remove) = restore_plan(
                &owned,
                if action == "restore" {
                    None
                } else {
                    Some(device_id)
                },
            );
            for p in &remove {
                args.extend(["--dev-unhide".into(), p.clone()]);
            }
            if !args.is_empty() {
                run_cli(&cli, &args)?;
            }
            let after = snapshot(&cli)?;
            if remove.iter().any(|p| has(&after.hidden, p)) {
                return Err(
                    "HidHide did not confirm restore. Recovery rules were retained; retry.".into(),
                );
            }
            for id in ids {
                owned.groups.remove(&id);
            }
            owned.added_devices.retain(|p| !has(&remove, p));
            // Keep allow-list entries so another still-hidden device cannot lock JSM out.
            save_owned(app, &owned)?;
        }
        _ => return Err("Unknown component control action".into()),
    }
    status_inner(app, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn service(status: &str) -> DriverInfo {
        DriverInfo {
            status: status.into(),
            error: None,
        }
    }
    #[test]
    fn native_hidhide_readiness_does_not_require_tools_or_prior_rules() {
        let health = hidhide_health(service("running"), None, || {
            HideInspection::State(DriverState::default())
        });
        assert_eq!(health.driver.status, "running");
        assert!(health.policy.is_some());
        assert!(!health.session);
    }
    #[test]
    fn hidhide_detection_does_not_open_control_during_mapping() {
        let mut visibility = VisibilityStatus {
            requested: true,
            active: true,
            ..Default::default()
        };
        let health = hidhide_health(service("running"), Some(&visibility), || {
            panic!("must reuse the active lease")
        });
        assert_eq!(health.driver.status, "running");
        assert!(health.session && health.policy.is_none());
        let removed = hidhide_health(service("restart_required"), Some(&visibility), || panic!());
        assert_eq!(removed.driver.status, "restart_required");
        assert!(removed.session && removed.policy.is_none());
        visibility.active = false;
        assert_eq!(
            hidhide_health(service("running"), Some(&visibility), || panic!())
                .driver
                .status,
            "pending"
        );
        visibility.error = Some("read failure".into());
        let failed = hidhide_health(service("running"), Some(&visibility), || panic!());
        assert_eq!(failed.driver.status, "unknown");
        assert_eq!(failed.driver.error.as_deref(), Some("read failure"));
    }
    #[test]
    fn hidhide_service_presence_cannot_mask_access_or_policy_failures() {
        for (code, expected) in [
            (32, "busy"),
            (5, "permission_denied"),
            (2, "control_unavailable"),
            (1234, "unknown"),
        ] {
            let health = hidhide_health(service("running"), None, || HideInspection::Failed {
                code: Some(code),
                message: "probe failed".into(),
            });
            assert_eq!(health.driver.status, expected);
            assert!(health.driver.error.is_some() && health.policy.is_none());
        }
        let inverse = hidhide_health(service("running"), None, || {
            HideInspection::State(DriverState {
                inverse: true,
                ..Default::default()
            })
        });
        assert_eq!(inverse.driver.status, "configuration");
        let busy = hidhide_health(service("running"), None, || HideInspection::InUse);
        assert_eq!(busy.driver.status, "busy");
        let missing = hidhide_health(service("missing"), None, || {
            panic!("must not invoke control or CLI")
        });
        assert_eq!(missing.driver.status, "missing");
        let stopped = hidhide_health(service("stopped"), None, || HideInspection::Failed {
            code: Some(2),
            message: "absent".into(),
        });
        assert_eq!(stopped.driver.status, "stopped");
    }
    #[test]
    fn hidhide_repair_recovers_a_pending_lease_before_allowing_the_engine() {
        let health = hidhide_health(service("running"), None, || HideInspection::InUse);
        let recovered = std::cell::Cell::new(false);
        repair_hide_policy(
            health,
            true,
            || {
                recovered.set(true);
                Ok(())
            },
            || {
                assert!(recovered.get());
                Ok(())
            },
        )
        .unwrap();
    }
    #[test]
    fn hidhide_repair_returns_the_driver_error_when_an_external_client_holds_it() {
        let health = hidhide_health(service("running"), None, || HideInspection::Failed {
            code: Some(32),
            message: "sharing violation".into(),
        });
        let error = health.driver.error.clone().unwrap();
        assert_eq!(
            repair_hide_policy(health, true, || panic!(), || panic!()),
            Err(error)
        );
    }
    #[test]
    fn hidhide_repair_stops_if_pending_rules_could_not_be_restored() {
        let health = hidhide_health(service("running"), None, || {
            HideInspection::State(DriverState::default())
        });
        assert_eq!(
            repair_hide_policy(health, true, || Err("restore failed".into()), || panic!()),
            Err("restore failed".into())
        );
    }
    #[test]
    #[ignore = "manual system integration only; CLI may register itself in the allow list"]
    fn manual_windows_driver_and_cli_probe() {
        for name in ["HidHide", "ViGEmBus"] {
            let result = driver(name);
            eprintln!("{name}: {}", result.status);
            assert!(["running", "missing", "stopped", "unknown", "unsupported", "restart_required"]
                .contains(&result.status.as_str()));
        }
        if let Some(path) = cli_path() {
            match snapshot(&path) {
                Ok(s) => eprintln!(
                    "HidHide CLI manual probe: {} device groups",
                    s.groups.len()
                ),
                Err(e) => eprintln!("HidHide CLI manual probe unavailable: {e}"),
            }
        }
    }
    fn inventory() -> String {
        r#"--cloak-on
--inv-off
--app-reg "C:\工具\JoyShockMapper.exe"
--dev-hide "HID\VID_1234&PID_5678\one"
[{"friendlyName":"测试手柄","devices":[{"present":true,"gamingDevice":true,"deviceInstancePath":"HID\\VID_1234&PID_5678\\one","xusbDeviceInstancePath":"USB\\VID_1234&PID_5678\\xusb","baseContainerDeviceInstancePath":"USB\\VID_1234&PID_5678\\receiver","baseContainerDeviceCount":3}]}]"#.into()
    }
    #[test]
    fn reads_real_cli_flags_lists_and_unicode_json() {
        let s = parse_snapshot(&inventory()).unwrap();
        assert!(s.cloak);
        assert!(!s.inverse);
        assert_eq!(s.groups[0].friendly_name, "测试手柄");
        assert!(has(&s.apps, r"c:\工具\joyshockmapper.exe"));
        assert_eq!(s.hidden.len(), 1);
    }
    #[test]
    fn confirms_verbatim_engine_paths_against_hidhide_dos_output() {
        let s = parse_snapshot(&inventory()).unwrap();
        let engine = r"\\?\C:\工具\JoyShockMapper.exe";
        assert!(
            !has(&s.apps, engine),
            "Regression: the former literal comparison failed"
        );
        assert!(has_application(&s.apps, engine));
        assert_eq!(application_path(engine), r"C:\工具\JoyShockMapper.exe");
        assert!(has_application(
            &[engine.into()],
            r"c:/工具/joyshockmapper.exe"
        ));
        assert!(has_application(&s.apps, r"\??\C:\工具\JoyShockMapper.exe"));
        assert!(!has_application(&s.apps, r"\\?\D:\工具\JoyShockMapper.exe"));
        assert!(!has_application(&s.apps, r"C:\其他\JoyShockMapper.exe"));
    }
    #[test]
    fn normalizes_unc_applications_without_changing_device_identity_matching() {
        let dos = r"\\server\share\JSM Studio\JoyShockMapper.exe";
        let extended = r"\\?\UNC\server\share\JSM Studio\JoyShockMapper.exe";
        assert_eq!(application_path(extended), dos);
        assert!(has_application(&[dos.into()], extended));
        assert!(has_application(&[extended.into()], dos));
        assert!(!has_application(
            &[dos.into()],
            r"\\other\share\JSM Studio\JoyShockMapper.exe"
        ));
        assert!(!has(
            &[r"HID\VID_1234&PID_5678\one".into()],
            r"\\?\HID\VID_1234&PID_5678\one"
        ));
    }
    #[test]
    fn rejects_incomplete_status_instead_of_assuming_safe_defaults() {
        assert!(parse_snapshot("[]").is_err());
        assert!(parse_snapshot("--cloak-on\n[]").is_err());
        assert!(parse_snapshot("--cloak-off\n--inv-off\nINVALID").is_err());
    }
    #[test]
    fn only_selected_physical_gaming_device_is_eligible() {
        let s = parse_snapshot(&inventory()).unwrap();
        assert!(find_device(&s, "missing").is_err());
        let mut d = s.groups[0].devices[0].clone();
        assert!(physical(&d));
        d.description = "Virtual Xbox".into();
        assert!(!physical(&d));
        d.description.clear();
        d.gaming_device = false;
        assert!(!physical(&d));
    }
    #[test]
    fn xinput_companions_require_explicit_opt_in() {
        let s = parse_snapshot(&inventory()).unwrap();
        let d = &s.groups[0].devices[0];
        assert_eq!(device_paths(d, false).unwrap().len(), 1);
        assert_eq!(device_paths(d, true).unwrap().len(), 3);
        let mut d = d.clone();
        d.xusb_device_instance_path.clear();
        assert!(device_paths(&d, true).is_err());
    }
    #[test]
    fn lists_preserve_spaces_and_quoted_paths() {
        assert_eq!(
            parse_list(
                "--app-reg \"C:\\Program Files\\JSM.exe\"\n--cloak-on",
                "--app-reg "
            ),
            vec![r"C:\Program Files\JSM.exe"]
        );
    }
    #[test]
    fn utf16_cli_output_is_supported() {
        let value = inventory();
        let bytes: Vec<_> = value.encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert_eq!(decode(&bytes), value);
    }
    #[test]
    fn rules_only_record_new_paths_and_survive_restart() {
        let mut o = Owned::default();
        o.groups.insert("pad".into(), vec!["owned".into()]);
        let restored: Owned = serde_json::from_slice(&serde_json::to_vec(&o).unwrap()).unwrap();
        assert_eq!(restored.groups["pad"], vec!["owned"]);
        assert!(!restored
            .groups
            .values()
            .flatten()
            .any(|v| v == "other-tool"));
    }
    #[test]
    fn restore_preserves_preexisting_paths_and_shared_receiver_dependencies() {
        let mut s = Snapshot {
            hidden: vec!["external".into()],
            ..Default::default()
        };
        let mut o = Owned::default();
        record_hide(
            &mut o,
            &s,
            "pad1",
            &["pad1".into(), "receiver".into(), "external".into()],
        );
        s.hidden.extend(["pad1".into(), "receiver".into()]);
        record_hide(&mut o, &s, "pad2", &["pad2".into(), "receiver".into()]);
        assert_eq!(restore_plan(&o, Some("pad1")).1, vec!["pad1"]);
        let all = restore_plan(&o, None).1;
        assert!(has(&all, "pad1"));
        assert!(has(&all, "pad2"));
        assert!(has(&all, "receiver"));
        assert!(!has(&all, "external"));
    }
    #[test]
    fn failed_hide_journal_remains_recoverable_even_if_device_disconnected() {
        let mut o = Owned::default();
        record_hide(&mut o, &Snapshot::default(), "gone", &["gone".into()]);
        let reloaded: Owned = serde_json::from_slice(&serde_json::to_vec(&o).unwrap()).unwrap();
        assert_eq!(restore_plan(&reloaded, None).1, vec!["gone"]);
    }
}
