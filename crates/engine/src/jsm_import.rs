//! One-way, offline JSM translation into editable Studio profiles.
use crate::{
    bindings::{input_chord, output_chord},
    controller_input::StickDirections,
    motion_config::{Space, Stick},
    profile::{Action, GamepadTarget, GyroOutput, Profile},
};
use anyhow::{bail, ensure, Context, Result};
use flexinput_engine::eval::{jsm_compile, jsm_tab_name, JsmAnalogSettings, JsmLineStatus};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

const MAX_BYTES: usize = 512 * 1024;
const MAX_LINES: usize = 4096;
const MAX_FILES: usize = 16;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JsmFile {
    pub name: String,
    pub text: String,
}

#[derive(Deserialize)]
struct LegacyProfile {
    schema_version: u32,
    name: String,
    #[serde(default)]
    virtual_gamepad: GamepadTarget,
    jsm: LegacySource,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacySource {
    files: Vec<JsmFile>,
    #[serde(default)]
    passthrough: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ImportLine {
    pub file: String,
    pub line: usize,
    pub source: String,
    pub status: String,
    pub message: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct JsmImport {
    pub profile: Profile,
    pub lines: Vec<ImportLine>,
    pub warnings: Vec<String>,
    pub converted: usize,
    pub approximate: usize,
    pub skipped: usize,
}
#[derive(Clone)]
struct Setting {
    index: usize,
    key: String,
    value: String,
}
type Settings = BTreeMap<String, Setting>;
fn mark(lines: &mut [ImportLine], index: usize, status: &str, message: impl Into<String>) {
    lines[index].status = status.into();
    lines[index].message = message.into();
}
fn base(name: &str) -> Profile {
    let mut p: Profile = toml::from_str(include_str!("../../../examples/blank.toml")).unwrap();
    p.name = name.chars().take(100).collect();
    if p.name.trim().is_empty() {
        p.name = "导入的 JSM 配置".into();
    }
    // JSM RESET_MAPPINGS: gyro sensitivity is zero, calibration is 40, game sens is 1.
    p.gyro.counts_per_degree = 40.;
    p.stick_deadzone = 0.15;
    let analog = JsmAnalogSettings::default();
    p.trigger_stages.left_mode = analog.zl;
    p.trigger_stages.right_mode = analog.zr;
    p.trigger_stages.soft_threshold = analog.threshold;
    p.trigger_stages.skip_delay_ms = analog.skip_delay * 1000.;
    p
}
pub fn input(name: &str) -> Option<&'static str> {
    Some(match name {
        "S" => "south",
        "E" => "east",
        "W" => "west",
        "N" => "north",
        "L" => "left_shoulder",
        "R" => "right_shoulder",
        "ZL" => "left_trigger_soft",
        "ZLF" => "left_trigger_full",
        "ZR" => "right_trigger_soft",
        "ZRF" => "right_trigger_full",
        "L3" => "left_stick_click",
        "R3" => "right_stick_click",
        "-" => "back",
        "+" => "start",
        "HOME" => "guide",
        "UP" => "dpad_up",
        "DOWN" => "dpad_down",
        "LEFT" => "dpad_left",
        "RIGHT" => "dpad_right",
        "LUP" => "left_stick_up",
        "LDOWN" => "left_stick_down",
        "LLEFT" => "left_stick_left",
        "LRIGHT" => "left_stick_right",
        "RUP" => "right_stick_up",
        "RDOWN" => "right_stick_down",
        "RLEFT" => "right_stick_left",
        "RRIGHT" => "right_stick_right",
        "CAPTURE" => "touchpad",
        "MIC" | "MISC1" => "misc1",
        "MISC2" => "misc2",
        "MISC3" => "misc3",
        "MISC4" => "misc4",
        "MISC5" => "misc5",
        "MISC6" => "misc6",
        "R4" | "RP1" | "RIGHT_PADDLE1" => "right_paddle1",
        "L4" | "LP1" | "LEFT_PADDLE1" => "left_paddle1",
        "R5" | "RP2" | "RIGHT_PADDLE2" => "right_paddle2",
        "L5" | "LP2" | "LEFT_PADDLE2" => "left_paddle2",
        _ => return None,
    })
}
fn trigger(lhs: &str) -> Result<(String, bool)> {
    if let Some(value) = input(lhs) {
        return Ok((value.into(), false));
    }
    for (i, sep) in lhs.char_indices() {
        if !matches!(sep, ',' | '+' | '*') {
            continue;
        }
        if let (Some(a), Some(b)) = (input(lhs[..i].trim()), input(lhs[i + 1..].trim())) {
            if a == b {
                bail!("双击触发暂不能迁移");
            }
            let value = format!("{a}+{b}");
            if input_chord(&value).is_none() {
                bail!("这个组合包含互斥方向");
            }
            return Ok((value, sep != '*'));
        }
    }
    bail!("输入、双击或按住切换设置暂不能迁移")
}
fn atom(value: &str) -> Option<String> {
    if let Some(digit) = value.strip_prefix('N') {
        if digit.len() == 1 && digit.as_bytes()[0].is_ascii_digit() {
            return Some(format!("key:NUMPAD{digit}"));
        }
    }
    let out = match value {
        "LMOUSE" => "mouse:left",
        "RMOUSE" => "mouse:right",
        "MMOUSE" => "mouse:middle",
        "BMOUSE" | "XBUTTON1" => "mouse:x1",
        "FMOUSE" | "XBUTTON2" => "mouse:x2",
        "SCROLLUP" => "wheel:up",
        "SCROLLDOWN" => "wheel:down",
        "SCROLLLEFT" => "wheel:left",
        "SCROLLRIGHT" => "wheel:right",
        "LSHIFT" => "key:SHIFT",
        "LCONTROL" | "CONTROL" | "LCTRL" => "key:CTRL",
        "RCONTROL" => "key:RCTRL",
        "LALT" => "key:ALT",
        "LWINDOWS" | "LWIN" | "WINDOWS" => "key:WIN",
        "RWINDOWS" => "key:RWIN",
        "RETURN" => "key:ENTER",
        "ESCAPE" => "key:ESC",
        "DEL" => "key:DELETE",
        "CAPS_LOCK" => "key:CAPSLOCK",
        "NUM_LOCK" => "key:NUMLOCK",
        "SCROLL_LOCK" => "key:SCROLLLOCK",
        "SCREENSHOT" => "key:PRINTSCREEN",
        "OPENBRACKET" => "key:LEFTBRACKET",
        "CLOSEBRACKET" => "key:RIGHTBRACKET",
        "QUOTE" => "key:APOSTROPHE",
        "BACKTICK" => "key:GRAVE",
        "PLUS" => "key:EQUALS",
        "ADD" => "key:NUMPADADD",
        "SUBTRACT" | "SUBSTRACT" => "key:NUMPADSUBTRACT",
        "MULTIPLY" => "key:NUMPADMULTIPLY",
        "DIVIDE" => "key:NUMPADDIVIDE",
        "DECIMAL" => "key:NUMPADDECIMAL",
        _ => {
            if crate::profile::valid_key(value) {
                return Some(format!("key:{value}"));
            }
            return None;
        }
    };
    Some(out.into())
}
fn outputs(value: &str) -> Result<String> {
    let tokens: Vec<_> = value.split_whitespace().collect();
    if tokens.is_empty() {
        bail!("缺少输出");
    }
    if tokens.len() > 1 && tokens.iter().any(|v| !v.ends_with('\\')) {
        bail!("多个未标注输出在 JSM 中表示轻按／长按，不能转换为同时按下；请手动补配");
    }
    let mut values = vec![];
    for token in tokens {
        let token = token.strip_suffix('\\').unwrap_or(token);
        if token.starts_with(['^', '!', '-', '"']) || token.ends_with(['/', '\'', '_', '+']) {
            bail!("切换、瞬按、轻按、长按、释放或连发修饰符暂不能迁移");
        }
        let Some(value) = atom(token) else {
            bail!("输出 {token} 暂不支持（F12 保留为紧急停止）");
        };
        if !values.contains(&value) {
            values.push(value);
        }
    }
    let value = values.join("+");
    if output_chord(&value).is_none() {
        bail!("输出组合需为最多 4 个成员、最多一个滚轮方向");
    }
    Ok(value)
}
fn pair(value: &str) -> Result<[f32; 2]> {
    let p: Vec<f32> = value
        .split_whitespace()
        .map(str::parse)
        .collect::<std::result::Result<_, _>>()?;
    if p.is_empty() || p.len() > 2 || p.iter().any(|v| !v.is_finite()) {
        bail!("需要一到两个有限数值");
    }
    Ok([p[0], *p.get(1).unwrap_or(&p[0])])
}
fn scalar(value: &str) -> Result<f32> {
    let p = pair(value)?;
    if value.split_whitespace().count() != 1 {
        bail!("需要一个数值");
    }
    Ok(p[0])
}
fn number(settings: &Settings, key: &str, default: f32) -> Result<f32> {
    settings
        .get(key)
        .map(|s| scalar(&s.value))
        .unwrap_or(Ok(default))
}
fn apply<F: FnOnce(&mut Profile) -> Result<Option<&'static str>>>(
    p: &mut Profile,
    lines: &mut [ImportLine],
    s: &Setting,
    f: F,
) {
    let mut next = p.clone();
    match f(&mut next).and_then(|note| {
        next.validate()?;
        Ok(note)
    }) {
        Ok(note) => {
            *p = next;
            mark(
                lines,
                s.index,
                if note.is_some() {
                    "approximate"
                } else {
                    "converted"
                },
                note.unwrap_or("已转换"),
            );
        }
        Err(error) => mark(lines, s.index, "skipped", format!("未转换：{error:#}")),
    }
}
fn native_output(value: &str) -> Option<&'static str> {
    Some(match value.trim_end_matches('\\') {
        "X_A" | "PS_CROSS" => "south",
        "X_B" | "PS_CIRCLE" => "east",
        "X_X" | "PS_SQUARE" => "west",
        "X_Y" | "PS_TRIANGLE" => "north",
        "X_LB" | "PS_L1" => "left_shoulder",
        "X_RB" | "PS_R1" => "right_shoulder",
        "X_LS" | "PS_L3" => "left_stick_click",
        "X_RS" | "PS_R3" => "right_stick_click",
        "X_BACK" | "PS_SHARE" => "back",
        "X_START" | "PS_OPTIONS" => "start",
        "X_GUIDE" | "PS_HOME" => "guide",
        "X_UP" | "PS_UP" => "dpad_up",
        "X_DOWN" | "PS_DOWN" => "dpad_down",
        "X_LEFT" | "PS_LEFT" => "dpad_left",
        "X_RIGHT" | "PS_RIGHT" => "dpad_right",
        "PS_PAD_CLICK" => "touchpad",
        _ => return None,
    })
}
fn remember(map: &mut Settings, key: String, s: Setting, lines: &mut [ImportLine]) {
    if let Some(old) = map.insert(key, s) {
        if !map.values().any(|v| v.index == old.index) {
            mark(lines, old.index, "overridden", "被后面的赋值覆盖");
        }
    }
}

pub fn import(content: &str, filename: &str) -> Result<JsmImport> {
    import_files(&[JsmFile {
        name: filename
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(filename)
            .into(),
        text: content.into(),
    }])
}

pub fn import_files(files: &[JsmFile]) -> Result<JsmImport> {
    let (main, related) = files.split_first().context("请选择 JSM 主配置文件")?;
    if !main.name.to_ascii_lowercase().ends_with(".toml") {
        return convert_files(files);
    }
    ensure!(related.is_empty(), "旧版配置已包含关联文件，请单独导入");
    ensure!(main.text.len() <= MAX_BYTES, "配置文件超过 512 KB");
    let legacy: LegacyProfile = toml::from_str(&main.text).context("请选择旧版 JSM 原文配置")?;
    ensure!(legacy.schema_version == 1, "请选择旧版 JSM 原文配置");
    let mut report = convert_files(&legacy.jsm.files)?;
    report.profile.name = legacy.name.trim().chars().take(100).collect();
    report.profile.validate()?;
    if legacy.jsm.passthrough || legacy.virtual_gamepad != report.profile.virtual_gamepad {
        report
            .warnings
            .push("旧配置的虚拟手柄输出或透传规则需要手动核对。".into());
    }
    Ok(report)
}

fn convert_files(files: &[JsmFile]) -> Result<JsmImport> {
    ensure!(
        (1..=MAX_FILES).contains(&files.len()),
        "JSM 配置需要 1–16 个文件"
    );
    let mut names = HashSet::new();
    let mut bytes = 0;
    let mut count = 0;
    for file in files {
        ensure!(
            !file.name.is_empty()
                && file.name.chars().count() <= 180
                && !file.name.contains(['/', '\\'])
                && !file.name.chars().any(char::is_control),
            "JSM 文件名无效"
        );
        let name = jsm_tab_name(&file.name);
        ensure!(!name.is_empty(), "JSM 文件名无效");
        ensure!(
            names.insert(name.to_ascii_lowercase()),
            "JSM 文件名重复：{name}"
        );
        ensure!(!file.text.contains('\0'), "配置包含空字符，请选择文本配置");
        bytes += file.text.len();
        count += file.text.lines().count();
        ensure!(bytes <= MAX_BYTES, "配置文件超过 512 KB");
        ensure!(count <= MAX_LINES, "JSM 配置最多 4096 行");
    }
    let filename = &files[0].name;
    // Flatten one level of explicitly supplied files at the reference site.
    // No filenames from the text are opened or used as filesystem paths.
    let mut source = Vec::new();
    let mut expanded_bytes = files[0].text.len();
    for (index, raw) in files[0]
        .text
        .trim_start_matches('\u{feff}')
        .lines()
        .enumerate()
    {
        let code = raw.split('#').next().unwrap_or("").trim();
        let looks_like_file = code.contains(['/', '\\'])
            || code.rsplit_once('.').is_some_and(|(_, ext)| {
                matches!(
                    ext.to_ascii_lowercase().as_str(),
                    "txt" | "jsm" | "cfg" | "conf"
                )
            });
        let related = if !looks_like_file || code.contains('=') {
            None
        } else {
            let tab = jsm_tab_name(code);
            files[1..]
                .iter()
                .find(|file| jsm_tab_name(&file.name).eq_ignore_ascii_case(&tab))
        };
        source.push((filename.as_str(), index + 1, raw, related.is_some()));
        if let Some(related) = related {
            expanded_bytes += related.text.len();
            ensure!(expanded_bytes <= MAX_BYTES, "配置文件超过 512 KB");
            source.extend(
                related
                    .text
                    .trim_start_matches('\u{feff}')
                    .lines()
                    .enumerate()
                    .map(|(index, line)| (related.name.as_str(), index + 1, line, false)),
            );
        }
        ensure!(source.len() <= MAX_LINES, "JSM 配置最多 4096 行");
    }
    let mut name = filename
        .rsplit_once('.')
        .map_or(filename.as_str(), |(n, _)| n)
        .to_owned();
    let mut labels: BTreeMap<(String, String), String> = BTreeMap::new();
    let mut lines = vec![];
    let mut settings = Settings::new();
    let mut bindings = Settings::new();
    for (file, number, raw, expanded) in source {
        let raw = raw.trim();
        if let Some(json) = raw
            .strip_prefix("# JSM_STUDIO_GAME ")
            .filter(|_| file == filename)
        {
            if let Ok(data) = serde_json::from_str::<serde_json::Value>(json) {
                if let Some(value) = data["gameName"].as_str().filter(|v| !v.trim().is_empty()) {
                    name = value.chars().take(100).collect();
                }
            }
        }
        if let Some(json) = raw.strip_prefix("# JSM_STUDIO_ACTION ") {
            if let Ok(data) = serde_json::from_str::<serde_json::Value>(json) {
                if data["gesture"].as_str().is_some_and(|g| g != "press") {
                    continue;
                }
                if let (Some(button), Some(key), Some(label)) = (
                    data["button"].as_str(),
                    data["key"].as_str(),
                    data["zh"].as_str(),
                ) {
                    if !label.trim().is_empty() {
                        labels.insert(
                            (button.to_uppercase(), key.to_uppercase()),
                            label.chars().take(100).collect(),
                        );
                    }
                }
            }
        }
        let code = raw.split('#').next().unwrap_or("").trim().to_uppercase();
        if code.is_empty() {
            continue;
        }
        let index = lines.len();
        lines.push(ImportLine {
            file: file.into(),
            line: number,
            source: raw.chars().take(2048).collect(),
            status: "skipped".into(),
            message: "此命令暂不能迁移".into(),
        });
        if expanded {
            mark(&mut lines, index, "included", "已在这里展开选中的关联文件");
            continue;
        }
        if code == "RESET_MAPPINGS" {
            for old in settings.values().chain(bindings.values()) {
                mark(
                    &mut lines,
                    old.index,
                    "overridden",
                    "被后面的 RESET_MAPPINGS 重置",
                );
            }
            settings.clear();
            bindings.clear();
            mark(&mut lines, index, "converted", "重置前面的配置项");
            continue;
        }
        if code == "NO_GYRO_BUTTON" {
            remember(
                &mut settings,
                "GYRO_GATE".into(),
                Setting {
                    index,
                    key: "GYRO_OFF".into(),
                    value: "NONE".into(),
                },
                &mut lines,
            );
            continue;
        }
        if code == "ONE_EURO_FILTER" {
            remember(
                &mut settings,
                "ONE_EURO_FILTER".into(),
                Setting {
                    index,
                    key: "ONE_EURO_FILTER".into(),
                    value: "ON".into(),
                },
                &mut lines,
            );
            continue;
        }
        let Some((key, value)) = code.split_once('=') else {
            mark(
                &mut lines,
                index,
                "skipped",
                "未迁移此命令。主文件引用的配置可作为关联文件添加",
            );
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        let s = Setting {
            index,
            key: key.into(),
            value: value.into(),
        };
        if trigger(key).is_ok() {
            // Canonicalize the unordered input set so equivalent definitions overwrite.
            let mut key: Vec<_> = trigger(key)
                .unwrap()
                .0
                .split('+')
                .map(str::to_owned)
                .collect();
            key.sort();
            remember(&mut bindings, key.join("+"), s, &mut lines);
        } else if key == "GYRO_SENS" {
            remember(&mut settings, "MIN_GYRO_SENS".into(), s.clone(), &mut lines);
            remember(&mut settings, "MAX_GYRO_SENS".into(), s, &mut lines);
        } else {
            let canonical = if matches!(key, "GYRO_ON" | "GYRO_OFF") {
                "GYRO_GATE"
            } else {
                key
            };
            remember(&mut settings, canonical.into(), s, &mut lines);
        }
    }
    let mut p = base(&name);
    let mut warnings = vec![];
    if let Some(s) = settings.get("VIRTUAL_CONTROLLER") {
        let target = match s.value.as_str() {
            "XBOX" | "X360" => Some(GamepadTarget::Xbox360),
            "DS4" => Some(GamepadTarget::DualShock4),
            "NONE" | "OFF" => Some(GamepadTarget::None),
            _ => None,
        };
        let complete = crate::profile::BUTTONS[..15].iter().all(|button| {
            bindings
                .get(*button)
                .is_some_and(|b| native_output(&b.value) == Some(*button))
        }) && settings
            .get("LEFT_STICK_MODE")
            .is_some_and(|s| s.value == "LEFT_STICK")
            && settings
                .get("RIGHT_STICK_MODE")
                .is_some_and(|s| s.value == "RIGHT_STICK")
            && settings
                .get("ZL_MODE")
                .is_some_and(|s| matches!(s.value.as_str(), "X_LT" | "PS_L2"))
            && settings
                .get("ZR_MODE")
                .is_some_and(|s| matches!(s.value.as_str(), "X_RT" | "PS_R2"));
        if let Some(target) = target.filter(|t| *t == GamepadTarget::None || complete) {
            p.virtual_gamepad = target;
            let message = if target == GamepadTarget::None {
                "已选择键鼠输出"
            } else {
                "已填入标准虚拟手柄布局"
            };
            mark(&mut lines, s.index, "converted", message);
        } else {
            mark(
                &mut lines,
                s.index,
                "skipped",
                "导入支持完整的标准 Xbox／DS4 布局。可在动作映射中手动绑定自定义键位",
            );
        }
    }
    let calibration = number(&settings, "REAL_WORLD_CALIBRATION", 40.).and_then(|rwc| {
        let game = number(&settings, "IN_GAME_SENS", 1.)?;
        let scale = rwc / game;
        if rwc <= 0. || game <= 0. || !scale.is_finite() || scale <= 0. || scale > 10000. {
            bail!("校准比例需为正数，且 RWC / IN_GAME_SENS 不超过 10000");
        }
        Ok(scale)
    });
    for key in ["REAL_WORLD_CALIBRATION", "IN_GAME_SENS"] {
        if let Some(s) = settings.get(key) {
            match &calibration {
                Ok(scale) => {
                    p.gyro.counts_per_degree = *scale;
                    mark(
                        &mut lines,
                        s.index,
                        "approximate",
                        "已换算为每度鼠标计数（RWC / IN_GAME_SENS）；请核验系统鼠标速度与游戏转角",
                    );
                }
                Err(error) => mark(
                    &mut lines,
                    s.index,
                    "skipped",
                    format!("校准未转换：{error:#}"),
                ),
            }
        }
    }
    let low = settings.get("MIN_GYRO_SENS");
    let high = settings.get("MAX_GYRO_SENS");
    if low.is_some() || high.is_some() {
        let sens = (|| -> Result<()> {
            let a = pair(low.map_or("0", |s| s.value.as_str()))?;
            let b = pair(high.map_or("0", |s| s.value.as_str()))?;
            if a == [0., 0.] && b == [0., 0.] {
                p.gyro.enabled = false;
                return Ok(());
            }
            if a[0] == 0.
                || a[1] == 0.
                || b[0] / a[0] < 1.
                || b[0] / a[0] > 4.
                || (b[1] / a[1] - b[0] / a[0]).abs() > 0.001
            {
                bail!("需要非零基础灵敏度、固定纵横比例，以及 1–4 倍快速转动倍率；无法等价转换该 MIN/MAX 组合");
            }
            let mut next = p.clone();
            next.gyro.enabled = true;
            next.gyro.sensitivity = a[0].abs();
            next.gyro.vertical_ratio = (a[1] / a[0]).abs();
            next.gyro.motion.invert_x = a[0] < 0.;
            next.gyro.invert_y = a[1] < 0.;
            next.gyro.acceleration.fast_multiplier = b[0] / a[0];
            if b != a {
                next.gyro.acceleration.start_dps = number(&settings, "MIN_GYRO_THRESHOLD", 0.)?;
                next.gyro.acceleration.full_dps = number(&settings, "MAX_GYRO_THRESHOLD", 75.)?;
            }
            next.validate()?;
            p = next;
            Ok(())
        })();
        for s in low.into_iter().chain(high) {
            match &sens {
                Ok(()) => mark(
                    &mut lines,
                    s.index,
                    "converted",
                    "已转换体感灵敏度、纵横比例与快速转动倍率",
                ),
                Err(error) => mark(
                    &mut lines,
                    s.index,
                    "skipped",
                    format!("体感灵敏度未转换：{error:#}"),
                ),
            }
        }
        for key in ["MIN_GYRO_THRESHOLD", "MAX_GYRO_THRESHOLD"] {
            if let Some(s) = settings.get(key) {
                mark(
                    &mut lines,
                    s.index,
                    if sens.is_ok() { "converted" } else { "skipped" },
                    "与 MIN/MAX_GYRO_SENS 一起转换；相同倍率时不启用加速",
                );
            }
        }
    }
    let sensitivity_enabled = p.gyro.enabled;
    let mut pad_trigger = [false; 2];
    for key in [
        "TRIGGER_THRESHOLD",
        "TRIGGER_SKIP_DELAY",
        "ZL_MODE",
        "ZR_MODE",
    ] {
        let Some(s) = settings.get(key) else { continue };
        let parsed = jsm_compile(&format!("{key} = {}", s.value), &[]);
        if !parsed
            .lines
            .first()
            .is_some_and(|line| matches!(line.status, JsmLineStatus::Ok))
        {
            mark(&mut lines, s.index, "skipped", "扳机模式或参数无效");
            continue;
        }
        let analog = parsed.settings;
        if matches!(key, "ZL_MODE" | "ZR_MODE") {
            pad_trigger[usize::from(key == "ZR_MODE")] = (if key == "ZL_MODE" {
                analog.zl
            } else {
                analog.zr
            })
            .pad_side()
            .is_some();
        }
        apply(&mut p, &mut lines, s, |p| {
            let trigger = &mut p.trigger_stages;
            match key {
                "TRIGGER_THRESHOLD" => {
                    trigger.soft_threshold =
                        if analog.threshold < 0. && analog.threshold.is_finite() {
                            -1.
                        } else {
                            analog.threshold
                        };
                }
                "TRIGGER_SKIP_DELAY" => trigger.skip_delay_ms = analog.skip_delay * 1000.,
                _ => {
                    let right = key == "ZR_MODE";
                    let mode = if right { analog.zr } else { analog.zl };
                    if let Some(side) = mode.pad_side() {
                        if p.virtual_gamepad == GamepadTarget::None || side != usize::from(right) {
                            bail!("未启用完整标准虚拟手柄映射");
                        }
                        return Ok(None);
                    }
                    if right {
                        trigger.right_mode = mode;
                    } else {
                        trigger.left_mode = mode;
                    }
                }
            }
            Ok(None)
        });
    }
    if let Some(s) = settings.get("GYRO_GATE") {
        apply(&mut p, &mut lines, s, |p| {
            if s.key == "GYRO_ON" && s.value == "NONE" {
                p.gyro.enabled = false;
                return Ok(None);
            }
            p.gyro.activation = "always".into();
            p.gyro.pause_input = None;
            if s.value != "NONE" {
                let pin = input(&s.value).context("体感开关需要一个支持的手柄输入")?;
                if s.key == "GYRO_ON" {
                    p.gyro.activation = pin.into();
                } else {
                    p.gyro.pause_input = Some(pin.into());
                }
            }
            Ok(None)
        });
    }
    // Dependent settings first. Individual invalid values leave the other settings intact.
    for s in settings.values() {
        if matches!(
            s.key.as_str(),
            "GYRO_SENS"
                | "MIN_GYRO_SENS"
                | "MAX_GYRO_SENS"
                | "REAL_WORLD_CALIBRATION"
                | "IN_GAME_SENS"
                | "MIN_GYRO_THRESHOLD"
                | "MAX_GYRO_THRESHOLD"
                | "GYRO_ON"
                | "GYRO_OFF"
                | "VIRTUAL_CONTROLLER"
                | "TRIGGER_THRESHOLD"
                | "TRIGGER_SKIP_DELAY"
                | "ZL_MODE"
                | "ZR_MODE"
        ) {
            continue;
        }
        apply(&mut p, &mut lines, s, |p| {
            let v = s.value.as_str();
            let g = &mut p.gyro;
            let note = match s.key.as_str() {
                "GYRO_SPACE" => {
                    g.motion.space = match v {
                        "LOCAL" => Space::PitchYaw,
                        "PLAYER" | "PLAYER_TURN" => Space::Player,
                        "WORLD" | "WORLD_TURN" => Space::World,
                        _ => bail!("此坐标模式暂不支持"),
                    };
                    (v != "LOCAL").then_some("已转换到对应坐标模式；姿态估计与上游手感需真机核验")
                }
                "MOUSE_X_FROM_GYRO_AXIS" => {
                    g.motion.space = match v {
                        "Y" => g.motion.space,
                        "Z" if g.motion.space == Space::PitchYaw => Space::PitchRoll,
                        _ => bail!("仅支持默认偏航或本地横滚轴"),
                    };
                    None
                }
                "MOUSE_Y_FROM_GYRO_AXIS" if v == "X" => None,
                "GYRO_AXIS_X" | "GYRO_AXIS_Y" => {
                    let invert = match v {
                        "STANDARD" | "1" => false,
                        "INVERTED" | "-1" => true,
                        _ => bail!("轴向仅支持 STANDARD／INVERTED"),
                    };
                    if s.key.ends_with('X') {
                        g.motion.invert_x ^= invert;
                    } else {
                        g.invert_y ^= invert;
                    }
                    None
                }
                "GYRO_OUTPUT" => {
                    g.output = match v { "MOUSE" => GyroOutput::Mouse, "RIGHT_STICK" if p.virtual_gamepad != GamepadTarget::None => GyroOutput::RightStick, _ => bail!("此体感输出暂不支持；右摇杆需要完整标准虚拟手柄配置，PS_MOTION 原始传感器转发不支持") };
                    None
                }
                "VIRTUAL_STICK_CALIBRATION" => {
                    g.stick_full_scale_dps = scalar(v)?;
                    None
                }
                "GYRO_CUTOFF_SPEED" => {
                    g.motion.cutoff_dps = scalar(v)?;
                    Some("已设置低速截止；平滑与截止恢复算法可能不同")
                }
                "GYRO_SMOOTH_TIME" => {
                    g.smoothing_ms = scalar(v)? * 1000.;
                    Some("秒已换算为毫秒；使用 Motion Studio 的时间平滑，最多 50 ms")
                }
                "GYRO_SMOOTHING_DECAY" => {
                    g.motion.smoothing = match v {
                        "ON" => crate::motion_config::Smoothing::Decay,
                        "OFF" => crate::motion_config::Smoothing::Ema,
                        _ => bail!("需要 ON 或 OFF"),
                    };
                    Some("已转换平滑方式，原 JSM 阈值平滑与时间平滑并非完全等价")
                }
                "GYRO_SMOOTH_THRESHOLD" => {
                    g.motion.decay_threshold = scalar(v)?;
                    Some("已用于衰减平滑阈值；传统阈值均值平滑未迁移")
                }
                "GYRO_ANGLE_SNAP" => {
                    g.motion.angle_snap = scalar(v)?;
                    None
                }
                "GYRO_ANGLE_SNAP_EASE" => {
                    g.motion.angle_snap_ease = match v {
                        "ON" => true,
                        "OFF" => false,
                        _ => bail!("需要 ON 或 OFF"),
                    };
                    None
                }
                "ONE_EURO_MIN_CUTOFF" => {
                    g.motion.one_euro_cutoff = scalar(v)?;
                    None
                }
                "ONE_EURO_SPEED_COEFF" => {
                    g.motion.one_euro_beta = scalar(v)?;
                    None
                }
                "ONE_EURO_FILTER" => {
                    g.motion.smoothing = crate::motion_config::Smoothing::OneEuro;
                    Some("已启用 One Euro；不叠加原 JSM 的前置阈值平滑")
                }
                "ACCEL_CURVE" => {
                    g.acceleration.curve = match v {
                        "LINEAR" => crate::motion_config::Curve::Linear,
                        "NATURAL" => crate::motion_config::Curve::Natural,
                        "POWER" => crate::motion_config::Curve::Power,
                        "QUADRATIC" => crate::motion_config::Curve::Quadratic,
                        "SIGMOID" => crate::motion_config::Curve::Sigmoid,
                        "JUMP" => crate::motion_config::Curve::Jump,
                        _ => bail!("此加速曲线暂不支持"),
                    };
                    Some("已转换曲线类型；非零 MIN_GYRO_THRESHOLD 下的曲线区间请核验")
                }
                "ACCEL_NATURAL_VHALF" => {
                    g.acceleration.natural_half = scalar(v)?;
                    None
                }
                "ACCEL_POWER_VREF" => {
                    g.acceleration.power_reference = scalar(v)?;
                    None
                }
                "ACCEL_POWER_EXPONENT" => {
                    g.acceleration.power_exponent = scalar(v)?;
                    None
                }
                "ACCEL_SIGMOID_MID" => {
                    g.acceleration.sigmoid_mid = scalar(v)?;
                    None
                }
                "ACCEL_SIGMOID_WIDTH" => {
                    g.acceleration.sigmoid_width = scalar(v)?;
                    None
                }
                "ACCEL_JUMP_TAU" => {
                    g.acceleration.jump_width = scalar(v)?;
                    None
                }
                "DECEL_BRAKE_STRENGTH" => {
                    g.motion.brake_strength = scalar(v)?;
                    Some("已转换减速刹车强度；历史窗口和手感请核验")
                }
                "DECEL_BRAKE_THRESHOLD" => {
                    g.motion.brake_threshold = scalar(v)?;
                    Some("已转换减速刹车阈值；历史窗口和手感请核验")
                }
                "GYRO_ONE_EURO_MIN_CUTOFF" => {
                    g.motion.one_euro_cutoff = scalar(v)?;
                    None
                }
                "GYRO_ONE_EURO_BETA" => {
                    g.motion.one_euro_beta = scalar(v)?;
                    None
                }
                "STICK_DEADZONE_INNER" => {
                    p.stick_deadzone = scalar(v)?;
                    p.stick_mouse.deadzone = p.stick_deadzone;
                    Some("统一用于方向映射与摇杆鼠标；外死区与特殊死区需核验")
                }
                "LEFT_STICK_DEADZONE_INNER" | "RIGHT_STICK_DEADZONE_INNER" => {
                    let value = scalar(v)?;
                    if s.key == "LEFT_STICK_DEADZONE_INNER" {
                        p.input.deadzone_left = Some(value);
                    } else {
                        p.input.deadzone_right = Some(value);
                    }
                    None
                }
                "FLICK_TIME" => {
                    g.motion.flick.smooth_ms = scalar(v)? * 1000.;
                    None
                }
                "FLICK_DEADZONE_ANGLE" => {
                    g.motion.flick.forward_deadzone = scalar(v)?;
                    None
                }
                "FLICK_THRESHOLD" => {
                    g.motion.flick.deadzone = scalar(v)?;
                    None
                }
                "LEFT_STICK_MODE" | "RIGHT_STICK_MODE" | "STICK_SENS" => return Ok(None), // Applied together below.
                "AUTOCONNECT" | "AUTOLOAD" | "TELEMETRY_ENABLED" | "TELEMETRY_PORT"
                | "TICK_TIME" | "LIGHT_BAR" | "RUMBLE" | "HIDE_MINIMIZED" | "JSM_DIRECTORY" => {
                    bail!("JSM 程序设置不写入 Motion Studio 游戏配置")
                }
                _ => bail!("此设置、按住切换设置或输入暂不支持"),
            };
            Ok(note)
        });
        if matches!(
            s.key.as_str(),
            "AUTOCONNECT"
                | "AUTOLOAD"
                | "TELEMETRY_ENABLED"
                | "TELEMETRY_PORT"
                | "TICK_TIME"
                | "LIGHT_BAR"
                | "RUMBLE"
                | "HIDE_MINIMIZED"
                | "JSM_DIRECTORY"
        ) {
            mark(
                &mut lines,
                s.index,
                "ignored",
                "本机 JSM 程序设置，未执行也未写入游戏配置",
            );
        }
    }
    let gyro_blocked = settings.iter().any(|(key, s)| {
        matches!(
            key.as_str(),
            "GYRO_GATE"
                | "GYRO_SPACE"
                | "GYRO_OUTPUT"
                | "MOUSE_X_FROM_GYRO_AXIS"
                | "MOUSE_Y_FROM_GYRO_AXIS"
                | "REAL_WORLD_CALIBRATION"
                | "IN_GAME_SENS"
        ) && lines[s.index].status == "skipped"
    });
    if gyro_blocked {
        p.gyro.enabled = false;
        warnings.push("体感的输出、坐标、开关或校准存在无法迁移的关键项，已关闭体感；核验后可在体感调校中手动开启。".into());
    }
    // Ordinary actions only. Side effects and multi-event bindings remain in the report.
    let mut ordered: Vec<_> = bindings.values().collect();
    ordered.sort_by_key(|s| s.index);
    for s in ordered {
        let (input, approximate) = trigger(&s.key)?;
        if s.value == "NONE" {
            mark(&mut lines, s.index, "converted", "此自定义动作不绑定输出");
            continue;
        }
        let trigger_parts = [
            (
                "left_trigger_soft",
                "left_trigger_full",
                p.trigger_stages.left_mode,
            ),
            (
                "right_trigger_soft",
                "right_trigger_full",
                p.trigger_stages.right_mode,
            ),
        ];
        if trigger_parts
            .iter()
            .zip(pad_trigger)
            .any(|((soft, full, _), pad)| {
                pad && input.split('+').any(|part| part == *soft || part == *full)
            })
        {
            mark(
                &mut lines,
                s.index,
                "skipped",
                "虚拟扳机透传模式下的绑定需要手动核对",
            );
            continue;
        }
        let full_disabled = trigger_parts
            .iter()
            .any(|(_, full, mode)| !mode.has_full() && input.split('+').any(|part| part == *full));
        let full_note = "已保留全拉绑定。请在扳机设置中启用该侧的全拉模式";
        let gyro_action = s.value.trim_end_matches('\\');
        if !input.contains('+')
            && matches!(
                gyro_action,
                "GYRO_ON"
                    | "^GYRO_ON"
                    | "GYRO_OFF"
                    | "GYRO_TRACK_X"
                    | "GYRO_TRACK_Y"
                    | "GYRO_TRACKBALL"
            )
        {
            apply(&mut p, &mut lines, s, |p| {
                match gyro_action {
                    "GYRO_ON" | "^GYRO_ON" => {
                        if !settings
                            .get("GYRO_GATE")
                            .is_some_and(|s| s.key == "GYRO_ON" && s.value == "NONE")
                            || !sensitivity_enabled
                            || gyro_blocked
                            || p.gyro.activation != "always"
                        {
                            bail!("此绑定与基础体感开关冲突；仅能从 GYRO_ON=NONE 转为一个按住／切换开启键");
                        }
                        p.gyro.enabled = true;
                        p.gyro.activation = input.clone();
                        p.gyro.activation_mode = if gyro_action.starts_with('^') {
                            crate::profile::GyroActivationMode::Toggle
                        } else {
                            crate::profile::GyroActivationMode::Hold
                        };
                    }
                    "GYRO_OFF" => {
                        if p.gyro.pause_input.is_some() {
                            bail!("已有体感暂停按键，不能叠加此绑定");
                        }
                        p.gyro.pause_input = Some(input.clone());
                    }
                    action => {
                        let t = &mut p.gyro.motion.trackball;
                        if (action != "GYRO_TRACK_Y" && t.horizontal_input.is_some())
                            || (action != "GYRO_TRACK_X" && t.vertical_input.is_some())
                        {
                            bail!("同一轴只支持一个惯性体感按键");
                        }
                        if action != "GYRO_TRACK_Y" {
                            t.horizontal_input = Some(input.clone());
                        }
                        if action != "GYRO_TRACK_X" {
                            t.vertical_input = Some(input.clone());
                        }
                    }
                }
                Ok(full_disabled.then_some(full_note))
            });
            continue;
        }
        if native_output(&s.value) == Some(input.as_str())
            && p.virtual_gamepad != GamepadTarget::None
        {
            mark(
                &mut lines,
                s.index,
                "converted",
                "已包含在标准虚拟手柄布局中",
            );
            continue;
        }
        let value = match outputs(&s.value) {
            Ok(v) => v,
            Err(error) => {
                mark(&mut lines, s.index, "skipped", format!("未转换：{error:#}"));
                continue;
            }
        };
        if p.actions.len() >= 128 {
            mark(
                &mut lines,
                s.index,
                "skipped",
                "已达到每份配置 128 个动作的上限",
            );
            continue;
        }
        let name = labels
            .get(&(s.key.clone(), s.value.clone()))
            .cloned()
            .unwrap_or_else(|| format!("{} → {}", s.key, s.value));
        let mut directional = false;
        for part in input.split('+') {
            if let Some((side, direction)) = part.split_once("_stick_") {
                if matches!(direction, "up" | "down" | "left" | "right") {
                    directional = true;
                    if side == "left" {
                        p.input.lstick_directions = StickDirections::EightWay;
                    } else {
                        p.input.rstick_directions = StickDirections::EightWay;
                    }
                }
            }
        }
        p.actions.push(Action {
            name,
            // Native NO_MOUSE directions activate immediately beyond the input deadzone.
            threshold: if directional { 0. } else { 0.5 },
            input: input.clone(),
            output: value,
            repeat_ms: None,
            trigger: Default::default(),
            trigger_ms: 250,
            sustain: true,
            turbo_ms: None,
            allow_simultaneous: false,
        });
        let note = if full_disabled {
            full_note
        } else if approximate {
            "已转为同时按住组合；原 JSM 的按下顺序／同时按压时间窗未保留"
        } else if directional {
            "已采用 JSM 八向重叠判定；斜向同时输出相邻两键，输入死区与其他摇杆模式需核验"
        } else if input.contains("stick_") {
            "已转为摇杆区域输入；区域边界需核验"
        } else if input == "touchpad" {
            "CAPTURE 已转为触摸板点按；其他手柄的 Capture 键请改选对应扩展键"
        } else {
            "已转换为按住输出；滚轮每次触发一格"
        };
        mark(
            &mut lines,
            s.index,
            if full_disabled || approximate || input.contains("stick_") || input == "touchpad" {
                "approximate"
            } else {
                "converted"
            },
            note,
        );
    }
    // A GYRO_ON binding can enable gyro with a hold or toggle gate after GYRO_ON=NONE.
    let mut sticks: Vec<_> = ["LEFT_STICK_MODE", "RIGHT_STICK_MODE"]
        .iter()
        .filter_map(|key| settings.get(*key))
        .collect();
    sticks.sort_by_key(|s| s.index);
    let aim_requested = sticks.iter().any(|s| s.value == "AIM");
    let mut chosen = false;
    for s in sticks {
        apply(&mut p, &mut lines, s, |p| {
            match s.value.as_str() {
                "NO_MOUSE" => {
                    if s.key.starts_with("LEFT") {
                        p.input.lstick_directions = StickDirections::EightWay;
                    } else {
                        p.input.rstick_directions = StickDirections::EightWay;
                    }
                    return Ok(None);
                }
                "LEFT_STICK" | "RIGHT_STICK" if p.virtual_gamepad != GamepadTarget::None => {
                    return Ok(None)
                }
                "AIM" | "FLICK" if !chosen => {}
                "AIM" | "FLICK" => bail!("当前只能把一个摇杆用于瞄准／Flick，请手动选择"),
                _ => bail!("该摇杆模式暂不能迁移"),
            }
            if gyro_blocked {
                bail!("体感存在无法等价迁移的关键项，请核验摇杆瞄准／Flick 设置");
            }
            let f = &mut p.gyro.motion.flick;
            f.standalone = !p.gyro.enabled;
            f.stick = if s.key.starts_with("LEFT") {
                Stick::Left
            } else {
                Stick::Right
            };
            f.enabled = s.value == "FLICK";
            if f.enabled {
                f.release_ease = 0.5;
            }
            f.stick_aim = s.value == "AIM";
            if f.stick_aim {
                let [horizontal, vertical] = settings
                    .get("STICK_SENS")
                    .map(|s| pair(&s.value))
                    .unwrap_or(Ok([360.; 2]))?;
                f.max_dps = horizontal;
                f.vertical_ratio = vertical / horizontal;
            }
            Ok(Some(
                "已转换为摇杆瞄准／Flick；摇杆加速度、门限和独立轴设置需核验",
            ))
        });
        if lines[s.index].status != "skipped" && matches!(s.value.as_str(), "AIM" | "FLICK") {
            chosen = true;
        }
    }
    if let Some(s) = settings.get("STICK_SENS") {
        if p.gyro.motion.flick.enabled && !aim_requested {
            mark(&mut lines, s.index, "ignored", "STICK_SENS 仅影响 AIM 摇杆瞄准");
        } else if !p.gyro.motion.flick.stick_aim {
            mark(
                &mut lines,
                s.index,
                "skipped",
                "未转换对应的 AIM 摇杆瞄准模式，灵敏度未应用",
            );
        }
    }
    p.materialize_gamepad_bindings();
    p.validate().context("转换后的配置校验失败")?;
    if lines.is_empty() {
        bail!("没有找到 JSM 配置命令");
    }
    let converted = lines.iter().filter(|l| l.status == "converted").count();
    let approximate = lines.iter().filter(|l| l.status == "approximate").count();
    let skipped = lines.iter().filter(|l| l.status == "skipped").count();
    if skipped > 0 {
        warnings.push(format!("有 {skipped} 行未迁移，请查看逐行报告并补配。"));
    }
    if approximate > 0 {
        warnings.push(format!(
            "有 {approximate} 行按现有功能转换，触发方式或手感需核验。"
        ));
    }
    Ok(JsmImport {
        profile: p,
        lines,
        warnings,
        converted,
        approximate,
        skipped,
    })
}
