//! Small, backwards-compatible chord expressions used by game actions.
pub const MAX_CHORD: usize = 4;
fn parts(value: &str) -> Option<Vec<&str>> {
    let values: Vec<_> = value.split('+').map(str::trim).collect();
    if values.is_empty()
        || values.len() > MAX_CHORD
        || values.iter().any(|v| v.is_empty())
        || values
            .iter()
            .enumerate()
            .any(|(i, v)| values[..i].contains(v))
    {
        return None;
    }
    Some(values)
}
pub fn input_chord(value: &str) -> Option<Vec<&str>> {
    let p = parts(value)?;
    if p.iter().any(|v| crate::profile::input_pin(v).is_none()) {
        return None;
    }
    for pair in [
        ["left_stick_up", "left_stick_down"],
        ["left_stick_left", "left_stick_right"],
        ["right_stick_up", "right_stick_down"],
        ["right_stick_left", "right_stick_right"],
        ["dpad_up", "dpad_down"],
        ["dpad_left", "dpad_right"],
        ["left_stick_inner", "left_stick_outer"],
        ["right_stick_inner", "right_stick_outer"],
    ] {
        if pair.iter().all(|v| p.contains(v)) {
            return None;
        }
    }
    Some(p)
}
pub fn valid_button(value: &str) -> bool {
    value
        .strip_prefix("key:")
        .is_some_and(crate::profile::valid_key)
        || matches!(
            value,
            "mouse:left" | "mouse:right" | "mouse:middle" | "mouse:x1" | "mouse:x2"
        )
}
pub fn wheel_delta(value: &str) -> Option<[i32; 2]> {
    Some(match value {
        "wheel:up" => [0, 1],
        "wheel:down" => [0, -1],
        "wheel:left" => [-1, 0],
        "wheel:right" => [1, 0],
        _ => return None,
    })
}
pub fn output_chord(value: &str) -> Option<Vec<&str>> {
    let p = parts(value)?;
    if p.iter()
        .any(|v| !valid_button(v) && wheel_delta(v).is_none())
        || p.iter().filter(|v| wheel_delta(v).is_some()).count() > 1
    {
        return None;
    }
    Some(p)
}
pub fn modifier(value: &str) -> bool {
    value
        .strip_prefix("key:")
        .and_then(crate::keyboard::key)
        .is_some_and(|key| key.modifier != 0)
}
/// Main buttons release before modifiers; modifiers press before main buttons.
pub fn button_order(keys: &std::collections::BTreeSet<String>, down: bool) -> Vec<String> {
    let mut keys: Vec<_> = keys.iter().cloned().collect();
    keys.sort_by_key(|v| u8::from(modifier(v) != down));
    keys
}
