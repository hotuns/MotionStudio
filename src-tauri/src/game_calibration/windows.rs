use super::{
    measured_angles, run_input, CalibrationAction, CalibrationView, Hotkey, Hotkeys, Input,
};
use crate::{background, text, AppState};
use motion_core::{motion_config::Stick, profile::GyroOutput, storage::Language};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    ptr::{null, null_mut},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex, OnceLock,
    },
};
use tauri::{AppHandle, Manager, Theme, Window};
use uuid::Uuid;
use windows_sys::{
    w,
    Win32::{
        Foundation::{GetLastError, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{
            BeginPaint, CreateFontW, CreateSolidBrush, DeleteObject, EndPaint, FillRect,
            SetBkColor, SetTextColor, CLEARTYPE_QUALITY, DEFAULT_CHARSET, HBRUSH, HDC, HFONT,
            PAINTSTRUCT,
        },
        System::SystemServices::{SS_ENDELLIPSIS, SS_NOPREFIX},
        UI::{
            HiDpi::GetDpiForWindow,
            Input::KeyboardAndMouse::{
                EnableWindow, GetCapture, IsWindowEnabled, RegisterHotKey, ReleaseCapture,
                SetCapture, UnregisterHotKey, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, VK_F8, VK_F9,
            },
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::*,
        },
    },
};

const SUBCLASS_ID: usize = 1;
const MEASURE_ID: i32 = 0x4d41;
const APPLY_ID: i32 = 0x4d42;
const BUTTON_ID: usize = 0x4d43;
const WM_REFRESH: u32 = WM_APP + 1;

struct Target {
    hwnd: usize,
    queued: bool,
}

#[derive(Default)]
pub(super) struct Updates(Mutex<Option<Target>>);

impl Updates {
    pub(super) fn request(&self) {
        let Ok(mut target) = self.0.lock() else {
            return;
        };
        if let Some(target) = target.as_mut().filter(|target| !target.queued) {
            // Queue one notification. The window reads the latest snapshot when it handles it.
            target.queued = unsafe { PostMessageW(target.hwnd as HWND, WM_REFRESH, 0, 0) } != 0;
        }
    }
}

struct Control {
    hwnd: HWND,
    value: RefCell<String>,
}

impl Control {
    unsafe fn new(parent: HWND, class: *const u16, style: u32, id: usize) -> Result<Self, String> {
        let hwnd = CreateWindowExW(
            0,
            class,
            w!(""),
            WS_CHILD | style,
            0,
            0,
            0,
            0,
            parent,
            id as HMENU,
            null_mut(),
            null(),
        );
        if hwnd.is_null() {
            return Err(format!("无法创建校准窗口控件（{}）", GetLastError()));
        }
        Ok(Self {
            hwnd,
            value: RefCell::new(String::new()),
        })
    }

    unsafe fn set_text(&self, value: &str) {
        if self.value.borrow().as_str() != value {
            let wide: Vec<_> = value.encode_utf16().chain([0]).collect();
            SetWindowTextW(self.hwnd, wide.as_ptr());
            self.value.replace(value.to_owned());
        }
    }
}

struct Button {
    control: Control,
    action: Cell<Option<CalibrationAction>>,
    visible: Cell<bool>,
}

impl Button {
    unsafe fn new(hwnd: HWND, index: usize) -> Result<Self, String> {
        let control = Control::new(hwnd, w!("BUTTON"), BS_MULTILINE as u32, BUTTON_ID + index)?;
        if SetWindowSubclass(control.hwnd, Some(button_proc), SUBCLASS_ID, 0) == 0 {
            return Err("无法设置校准窗口按钮".into());
        }
        Ok(Self {
            control,
            action: Cell::new(None),
            visible: Cell::new(false),
        })
    }

    unsafe fn update(
        &self,
        action: Option<CalibrationAction>,
        enabled: bool,
        view: &CalibrationView,
    ) {
        use CalibrationAction::*;
        if let Some(action) = action {
            let label = match action {
                Start { axis: "pitch" } => "测量垂直 180°",
                Start { .. } => "测量水平 360°",
                Finish => "结束测量",
                Cancel if view.measuring.is_some() => "取消测量",
                Cancel => "重新测量",
                Apply => "保存并应用校准",
                Close => "关闭悬浮窗",
            };
            self.control.set_text(translated(view.language, label));
        }
        let next_action = action.filter(|_| enabled);
        if self.action.replace(next_action) != next_action && GetCapture() == self.control.hwnd {
            ReleaseCapture();
        }
        if (IsWindowEnabled(self.control.hwnd) != 0) != enabled {
            EnableWindow(self.control.hwnd, i32::from(enabled));
        }
        let visible = action.is_some();
        if self.visible.replace(visible) != visible {
            ShowWindow(self.control.hwnd, if visible { SW_SHOWNA } else { SW_HIDE });
        }
    }
}

struct Fonts {
    body: HFONT,
    heading: HFONT,
    degrees: HFONT,
}

impl Drop for Fonts {
    fn drop(&mut self) {
        unsafe {
            for font in [self.body, self.heading, self.degrees] {
                if !font.is_null() {
                    DeleteObject(font);
                }
            }
        }
    }
}

struct Context {
    app: AppHandle,
    session: Uuid,
    updates: Arc<Updates>,
    busy: Arc<AtomicBool>,
    profile: Control,
    heading: Control,
    degrees: Control,
    detail: Control,
    shortcuts: Control,
    buttons: [Button; 2],
    bg: u32,
    fg: u32,
    brush: HBRUSH,
    dpi: Cell<u32>,
    fonts: RefCell<Option<Fonts>>,
}

impl Drop for Context {
    fn drop(&mut self) {
        unsafe { DeleteObject(self.brush) };
    }
}

impl Context {
    unsafe fn refresh(&self, hwnd: HWND) {
        let Some(state) = self.app.try_state::<AppState>() else {
            return;
        };
        let view = state
            .client
            .snapshot
            .lock()
            .ok()
            .and_then(|snapshot| state.game_calibration.view(&snapshot));
        let Some(view) = view.filter(|view| view.id == self.session) else {
            return;
        };
        let panel = Panel::from_view(&view);
        let tr = |value| translated(view.language, value);
        let output = match view.output {
            GyroOutput::Mouse => "鼠标",
            GyroOutput::RightStick => "虚拟右摇杆",
        };
        self.profile
            .set_text(&format!("{} · {}", view.profile_name, tr(output)));
        self.heading.set_text(tr(panel.heading));
        let had_degrees = !self.degrees.value.borrow().is_empty();
        let had_shortcuts = !self.shortcuts.value.borrow().is_empty();
        if let Some(degrees) = panel.degrees {
            self.degrees.set_text(&format!("{:.1}°", degrees.abs()));
        } else {
            self.degrees.set_text("");
        }
        self.detail.set_text(&panel.detail);
        self.shortcuts.set_text(&panel.shortcuts);
        let busy = self.busy.load(Ordering::Acquire);
        for (button, action) in self.buttons.iter().zip(panel.actions) {
            let enabled = action.is_some()
                && !busy
                && (action != Some(CalibrationAction::Apply) || panel.valid_result);
            button.update(action, enabled, &view);
        }
        let has_shortcuts = !panel.shortcuts.is_empty();
        if had_degrees != panel.degrees.is_some() || had_shortcuts != has_shortcuts {
            self.layout(hwnd);
        }
    }

    unsafe fn layout(&self, hwnd: HWND) {
        let dpi = GetDpiForWindow(hwnd).max(96);
        let px = |n: i32| (n * dpi as i32 + 48) / 96;
        if self.dpi.replace(dpi) != dpi {
            let create_font = |size, weight| {
                CreateFontW(
                    -px(size),
                    0,
                    0,
                    0,
                    weight,
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET as u32,
                    0,
                    0,
                    CLEARTYPE_QUALITY as u32,
                    0,
                    w!("Microsoft YaHei UI"),
                )
            };
            let fonts = Fonts {
                body: create_font(14, 400),
                heading: create_font(16, 600),
                degrees: create_font(32, 600),
            };
            for control in [
                &self.profile,
                &self.detail,
                &self.shortcuts,
                &self.buttons[0].control,
                &self.buttons[1].control,
            ] {
                SendMessageW(control.hwnd, WM_SETFONT, fonts.body as usize, 1);
            }
            SendMessageW(self.heading.hwnd, WM_SETFONT, fonts.heading as usize, 1);
            SendMessageW(self.degrees.hwnd, WM_SETFONT, fonts.degrees as usize, 1);
            *self.fonts.borrow_mut() = Some(fonts);
        }
        let mut rect = RECT::default();
        GetClientRect(hwnd, &mut rect);
        let padding = px(16);
        let width = (rect.right - padding * 2).max(1);
        let button_y = (rect.bottom - px(50)).max(0);
        let has_shortcuts = !self.shortcuts.value.borrow().is_empty();
        let heading_y = if has_shortcuts { px(88) } else { px(44) };
        let degrees_y = heading_y + px(26);
        let degrees_height = if self.degrees.value.borrow().is_empty() {
            0
        } else {
            px(48)
        };
        let detail_y = degrees_y + degrees_height;
        MoveWindow(self.profile.hwnd, padding, px(14), width, px(23), 1);
        MoveWindow(
            self.shortcuts.hwnd,
            padding,
            px(40),
            width,
            if has_shortcuts { px(45) } else { 0 },
            1,
        );
        MoveWindow(self.heading.hwnd, padding, heading_y, width, px(24), 1);
        MoveWindow(
            self.degrees.hwnd,
            padding,
            degrees_y,
            width,
            degrees_height,
            1,
        );
        MoveWindow(
            self.detail.hwnd,
            padding,
            detail_y,
            width,
            (button_y - detail_y - px(8)).max(0),
            1,
        );
        let gap = px(8);
        let button_width = (width - gap).max(0) / 2;
        for (index, button) in self.buttons.iter().enumerate() {
            MoveWindow(
                button.control.hwnd,
                padding + index as i32 * (button_width + gap),
                button_y,
                button_width,
                px(34),
                1,
            );
        }
    }

    fn submit(&self, input: Input) {
        if self.busy.swap(true, Ordering::AcqRel) {
            return;
        }
        self.updates.request();
        let app = self.app.clone();
        let session = self.session;
        let busy = self.busy.clone();
        let updates = self.updates.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let result = run_input(&app, session, input);
            busy.store(false, Ordering::Release);
            updates.request();
            if let (Err(error), Some(state)) = (result, app.try_state::<AppState>()) {
                state.client.log(error);
            }
        });
    }
}

#[derive(Default)]
struct Panel {
    heading: &'static str,
    degrees: Option<f32>,
    detail: String,
    shortcuts: String,
    actions: [Option<CalibrationAction>; 2],
    valid_result: bool,
}

impl Panel {
    fn from_view(view: &CalibrationView) -> Self {
        use CalibrationAction::*;
        let tr = |value| translated(view.language, value);
        let mut panel = Self::default();
        if let Some(saved) = &view.saved {
            panel.heading = if saved.applied {
                "配置已保存并应用"
            } else {
                "配置已保存"
            };
            if let Some(warning) = &saved.warning {
                panel.detail = tr("配置已保存，应用失败：{0}").replace("{0}", tr(warning));
            }
            panel.actions[0] = Some(Close);
        } else if let Some(blocked) = view.blocked {
            panel.detail = tr(blocked).into();
        } else if let Some(axis) = view.measuring.as_deref() {
            panel.heading = if axis == "yaw" {
                "正在测量水平 360°"
            } else {
                "正在测量垂直 180°"
            };
            let instruction = if axis == "yaw" {
                "让游戏镜头转一整圈并回到参照点。松开摇杆，等镜头停稳后结束测量。"
            } else {
                "让游戏镜头从最低转到最高，停稳后结束测量。"
            };
            panel.degrees = Some(view.degrees);
            panel.detail = format!("{}\r\n\r\n{}", tr("手柄净转角"), tr(instruction));
            panel.actions = [Some(Finish), Some(Cancel)];
        } else if let Some(result) = &view.result {
            panel.heading = if result.axis == "yaw" {
                "水平测量已完成"
            } else {
                "垂直测量已完成"
            };
            panel.degrees = Some(result.degrees);
            panel.detail = tr("手柄净转角").into();
            match measured_angles(result) {
                Ok(_) => panel.valid_result = true,
                Err(error) => {
                    panel.detail.push_str("\r\n\r\n");
                    panel.detail.push_str(tr(error));
                }
            }
            panel.actions = [Some(Apply), Some(Cancel)];
        } else {
            let stick_hint = match view.stick {
                Stick::Left => "水平测量可沿左摇杆外圈转动，再用体感微调。",
                Stick::Right => "水平测量可沿右摇杆外圈转动，再用体感微调。",
            };
            let mode_hint = if view.hotkeys.measure {
                "请使用窗口或无边框全屏。游戏锁定光标时，可以用上方的校准按键。"
            } else {
                "请使用窗口或无边框全屏。游戏锁定光标时，可通过游戏菜单调出光标。"
            };
            panel.detail = format!(
                "{}\r\n\r\n{}\r\n\r\n{}",
                tr("先在游戏内选一个清楚的参照点，再开始测量。"),
                tr(stick_hint),
                tr(mode_hint),
            );
            panel.actions = [Some(Start { axis: "yaw" }), Some(Start { axis: "pitch" })];
        }
        for error in [&view.measurement_error, &view.action_error]
            .into_iter()
            .flatten()
        {
            if !panel.detail.is_empty() {
                panel.detail.push_str("\r\n\r\n");
            }
            panel.detail.push_str(tr(error));
        }
        if view.hotkeys.measure && view.blocked.is_none() && view.saved.is_none() {
            let label = if view.measuring.is_some() {
                "结束测量"
            } else if view.result.is_some() {
                "重测水平 360°"
            } else {
                "开始水平测量"
            };
            panel.shortcuts = format!("Ctrl + Shift + F8   {}", tr(label));
        }
        if view.hotkeys.apply && (view.saved.is_some() || panel.valid_result) {
            if !panel.shortcuts.is_empty() {
                panel.shortcuts.push_str("\r\n");
            }
            let label = if view.saved.is_some() {
                "关闭悬浮窗"
            } else {
                "保存并应用校准"
            };
            panel
                .shortcuts
                .push_str(&format!("Ctrl + Shift + F9   {}", tr(label)));
        }
        panel
    }
}

fn translated(language: Language, value: &str) -> &str {
    static ENGLISH: OnceLock<HashMap<String, String>> = OnceLock::new();
    if language == Language::En {
        return ENGLISH
            .get_or_init(|| {
                serde_json::from_str(include_str!("../../../src/locales/en.json"))
                    .unwrap_or_default()
            })
            .get(value)
            .map_or(value, String::as_str);
    }
    value
}

pub(super) async fn install(window: &Window, session: Uuid, theme: Theme) -> Result<(), String> {
    let handle = window.hwnd().map_err(text)?.0 as usize;
    let app = window.app_handle().clone();
    let (ready, result) = mpsc::sync_channel(1);
    window
        .run_on_main_thread(move || {
            let _ = ready.send(unsafe { install_controls(&app, handle as HWND, session, theme) });
        })
        .map_err(text)?;
    background(move || result.recv().map_err(text)?).await
}

unsafe fn install_controls(
    app: &AppHandle,
    hwnd: HWND,
    session: Uuid,
    theme: Theme,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    if IsWindow(hwnd) == 0 || state.game_calibration.session()?.id != session {
        return Err("校准悬浮窗已关闭。".into());
    }
    let style = GetWindowLongW(hwnd, GWL_STYLE);
    SetWindowLongW(hwnd, GWL_STYLE, style | WS_CLIPCHILDREN as i32);
    let updates = state.game_calibration.updates.clone();
    let label = |style| Control::new(hwnd, w!("STATIC"), WS_VISIBLE | SS_NOPREFIX | style, 0);
    let (bg, fg) = if theme == Theme::Dark {
        // COLORREF uses BGR: match Studio's charcoal background and light text.
        (0x00101210, 0x00edf3ef)
    } else {
        (0x00fbf9f7, 0x00292521)
    };
    let mut context = Box::new(Context {
        app: app.clone(),
        session,
        updates: updates.clone(),
        busy: Arc::new(AtomicBool::new(false)),
        profile: label(SS_ENDELLIPSIS)?,
        heading: label(0)?,
        degrees: label(0)?,
        detail: label(0)?,
        shortcuts: label(0)?,
        buttons: [Button::new(hwnd, 0)?, Button::new(hwnd, 1)?],
        bg,
        fg,
        brush: CreateSolidBrush(bg),
        dpi: Cell::new(0),
        fonts: RefCell::new(None),
    });
    if context.brush.is_null()
        || SetWindowSubclass(
            hwnd,
            Some(window_proc),
            SUBCLASS_ID,
            (&mut *context as *mut Context) as usize,
        ) == 0
    {
        return Err("无法安装校准窗口处理程序".into());
    }
    let context = &*Box::into_raw(context);
    let modifiers = MOD_CONTROL | MOD_SHIFT | MOD_NOREPEAT;
    let hotkeys = Hotkeys {
        measure: RegisterHotKey(hwnd, MEASURE_ID, modifiers, u32::from(VK_F8)) != 0,
        apply: RegisterHotKey(hwnd, APPLY_ID, modifiers, u32::from(VK_F9)) != 0,
    };
    if let Ok(mut current) = state.game_calibration.current.lock() {
        if let Some(current) = current.as_mut().filter(|current| current.id == session) {
            current.hotkeys = hotkeys;
        }
    }
    *updates.0.lock().map_err(text)? = Some(Target {
        hwnd: hwnd as usize,
        queued: false,
    });
    context.refresh(hwnd);
    context.layout(hwnd);
    Ok(())
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    subclass: usize,
    data: usize,
) -> LRESULT {
    let context = &*(data as *const Context);
    match message {
        WM_MOUSEACTIVATE => return MA_NOACTIVATE as LRESULT,
        WM_REFRESH => {
            if let Ok(mut slot) = context.updates.0.lock() {
                if let Some(target) = slot.as_mut() {
                    target.queued = false;
                }
            }
            context.refresh(hwnd);
            return 0;
        }
        WM_SIZE | WM_DPICHANGED => {
            let result = DefSubclassProc(hwnd, message, wparam, lparam);
            context.layout(hwnd);
            return result;
        }
        WM_ERASEBKGND => return 1,
        WM_PAINT => {
            let mut paint = PAINTSTRUCT::default();
            let dc = BeginPaint(hwnd, &mut paint);
            FillRect(dc, &paint.rcPaint, context.brush);
            EndPaint(hwnd, &paint);
            return 0;
        }
        WM_CTLCOLORSTATIC => {
            SetTextColor(wparam as HDC, context.fg);
            SetBkColor(wparam as HDC, context.bg);
            return context.brush as LRESULT;
        }
        WM_COMMAND if wparam >> 16 == BN_CLICKED as usize => {
            let id = wparam & 0xffff;
            if let Some(button) = id
                .checked_sub(BUTTON_ID)
                .and_then(|index| context.buttons.get(index))
                .filter(|button| button.control.hwnd as LPARAM == lparam)
            {
                if let Some(action) = button.action.get() {
                    context.submit(Input::Button(action));
                }
                return 0;
            }
        }
        WM_HOTKEY => {
            let key = match wparam as i32 {
                MEASURE_ID => Some(Hotkey::Measure),
                APPLY_ID => Some(Hotkey::Apply),
                _ => None,
            };
            if let Some(key) = key {
                context.submit(Input::Hotkey(key));
                return 0;
            }
        }
        WM_NCDESTROY => {
            if let Ok(mut target) = context.updates.0.lock() {
                *target = None;
            }
            UnregisterHotKey(hwnd, MEASURE_ID);
            UnregisterHotKey(hwnd, APPLY_ID);
            if RemoveWindowSubclass(hwnd, Some(window_proc), subclass) != 0 {
                drop(Box::from_raw(data as *mut Context));
            }
        }
        _ => {}
    }
    DefSubclassProc(hwnd, message, wparam, lparam)
}

unsafe extern "system" fn button_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    subclass: usize,
    _: usize,
) -> LRESULT {
    match message {
        WM_MOUSEACTIVATE => return MA_NOACTIVATE as LRESULT,
        WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
            // A standard button focuses itself on mouse-down. Keep the game active.
            if IsWindowEnabled(hwnd) != 0 {
                SetCapture(hwnd);
                if GetCapture() == hwnd {
                    SendMessageW(hwnd, BM_SETSTATE, 1, 0);
                }
            }
            return 0;
        }
        WM_LBUTTONUP => {
            if GetCapture() == hwnd {
                ReleaseCapture();
                SendMessageW(hwnd, BM_SETSTATE, 0, 0);
                let x = i32::from(lparam as i16);
                let y = i32::from((lparam >> 16) as i16);
                let mut rect = RECT::default();
                GetClientRect(hwnd, &mut rect);
                if x >= 0
                    && x < rect.right
                    && y >= 0
                    && y < rect.bottom
                    && IsWindowEnabled(hwnd) != 0
                {
                    SendMessageW(
                        GetParent(hwnd),
                        WM_COMMAND,
                        GetDlgCtrlID(hwnd) as usize,
                        hwnd as LPARAM,
                    );
                }
            }
            return 0;
        }
        WM_CAPTURECHANGED => {
            SendMessageW(hwnd, BM_SETSTATE, 0, 0);
        }
        WM_NCDESTROY => {
            RemoveWindowSubclass(hwnd, Some(button_proc), subclass);
        }
        _ => {}
    }
    DefSubclassProc(hwnd, message, wparam, lparam)
}
