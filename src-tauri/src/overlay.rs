//! Ordinary desktop notification surface. No game hooks or focus activation.
use motion_core::storage::{OverlayPosition, Settings};
use std::sync::{Arc, Condvar, Mutex};

#[derive(Clone, Debug)]
struct Notice {
    heading: &'static str,
    name: String,
    reason: String,
    position: OverlayPosition,
    duration: std::time::Duration,
}
fn label(value: &str) -> String {
    value.chars().filter(|c| !c.is_control()).take(80).collect()
}
#[derive(Default)]
struct Slot {
    pending: Option<Notice>,
    dismiss: bool,
    shutdown: bool,
    error: Option<String>,
}
type Shared = (Mutex<Slot>, Condvar);
#[derive(Clone)]
pub struct Overlay(Arc<Shared>);
impl Overlay {
    pub fn start() -> Self {
        let overlay = Self(Arc::new((Mutex::new(Slot::default()), Condvar::new())));
        let slot = overlay.0.clone();
        std::thread::spawn(move || {
            if let Err(error) = native::run(slot.clone()) {
                if let Ok(mut state) = slot.0.lock() {
                    state.error = Some(error);
                }
            }
        });
        overlay
    }
    pub fn show(&self, settings: &Settings, name: &str, reason: &str) -> Result<(), String> {
        if !settings.overlay_enabled {
            return Ok(());
        }
        let mut slot = self.0 .0.lock().map_err(|_| "配置提示暂不可用")?;
        if let Some(error) = &slot.error {
            return Err(error.clone());
        }
        if slot.shutdown {
            return Err("应用正在退出".into());
        }
        // A fast switch replaces the old notice and its deadline; never queue
        // stale profile names behind the configuration that is actually active.
        slot.dismiss = false;
        slot.pending = Some(Notice {
            heading: if settings.language == motion_core::storage::Language::En {
                "Motion Studio · Profile changed"
            } else {
                "Motion Studio · 配置已切换"
            },
            name: label(name),
            reason: label(if settings.language == motion_core::storage::Language::En {
                match reason {
                    "快捷键切换" => "Keyboard shortcut",
                    "手柄快捷键切换" => "Controller shortcut",
                    "光标显示 · 自动切换" => "Cursor visible · Automatic switch",
                    "光标隐藏 · 自动切换" => "Cursor hidden · Automatic switch",
                    "手动切换" => "Manual switch",
                    "配置提示预览" => "Notification preview",
                    _ => reason,
                }
            } else {
                reason
            }),
            position: settings.overlay_position,
            duration: std::time::Duration::from_millis(u64::from(
                settings.overlay_duration_ms.clamp(1000, 8000),
            )),
        });
        self.0 .1.notify_one();
        Ok(())
    }
    pub fn dismiss(&self) {
        if let Ok(mut slot) = self.0 .0.lock() {
            slot.pending = None;
            slot.dismiss = true;
            self.0 .1.notify_one();
        }
    }
    pub fn shutdown(&self) {
        if let Ok(mut slot) = self.0 .0.lock() {
            slot.pending = None;
            slot.shutdown = true;
            self.0 .1.notify_one();
        }
    }
}
fn position(work: [i32; 4], size: [i32; 2], margin: i32, corner: OverlayPosition) -> [i32; 2] {
    use OverlayPosition::*;
    let x = if matches!(corner, TopRight | BottomRight) {
        work[2] - size[0] - margin
    } else {
        work[0] + margin
    };
    let y = if matches!(corner, BottomLeft | BottomRight) {
        work[3] - size[1] - margin
    } else {
        work[1] + margin
    };
    [
        x.max(work[0]).min((work[2] - size[0]).max(work[0])),
        y.max(work[1]).min((work[3] - size[1]).max(work[1])),
    ]
}

#[cfg(windows)]
mod native {
    use super::*;
    use std::{
        mem::zeroed,
        time::{Duration, Instant},
    };
    use windows_sys::Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::LibraryLoader::GetModuleHandleW,
        UI::{HiDpi::*, WindowsAndMessaging::*},
    };
    const EX_STYLE: u32 =
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED | WS_EX_TRANSPARENT;
    const COLOR_BG: u32 = 0x00231d18;
    struct Paint {
        notice: Option<Notice>,
        scale: f64,
    }
    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain([0]).collect()
    }
    fn px(value: i32, scale: f64) -> i32 {
        (f64::from(value) * scale).round() as i32
    }
    unsafe fn text(
        dc: HDC,
        value: &str,
        mut rect: RECT,
        size: i32,
        weight: i32,
        color: u32,
        scale: f64,
    ) {
        let face = wide("Microsoft YaHei UI");
        let font = CreateFontW(
            -px(size, scale),
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            OUT_DEFAULT_PRECIS as u32,
            CLIP_DEFAULT_PRECIS as u32,
            CLEARTYPE_QUALITY as u32,
            DEFAULT_PITCH as u32,
            face.as_ptr(),
        );
        let old = SelectObject(dc, font);
        SetTextColor(dc, color);
        SetBkMode(dc, TRANSPARENT as i32);
        let text = wide(value);
        DrawTextW(
            dc,
            text.as_ptr(),
            (text.len() - 1) as i32,
            &mut rect,
            DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
        SelectObject(dc, old);
        DeleteObject(font);
    }
    unsafe extern "system" fn wndproc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if message == WM_NCCREATE {
            let create = &*(lparam as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
        }
        match message {
            WM_MOUSEACTIVATE => MA_NOACTIVATE as isize,
            WM_NCHITTEST => HTTRANSPARENT as isize,
            WM_ERASEBKGND => 1,
            WM_PAINT => {
                let mut paint: PAINTSTRUCT = zeroed();
                let dc = BeginPaint(hwnd, &mut paint);
                let context = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Paint;
                let mut rect: RECT = zeroed();
                GetClientRect(hwnd, &mut rect);
                let brush = CreateSolidBrush(COLOR_BG);
                FillRect(dc, &rect, brush);
                DeleteObject(brush);
                if let Some(context) = context.as_ref() {
                    if let Some(notice) = &context.notice {
                        let s = context.scale;
                        let accent = RECT {
                            left: 0,
                            top: 0,
                            right: px(4, s),
                            bottom: rect.bottom,
                        };
                        let brush = CreateSolidBrush(0x00d3c941);
                        FillRect(dc, &accent, brush);
                        DeleteObject(brush);
                        let area = |top, bottom| RECT {
                            left: px(22, s),
                            top: px(top, s),
                            right: rect.right - px(20, s),
                            bottom: px(bottom, s),
                        };
                        text(dc, notice.heading, area(12, 34), 12, 400, 0x00c5b9aa, s);
                        text(dc, &notice.name, area(34, 70), 21, 600, 0x00faf6f1, s);
                        text(dc, &notice.reason, area(74, 100), 13, 400, 0x00e0d4c4, s);
                    }
                }
                EndPaint(hwnd, &paint);
                0
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }
    pub(super) fn run(slot: Arc<Shared>) -> Result<(), String> {
        // Every HWND/GDI object is created, painted and destroyed on this thread.
        unsafe {
            let module = GetModuleHandleW(std::ptr::null());
            let class = wide("MotionStudio.ConfigurationNotice.v1");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: module,
                lpszClassName: class.as_ptr(),
                ..zeroed()
            };
            if RegisterClassW(&wc) == 0 {
                return Err(format!("无法注册配置提示窗口：{}", GetLastError()));
            }
            let mut paint = Paint {
                notice: None,
                scale: 1.,
            };
            let hwnd = CreateWindowExW(
                EX_STYLE,
                class.as_ptr(),
                wide("Motion Studio 配置提示").as_ptr(),
                WS_POPUP,
                0,
                0,
                1,
                1,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                module,
                (&mut paint as *mut Paint).cast(),
            );
            if hwnd.is_null() {
                UnregisterClassW(class.as_ptr(), module);
                return Err(format!("无法创建配置提示窗口：{}", GetLastError()));
            }
            let mut deadline: Option<Instant> = None;
            loop {
                let (pending, dismiss, shutdown) = {
                    let mut state = slot.0.lock().map_err(|_| "配置提示线程不可用")?;
                    (
                        state.pending.take(),
                        std::mem::take(&mut state.dismiss),
                        state.shutdown,
                    )
                };
                if shutdown {
                    break;
                }
                if dismiss {
                    ShowWindow(hwnd, SW_HIDE);
                    deadline = None;
                }
                if let Some(notice) = pending {
                    let monitor =
                        MonitorFromWindow(GetForegroundWindow(), MONITOR_DEFAULTTONEAREST);
                    let mut info: MONITORINFO = zeroed();
                    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
                    if GetMonitorInfoW(monitor, &mut info) == 0 {
                        continue;
                    }
                    let mut x = 96;
                    let mut y = 96;
                    let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut x, &mut y);
                    paint.scale = f64::from(x.max(96)) / 96.;
                    let work = info.rcWork;
                    let size = [
                        px(380, paint.scale).min(work.right - work.left),
                        px(112, paint.scale).min(work.bottom - work.top),
                    ];
                    let point = position(
                        [work.left, work.top, work.right, work.bottom],
                        size,
                        px(24, paint.scale),
                        notice.position,
                    );
                    deadline = Some(Instant::now() + notice.duration);
                    paint.notice = Some(notice);
                    let region = CreateRoundRectRgn(
                        0,
                        0,
                        size[0] + 1,
                        size[1] + 1,
                        px(16, paint.scale),
                        px(16, paint.scale),
                    );
                    if SetWindowRgn(hwnd, region, 0) == 0 {
                        DeleteObject(region);
                    }
                    SetLayeredWindowAttributes(hwnd, 0, 245, LWA_ALPHA);
                    SetWindowPos(
                        hwnd,
                        HWND_TOPMOST,
                        point[0],
                        point[1],
                        size[0],
                        size[1],
                        SWP_NOACTIVATE | SWP_SHOWWINDOW,
                    );
                    InvalidateRect(hwnd, std::ptr::null(), 0);
                }
                if let Some(end) = deadline {
                    let remaining = end.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        ShowWindow(hwnd, SW_HIDE);
                        deadline = None;
                        paint.notice = None;
                    } else if remaining < Duration::from_millis(220) {
                        SetLayeredWindowAttributes(
                            hwnd,
                            0,
                            ((remaining.as_millis() * 245) / 220) as u8,
                            LWA_ALPHA,
                        );
                    }
                }
                let mut message: MSG = zeroed();
                while PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
                let state = slot.0.lock().map_err(|_| "配置提示线程不可用")?;
                if state.pending.is_none() && !state.dismiss && !state.shutdown {
                    drop(
                        slot.1
                            .wait_timeout(
                                state,
                                Duration::from_millis(if deadline.is_some() { 16 } else { 250 }),
                            )
                            .map_err(|_| "配置提示线程不可用")?,
                    );
                }
            }
            DestroyWindow(hwnd);
            UnregisterClassW(class.as_ptr(), module);
        }
        Ok(())
    }
    #[cfg(test)]
    #[test]
    fn overlay_is_layered_click_through_nonactivating_and_absent_from_taskbar() {
        for flag in [
            WS_EX_LAYERED,
            WS_EX_TRANSPARENT,
            WS_EX_NOACTIVATE,
            WS_EX_TOOLWINDOW,
            WS_EX_TOPMOST,
        ] {
            assert_ne!(EX_STYLE & flag, 0);
        }
    }
}
#[cfg(not(windows))]
mod native {
    use super::*;
    pub(super) fn run(_: Arc<Shared>) -> Result<(), String> {
        Err("配置浮层目前仅支持 Windows".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn english_native_notice_keeps_user_names_and_localizes_the_switch_reason() {
        let overlay = Overlay(Arc::new((Mutex::new(Slot::default()), Condvar::new())));
        let settings = Settings {
            language: motion_core::storage::Language::En,
            ..Default::default()
        };
        overlay
            .show(&settings, "鼠标", "光标隐藏 · 自动切换")
            .unwrap();
        let state = overlay.0 .0.lock().unwrap();
        let notice = state.pending.as_ref().unwrap();
        assert_eq!(notice.name, "鼠标");
        assert_eq!(notice.heading, "Motion Studio · Profile changed");
        assert_eq!(notice.reason, "Cursor hidden · Automatic switch");
        drop(state);
        overlay.show(&settings, "鼠标", "手柄快捷键切换").unwrap();
        let state = overlay.0 .0.lock().unwrap();
        let notice = state.pending.as_ref().unwrap();
        assert_eq!(notice.name, "鼠标");
        assert_eq!(notice.reason, "Controller shortcut");
    }
    #[test]
    fn rapid_switches_replace_notice_and_disabling_discards_it() {
        let overlay = Overlay(Arc::new((Mutex::new(Slot::default()), Condvar::new())));
        let mut settings = Settings::default();
        overlay.show(&settings, "旧配置", "快捷键").unwrap();
        overlay.show(&settings, "新配置", "光标隐藏").unwrap();
        assert_eq!(
            overlay.0 .0.lock().unwrap().pending.as_ref().unwrap().name,
            "新配置"
        );
        overlay.dismiss();
        settings.overlay_enabled = false;
        overlay.show(&settings, "不能弹出", "自动切换").unwrap();
        assert!(overlay.0 .0.lock().unwrap().pending.is_none());
        overlay.shutdown();
    }
    #[test]
    fn corners_handle_secondary_monitor_negative_origins_and_tiny_work_areas() {
        assert_eq!(
            position(
                [-1920, 0, 0, 1080],
                [380, 112],
                24,
                OverlayPosition::TopRight
            ),
            [-404, 24]
        );
        assert_eq!(
            position(
                [-1920, 0, 0, 1080],
                [380, 112],
                24,
                OverlayPosition::BottomLeft
            ),
            [-1896, 944]
        );
        assert_eq!(
            position(
                [0, 0, 200, 100],
                [380, 112],
                24,
                OverlayPosition::BottomRight
            ),
            [0, 0]
        );
    }
    #[test]
    fn long_unicode_names_and_control_characters_cannot_change_notice_structure() {
        assert_eq!(label("游戏\n配置\0"), "游戏配置");
        assert_eq!(label(&"测".repeat(120)).chars().count(), 80);
        let settings: Settings = serde_json::from_str(r#"{"version":1}"#).unwrap();
        assert!(settings.overlay_enabled);
        assert_eq!(settings.overlay_duration_ms, 2500);
    }
}
