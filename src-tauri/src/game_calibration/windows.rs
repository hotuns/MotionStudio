use super::{
    measured_angles, run_input, CalibrationAction, CalibrationView, CurvePhase, CurveView, Input,
    IMU_CALIBRATION_UNAVAILABLE,
};
use crate::{background, text, AppState};
use motion_core::{
    motion_config::Stick, profile::GyroOutput, protocol::CalibrationButton, storage::Language,
};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, VecDeque},
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
                EnableWindow, GetCapture, IsWindowEnabled, ReleaseCapture, SetCapture,
            },
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::*,
        },
    },
};

const SUBCLASS_ID: usize = 1;
const BUTTON_ID: usize = 0x4d43;
const SCALE_ID: usize = 0x4d50;
const WM_REFRESH: u32 = WM_APP + 1;
const WM_PAD: u32 = WM_APP + 2;

struct Target {
    session: Uuid,
    hwnd: usize,
    queued: bool,
    pad_inputs: VecDeque<CalibrationButton>,
}

#[derive(Default)]
pub(super) struct Updates(Mutex<Option<Target>>);

impl Updates {
    pub(super) fn pad_input(&self, session: Uuid, button: CalibrationButton) {
        if let Ok(mut slot) = self.0.lock() {
            if let Some(target) = slot.as_mut().filter(|target| target.session == session) {
                if unsafe { PostMessageW(target.hwnd as HWND, WM_PAD, 0, 0) } != 0 {
                    target.pad_inputs.push_back(button);
                }
            }
        }
    }

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
                CurveStart if view.curve.as_ref().is_some_and(|curve| {
                    curve.samples.iter().any(|(travel, _)| *travel == curve.percent)
                }) => "重测这一档",
                CurveStart => "开始推杆",
                CurveMark if view.curve.as_ref().is_some_and(|curve| matches!(curve.phase, CurvePhase::Timing)) => "完成计时",
                CurveMark => "标记起点",
                CurveCancel => "停止本次推杆",
                CurveZero => "镜头未动 · 记为 0%",
                CurvePrev => "上一档",
                CurveNext => "下一档",
                CurveLaps => "切换圈数",
                Apply if view.curve.is_some() => "保存游戏曲线",
                Apply => "保存并应用校准",
                Close => "关闭悬浮窗",
            };
            let label = translated(view.language, label);
            // The action table is shared with dispatch, including manual mode and saved results.
            if let Some(index) = view
                .pad_actions
                .iter()
                .position(|mapped| *mapped == Some(action))
            {
                let key = ["面键 ↓", "面键 ↑", "十字键 ←", "十字键 →", "面键 ←", "面键 →"][index];
                self.control
                    .set_text(&format!("{} · {}", translated(view.language, key), label));
            } else {
                self.control.set_text(label);
            }
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
    curve_game: Cell<HWND>,
    curve_focus_lost: Cell<bool>,
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
        if view.curve.as_ref().is_some_and(|curve| matches!(curve.phase, CurvePhase::Align | CurvePhase::Timing))
            && !self.curve_game.get().is_null()
        {
            if GetForegroundWindow() != self.curve_game.get() {
                self.curve_focus_lost.set(true);
            }
            if self.curve_focus_lost.get() && !self.busy.load(Ordering::Acquire) {
                self.submit(CalibrationAction::CurveCancel, None);
            }
        }
        let panel = Panel::from_view(&view);
        let tr = |value| translated(view.language, value);
        let output = match (view.curve.is_some(), view.output) {
            (true, _) | (_, GyroOutput::RightStick) => "虚拟右摇杆",
            (_, GyroOutput::Mouse) => "鼠标",
            (_, GyroOutput::Ds4Imu | GyroOutput::DualSenseImu) => return,
        };
        self.profile
            .set_text(&format!("{} · {}", view.profile_name, tr(output)));
        self.heading.set_text(tr(panel.heading));
        let readout_was_empty = self.degrees.value.borrow().is_empty();
        let had_shortcuts = !self.shortcuts.value.borrow().is_empty();
        self.degrees.set_text(&panel.readout);
        self.detail.set_text(&panel.detail);
        self.shortcuts.set_text(&panel.shortcuts);
        let busy = self.busy.load(Ordering::Acquire);
        let show_scale =
            view.curve.is_none() && !view.manual && view.measuring.is_none() && view.saved.is_none();
        let scale_changed = self.scale_visible.replace(show_scale) != show_scale;
        if show_scale {
            let label = match view.output {
                GyroOutput::Mouse => "鼠标转角系数（counts/°）",
                GyroOutput::RightStick => "满杆角速度（°/s）",
                GyroOutput::Ds4Imu | GyroOutput::DualSenseImu => return,
            };
            self.scale_label.set_text(tr(label));
            if self.last_scale.replace(view.draft_scale) != view.draft_scale {
                self.scale_edit.value.borrow_mut().clear();
                self.scale_edit.set_text(&view.draft_scale.to_string());
            }
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
                && (view.curve.is_none()
                    || action == Some(CalibrationAction::Close)
                    || view.pad_actions.contains(&action))
                && (action != Some(CalibrationAction::Apply) || panel.valid_result)
                && (action != Some(CalibrationAction::Manual) || view.manual_available);
            button.update(action, enabled, &view);
        }
        let has_shortcuts = !panel.shortcuts.is_empty();
        if scale_changed || readout_was_empty != panel.readout.is_empty() || had_shortcuts != has_shortcuts
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

    fn submit(&self, action: CalibrationAction, button: Option<CalibrationButton>) {
        if self.busy.swap(true, Ordering::AcqRel) {
            return;
        }
        // Only the numeric field activates the overlay. Return focus to the
        // previous game before a button starts emitting a trial.
        unsafe {
            let parent = GetParent(self.scale_edit.hwnd);
            let starts_output = matches!(
                action,
                CalibrationAction::Trial
                    | CalibrationAction::Start { .. }
                    | CalibrationAction::CurveStart
            );
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
                                    Some("请先切回游戏，再按手柄键开始。".into());
                            }
                        }
                    }
                    self.busy.store(false, Ordering::Release);
                    self.updates.request();
                    return;
                }
                if matches!(action, CalibrationAction::CurveStart) {
                    self.curve_game.set(if foreground == parent {
                        self.game_window.get()
                    } else {
                        foreground
                    });
                    self.curve_focus_lost.set(false);
                }
            }
        }
        let input = match button {
            Some(button) => Input::Pad(button, action),
            None => Input::Button(action),
        };
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
    readout: String,
    detail: String,
    shortcuts: String,
    actions: [Option<CalibrationAction>; 6],
    valid_result: bool,
}

impl Panel {
    fn curve(&mut self, curve: &CurveView, language: Language) {
        use CalibrationAction::*;
        let tr = |value| translated(language, value);
        let sampling = matches!(curve.phase, CurvePhase::Align | CurvePhase::Timing);
        let (heading, instruction) = match curve.phase {
            CurvePhase::Idle => (
                "准备测量游戏转速",
                "选一个清楚的参照物，松开实体右摇杆。转得很快时，可用「切换圈数」增加本档圈数。",
            ),
            CurvePhase::Align => (
                "等待标记起点",
                "等镜头转稳，准星经过参照物时标记起点，从 0 开始数圈。",
            ),
            CurvePhase::Timing => (
                "正在计时",
                "每次回到同一参照物算一圈，第 {0} 次回来时按「完成计时」。中间经过时只数圈。",
            ),
            CurvePhase::Complete => (
                "曲线测量完成",
                "确认各档转速后保存。用十字键切换可重测，也可加测 10% 和 90%。",
            ),
            CurvePhase::Interrupted => (
                "本档测量已中断",
                "重新对准参照物后可以重测。已完成的档位会保留。",
            ),
        };
        self.heading = heading;
        let laps = curve.laps.to_string();
        self.readout = tr("推杆 {0}% · 测 {1} 圈")
            .replace("{0}", &curve.percent.to_string())
            .replace("{1}", &laps);
        self.detail = tr(instruction).replace("{0}", &laps);
        if let Some(problem) = curve.problem {
            self.detail.push_str("\r\n");
            self.detail.push_str(tr(problem));
        }
        if !curve.samples.is_empty() {
            self.detail.push_str("\r\n\r\n");
            self.detail.push_str(tr("已测推杆量 → 游戏转速"));
            for row in curve.samples.chunks(2) {
                self.detail.push_str("\r\n");
                let values: Vec<_> = row.iter()
                    .map(|(travel, speed)| format!("{travel}% → {speed:.1}%"))
                    .collect();
                self.detail.push_str(&values.join("     "));
            }
        }
        self.valid_result = curve.can_save && !sampling;
        self.actions = if sampling {
            [
                Some(CurveMark), Some(CurveCancel),
                (matches!(curve.phase, CurvePhase::Align) && curve.percent < 100).then_some(CurveZero),
                None, None, None,
            ]
        } else {
            [Some(CurveStart), Some(Apply), Some(CurvePrev), Some(CurveNext), Some(CurveLaps), Some(Close)]
        };
    }

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
        } else if let Some(curve) = &view.curve {
            panel.curve(curve, view.language);
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
            panel.readout = format!("{:.1}°", view.degrees.abs());
            panel.detail = format!("{}\r\n\r\n{}", tr("手柄净转角"), tr(instruction));
            panel.actions = [Some(Finish), Some(Cancel), None, None, None, None];
        } else if let Some(result) = &view.result {
            panel.heading = if result.axis == "yaw" {
                "水平测量已完成"
            } else {
                "垂直测量已完成"
            };
            panel.readout = format!("{:.1}°", result.degrees.abs());
            panel.detail = tr("手柄净转角").into();
            match measured_angles(result) {
                Ok(_) => panel.valid_result = true,
                Err(error) => {
                    panel.detail.push_str("\r\n\r\n");
                    panel.detail.push_str(tr(error));
                }
            }
            panel.actions = [
                Some(Start { axis: "yaw" }),
                Some(Start { axis: "pitch" }),
                Some(Back),
                Some(Close),
                None,
                None,
            ];
        } else if view.manual {
            panel.heading = "手动转动测量";
            let stick_hint = match view.stick {
                Stick::Left => "水平测量可沿左摇杆外圈转动，再用体感微调。",
                Stick::Right => "水平测量可沿右摇杆外圈转动，再用体感微调。",
            };
            panel.detail = format!(
                "{}\r\n\r\n{}",
                tr("先在游戏内选一个清楚的参照点，再开始测量。"),
                tr(stick_hint),
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
                GyroOutput::RightStick => "没转够就调小数值，转过头就调大数值。使用固定的游戏转向速度，按游戏实际曲线设置右摇杆响应补偿。",
                GyroOutput::Ds4Imu | GyroOutput::DualSenseImu => IMU_CALIBRATION_UNAVAILABLE,
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
        if view.pad_actions.iter().any(Option::is_some) {
            panel.shortcuts =
                tr("按按钮标出的手柄键操作。面键按位置标注，下键对应 Xbox A、PS ×、Switch B。").into();
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
        curve_game: Cell::new(null_mut()),
        curve_focus_lost: Cell::new(false),
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
    *updates.0.lock().map_err(text)? = Some(Target {
        hwnd: hwnd as usize,
        session,
        queued: false,
        pad_inputs: VecDeque::new(),
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
                    context.submit(action, None);
                }
                return 0;
            }
        }
        WM_PAD => {
            let button = context.updates.0.lock().ok().and_then(|mut slot| {
                slot.as_mut()
                    .filter(|target| target.session == context.session)?
                    .pad_inputs.pop_front()
            });
            if let (Some(button), Some(state)) = (button, context.app.try_state::<AppState>()) {
                let action = state.client.snapshot.lock().ok().and_then(|snapshot| {
                    let session = state.game_calibration.session_for(&snapshot).ok()?;
                    if session.id != context.session {
                        return None;
                    }
                    session.pad_action(&snapshot, button)
                });
                if let Some(action) = action {
                    context.submit(action, Some(button));
                }
            }
            return 0;
        }
        WM_NCDESTROY => {
            if let Ok(mut target) = context.updates.0.lock() {
                if target.as_ref().is_some_and(|target| target.session == context.session) {
                    *target = None;
                }
            }
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
