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
const SCALE_ID: usize = 0x4d50;
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
                Trial => "试转 360°",
                Less => "没转够 · 微调 1%",
                More => "转过头 · 微调 1%",
                Manual => "更多校准方式",
                Back => "返回试转校准",
                Start { axis: "pitch" } => "测量垂直 180°",
                Start { .. } => "测量水平 360°",
                Finish => "结束测量",
                Cancel if view.measuring.as_deref() == Some("trial") => "停止试转",
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
    scale_label: Control,
    scale_edit: Control,
    scale_visible: Cell<bool>,
    last_scale: Cell<f32>,
    game_window: Cell<HWND>,
    buttons: [Button; 6],
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
        let show_scale = !view.manual && view.measuring.is_none() && view.saved.is_none();
        let scale_changed = self.scale_visible.replace(show_scale) != show_scale;
        let label = match view.output {
            GyroOutput::Mouse => "鼠标转角系数（counts/°）",
            GyroOutput::RightStick => "满杆角速度（°/s）",
        };
        self.scale_label.set_text(tr(label));
        if self.last_scale.replace(view.draft_scale) != view.draft_scale {
            self.scale_edit.value.borrow_mut().clear();
            self.scale_edit.set_text(&view.draft_scale.to_string());
        }
        for control in [&self.scale_label, &self.scale_edit] {
            ShowWindow(control.hwnd, if show_scale { SW_SHOWNA } else { SW_HIDE });
        }
        EnableWindow(
            self.scale_edit.hwnd,
            i32::from(!busy && view.blocked.is_none()),
        );
        for (button, action) in self.buttons.iter().zip(panel.actions) {
            let enabled = action.is_some()
                && !busy
                && (action != Some(CalibrationAction::Apply) || panel.valid_result)
                && (action != Some(CalibrationAction::Manual) || view.manual_available);
            button.update(action, enabled, &view);
        }
        let has_shortcuts = !panel.shortcuts.is_empty();
        if scale_changed || had_degrees != panel.degrees.is_some() || had_shortcuts != has_shortcuts
        {
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
                &self.scale_label,
                &self.scale_edit,
                &self.buttons[0].control,
                &self.buttons[1].control,
                &self.buttons[2].control,
                &self.buttons[3].control,
                &self.buttons[4].control,
                &self.buttons[5].control,
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
        let button_y = (rect.bottom - px(134)).max(0);
        let has_shortcuts = !self.shortcuts.value.borrow().is_empty();
        let top = if has_shortcuts { px(88) } else { px(44) };
        MoveWindow(self.scale_label.hwnd, padding, top, width, px(24), 1);
        MoveWindow(
            self.scale_edit.hwnd,
            padding,
            top + px(26),
            width,
            px(34),
            1,
        );
        let heading_y = top + if self.scale_visible.get() { px(70) } else { 0 };
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
                padding + (index % 2) as i32 * (button_width + gap),
                button_y + (index / 2) as i32 * px(42),
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
        // Only the numeric field activates the overlay. Return focus to the
        // previous game before a button/hotkey starts emitting a trial.
        unsafe {
            let parent = GetParent(self.scale_edit.hwnd);
            let measuring = self.app.try_state::<AppState>().is_some_and(|state| {
                state
                    .client
                    .snapshot
                    .lock()
                    .is_ok_and(|snapshot| snapshot.status.motion.measuring.is_some())
            });
            let starts_output = matches!(
                &input,
                Input::Button(CalibrationAction::Trial | CalibrationAction::Start { .. })
            ) || (matches!(&input, Input::Hotkey(Hotkey::Measure))
                && !measuring);
            if starts_output {
                let foreground = GetForegroundWindow();
                let studio_window = |handle: HWND| {
                    self.app.windows().values().any(|window| {
                        window
                            .hwnd()
                            .is_ok_and(|hwnd| hwnd.0 as usize == handle as usize)
                    })
                };
                let focus_failed = if foreground == parent {
                    let game = self.game_window.get();
                    game.is_null()
                        || studio_window(game)
                        || IsWindow(game) == 0
                        || SetForegroundWindow(game) == 0
                } else {
                    foreground.is_null() || studio_window(foreground)
                };
                if focus_failed {
                    if let Some(state) = self.app.try_state::<AppState>() {
                        if let Ok(mut current) = state.game_calibration.current.lock() {
                            if let Some(session) = current.as_mut().filter(|s| s.id == self.session)
                            {
                                session.action_error =
                                    Some("请切回游戏，再使用 Ctrl + Shift + F8 试转。".into());
                            }
                        }
                    }
                    self.busy.store(false, Ordering::Release);
                    self.updates.request();
                    return;
                }
            }
        }
        let input = if self.scale_visible.get() {
            let mut value = vec![0u16; 128];
            let len = unsafe {
                GetWindowTextW(self.scale_edit.hwnd, value.as_mut_ptr(), value.len() as i32)
            };
            Input::Edited(
                String::from_utf16_lossy(&value[..len.max(0) as usize]),
                Box::new(input),
            )
        } else {
            input
        };
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
    actions: [Option<CalibrationAction>; 6],
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
            panel.actions[0] = Some(Close);
        } else if view.measuring.as_deref() == Some("trial") {
            panel.heading = "正在试转 360°";
            panel.detail = tr("请观察是否完整转一圈。试转结束后自动停止，再对齐参照物并调整数值。F12 可停止映射。开始和结束方向相同，也可能已转过多圈。").into();
            panel.actions = [Some(Cancel), Some(Close), None, None, None, None];
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
            panel.actions = [Some(Finish), Some(Cancel), None, None, None, None];
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
            panel.actions = [Some(Cancel), Some(Back), None, None, None, None];
        } else if view.manual {
            panel.heading = "手动转动测量";
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
            panel.actions = [
                Some(Start { axis: "yaw" }),
                Some(Start { axis: "pitch" }),
                Some(Back),
                Some(Close),
                None,
                None,
            ];
        } else {
            panel.heading = "对齐参照物后试转";
            let hint = match view.output {
                GyroOutput::Mouse => "没转够 → 调大数值；转过头 → 调小数值。鼠标转角系数表示每度需要的移动量。",
                GyroOutput::RightStick => "没转够 → 调小数值；转过头 → 调大数值。使用摇杆 Flick 前，请将游戏摇杆响应曲线设为线性，并关闭边缘及其他转向加速。",
            };
            panel.detail = format!("{}\r\n\r\n{}", tr(hint), tr("每次重新对齐参照物，确认完整转一圈。修改后再次试转，保存时才写入配置。校准后保持游戏灵敏度不变。"));
            panel.valid_result = true;
            panel.actions = [
                Some(Trial),
                Some(Apply),
                Some(Less),
                Some(More),
                Some(Manual),
                Some(Close),
            ];
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
            let label = if view.measuring.as_deref() == Some("trial") {
                "停止试转"
            } else if view.measuring.is_some() {
                "结束测量"
            } else if !view.manual {
                "试转 360°"
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
        scale_label: label(0)?,
        scale_edit: Control::new(
            hwnd,
            w!("EDIT"),
            WS_TABSTOP | WS_BORDER | ES_AUTOHSCROLL as u32,
            SCALE_ID,
        )?,
        scale_visible: Cell::new(false),
        last_scale: Cell::new(f32::NAN),
        game_window: Cell::new(GetForegroundWindow()),
        buttons: [
            Button::new(hwnd, 0)?,
            Button::new(hwnd, 1)?,
            Button::new(hwnd, 2)?,
            Button::new(hwnd, 3)?,
            Button::new(hwnd, 4)?,
            Button::new(hwnd, 5)?,
        ],
        bg,
        fg,
        brush: CreateSolidBrush(bg),
        dpi: Cell::new(0),
        fonts: RefCell::new(None),
    });
    if context.brush.is_null()
        || SetWindowSubclass(
            context.scale_edit.hwnd,
            Some(edit_proc),
            SUBCLASS_ID,
            (&mut *context as *mut Context) as usize,
        ) == 0
        || SetWindowSubclass(
            hwnd,
            Some(window_proc),
            SUBCLASS_ID,
            (&mut *context as *mut Context) as usize,
        ) == 0
    {
        RemoveWindowSubclass(context.scale_edit.hwnd, Some(edit_proc), SUBCLASS_ID);
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
        WM_CTLCOLOREDIT | WM_CTLCOLORSTATIC => {
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

unsafe extern "system" fn edit_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    subclass: usize,
    data: usize,
) -> LRESULT {
    if message == WM_MOUSEACTIVATE {
        let context = &*(data as *const Context);
        let foreground = GetForegroundWindow();
        if !foreground.is_null() && foreground != GetParent(hwnd) {
            context.game_window.set(foreground);
        }
        return MA_ACTIVATE as LRESULT;
    }
    if message == WM_NCDESTROY {
        RemoveWindowSubclass(hwnd, Some(edit_proc), subclass);
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
