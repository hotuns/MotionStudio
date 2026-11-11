//! Hold layers merge bounded configuration patches over one physical session.
use crate::profile::{Action, Profile};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Layer {
    pub id: String,
    pub name: String,
    pub input: String,
    #[serde(default = "default_threshold")]
    pub activation_threshold: f32,
    #[serde(default = "default_hysteresis")]
    pub activation_hysteresis: f32,
    #[serde(default, deserialize_with = "deserialize_settings")]
    pub settings: Map<String, Value>,
    #[serde(default)]
    pub actions: Vec<Action>,
    #[serde(default)]
    pub disabled_inputs: Vec<String>,
}
fn default_threshold() -> f32 {
    0.5
}
fn default_hysteresis() -> f32 {
    0.1
}
fn deserialize_settings<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Map<String, Value>, D::Error> {
    let mut settings = Map::<String, Value>::deserialize(deserializer)?;
    let Some(Value::Object(gyro)) = settings.get_mut("gyro") else {
        return Ok(settings);
    };
    let Some(Value::Object(motion)) = gyro.get_mut("motion") else {
        return Ok(settings);
    };
    let Some(Value::Object(flick)) = motion.get_mut("flick") else {
        return Ok(settings);
    };
    // Flick follows gyro.output. Drop obsolete layer overrides and empty groups.
    if flick.remove("output").is_some() && flick.is_empty() {
        motion.remove("flick");
        if motion.is_empty() {
            gyro.remove("motion");
        }
        if gyro.is_empty() {
            settings.remove("gyro");
        }
    }
    Ok(settings)
}

pub fn merge(target: &mut Value, patch: &Value) {
    if patch
        .as_object()
        .is_some_and(|m| m.len() == 1 && m.get("clear") == Some(&Value::Bool(true)))
    {
        *target = Value::Null;
        return;
    }
    if let (Some(dst), Some(src)) = (target.as_object_mut(), patch.as_object()) {
        for (key, value) in src {
            merge(dst.entry(key).or_insert(Value::Null), value);
        }
    } else {
        *target = patch.clone();
    }
}
fn input_key(input: &str) -> String {
    let mut parts: Vec<_> = input.split('+').map(str::trim).collect();
    parts.sort();
    parts.join("+")
}
pub fn resolve(base: &Profile, active: &[usize]) -> Result<Profile> {
    let mut value = serde_json::to_value(base)?;
    value["layers"] = Value::Array(vec![]);
    let mut actions = base.actions.clone();
    for &index in active {
        let layer = base.layers.get(index).context("配置层不存在")?;
        merge(&mut value, &Value::Object(layer.settings.clone()));
        actions.retain(|a| {
            !layer
                .disabled_inputs
                .iter()
                .chain(layer.actions.iter().map(|a| &a.input))
                .any(|input| input_key(input) == input_key(&a.input))
        });
        actions.extend(layer.actions.clone());
    }
    value["actions"] = serde_json::to_value(actions)?;
    Ok(serde_json::from_value(value)?)
}
pub fn validate(base: &Profile) -> Result<()> {
    if base.layers.is_empty() {
        return Ok(());
    }
    ensure!(
        base.layers.len() <= 8,
        "每个游戏配置最多支持 8 个按住配置层"
    );
    ensure!(
        base.actions.len() + base.layers.iter().map(|l| l.actions.len()).sum::<usize>() <= 128,
        "基础层与所有配置层合计最多 128 个动作"
    );
    let mut ids = std::collections::HashSet::new();
    for layer in &base.layers {
        ensure!(
            layer.activation_threshold.is_finite()
                && (0.05..=0.95).contains(&layer.activation_threshold)
                && layer.activation_hysteresis.is_finite()
                && (0.0..=0.3).contains(&layer.activation_hysteresis)
                && layer.activation_hysteresis < layer.activation_threshold,
            "配置层触发阈值或释放回差无效"
        );
        ensure!(
            !layer.id.is_empty() && layer.id.len() <= 64 && ids.insert(&layer.id),
            "配置层标识重复或无效"
        );
        ensure!(
            !layer.name.trim().is_empty() && layer.name.chars().count() <= 60,
            "配置层名称需为 1–60 个字符"
        );
        ensure!(
            crate::bindings::input_chord(&layer.input).is_some(),
            "配置层启用输入无效"
        );
        ensure!(
            layer.settings.keys().all(|k| matches!(
                k.as_str(),
                "gyro"
                    | "gamepad_bindings"
                    | "input"
                    | "feedback"
                    | "touchpad"
                    | "stick_mouse"
                    | "stick_deadzone"
                    | "stick_zones"
                    | "stick_rebound_ms"
            )),
            "配置层只能覆盖体感、摇杆和动作，不能改变输出设备"
        );
        ensure!(
            layer
                .settings
                .get("gamepad_bindings")
                .is_none_or(Value::is_array),
            "配置层的手柄映射需为列表"
        );
        ensure!(
            layer.disabled_inputs.len() <= 128
                && layer
                    .disabled_inputs
                    .iter()
                    .all(|v| crate::bindings::input_chord(v).is_some()),
            "配置层禁用输入无效"
        );
    }
    // At most eight layers means 255 nonempty combinations. Validating only
    // each layer and the full stack misses conflicts masked by a later layer.
    for mask in 1..(1usize << base.layers.len()) {
        let active: Vec<_> = (0..base.layers.len())
            .filter(|index| mask & (1 << index) != 0)
            .collect();
        resolve(base, &active)?.validate().with_context(|| {
            format!(
                "配置层组合 {} 无效",
                active
                    .iter()
                    .map(|index| base.layers[*index].name.as_str())
                    .collect::<Vec<_>>()
                    .join(" + ")
            )
        })?;
    }
    Ok(())
}
