use crate::{apply_cursor_profile, apply_profile, AppState};
use motion_core::protocol::{Command, Status};
use motion_core::shortcut::{Modifiers, PadShortcut, ShortcutEdges, ShortcutTrigger};
use motion_core::storage::SwitchTarget;
use motion_core::switching::CursorEdges;
use std::{
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

pub fn start(state: AppState) {
    std::thread::spawn(move || {
        let mut cursor = CursorEdges::default();
        let mut shortcut_edges = Vec::new();
        let mut capture_revision = 0;
        let mut last_rules = Vec::new();
        let mut last_cursor_settings = None;
        let mut pending_edge = None;
        let mut pending_shortcuts = Vec::new();
        let mut pad_cursor = PadShortcutCursor::default();
        while !state.client.stopping.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(50));
            let settings = state.settings.lock().unwrap().clone();
            let snapshot = state.client.snapshot.lock().unwrap().clone();
            let rules = settings.effective_shortcut_rules();
            let (pad_rule_indices, desired_pad): (Vec<_>, Vec<_>) = rules
                .iter()
                .enumerate()
                .filter_map(|(index, rule)| match &rule.trigger {
                    ShortcutTrigger::Gamepad { input } => Some((
                        index,
                        PadShortcut {
                            input: input.clone(),
                            target: rule.target.clone(),
                        },
                    )),
                    ShortcutTrigger::Keyboard { .. } => None,
                })
                .unzip();
            if snapshot.engine_ready && snapshot.status.pad_shortcuts != desired_pad {
                if let Ok(_operation) = state.operations.try_lock() {
                    let rules_current = state.settings.lock().unwrap().effective_shortcut_rules() == rules;
                    if rules_current {
                        if let Err(error) = state.client.request(Command::SetPadShortcuts {
                            bindings: desired_pad,
                        }) {
                            state.client.snapshot.lock().unwrap().policy_error =
                                Some(format!("手柄快捷键未配置：{error}"));
                        }
                    }
                }
                pending_shortcuts.clear();
                continue;
            }
            let running = matches!(snapshot.status.state.as_str(), "running" | "observing")
                && !snapshot.status.ui_navigation
                && !snapshot.status.binding_capture
                && !state.game_calibration.is_open()
                && snapshot.status.motion.measuring.is_none();
            let capture = state
                .shortcut_capture_until
                .lock()
                .unwrap()
                .is_some_and(|until| Instant::now() < until);
            let current_capture_revision = state.shortcut_capture_revision.load(Ordering::Acquire);
            if rebase_after_capture(
                &mut shortcut_edges,
                &mut capture_revision,
                current_capture_revision,
            ) {
                pending_shortcuts.clear();
            }
            let down = |key: u16| unsafe {
                windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(i32::from(key))
                    < 0
            };
            if last_rules != rules {
                shortcut_edges.clear();
                shortcut_edges.resize_with(rules.len(), ShortcutEdges::default);
                pending_shortcuts.clear();
                last_rules.clone_from(&rules);
            }
            let mut presses: Vec<_> = pad_cursor
                .update(
                    &snapshot.status,
                    &desired_pad,
                    snapshot.restart_count,
                    running && !capture,
                )
                .into_iter()
                .map(|index| pad_rule_indices[index])
                .collect();
            let modifiers = Modifiers {
                ctrl: down(0x11),
                alt: down(0x12),
                shift: down(0x10),
                windows: down(0x5b) || down(0x5c),
            };
            for (index, (rule, edges)) in rules.iter().zip(&mut shortcut_edges).enumerate() {
                let ShortcutTrigger::Keyboard { key } = &rule.trigger else {
                    continue;
                };
                let pressed = edges.update(
                    key,
                    Some(&rule.target),
                    key.virtual_key().is_some_and(down),
                    modifiers,
                    running && !capture,
                );
                // Coalesce simultaneous presses only when their actions match.
                if pressed && !presses.iter().any(|&other| {
                    rules[other].target == rule.target
                        && rules[other].return_on_repeat == rule.return_on_repeat
                }) {
                    presses.push(index);
                }
            }
            if !running || capture {
                cursor = CursorEdges::default();
                last_cursor_settings = None;
                pending_edge = None;
                pending_shortcuts.clear();
                continue;
            }
            let cursor_settings = (
                settings.cursor_enabled,
                settings.cursor_debounce_ms,
                settings.cursor_shown.clone(),
                settings.cursor_hidden.clone(),
            );
            if last_cursor_settings.as_ref() != Some(&cursor_settings) {
                cursor = CursorEdges::default();
                last_cursor_settings = Some(cursor_settings);
                pending_edge = None;
            }
            let edge = if settings.cursor_enabled {
                use windows_sys::Win32::UI::WindowsAndMessaging::{
                    GetCursorInfo, CURSORINFO, CURSOR_SHOWING,
                };
                let mut info: CURSORINFO = unsafe { std::mem::zeroed() };
                info.cbSize = std::mem::size_of::<CURSORINFO>() as u32;
                if unsafe { GetCursorInfo(&mut info) } != 0 {
                    cursor.update_with_delay(
                        info.flags & CURSOR_SHOWING != 0,
                        Instant::now(),
                        Duration::from_millis(u64::from(settings.cursor_debounce_ms)),
                    )
                } else {
                    None
                }
            } else {
                None
            };
            if edge.is_some() {
                pending_edge = edge;
            }
            pending_shortcuts.extend(presses);
            let overflow = pending_shortcuts.len().saturating_sub(64);
            pending_shortcuts.drain(..overflow);
            if pending_edge.is_none() && pending_shortcuts.is_empty() {
                continue;
            }
            let Ok(_operation) = state.operations.try_lock() else {
                continue;
            };
            let current_settings = state.settings.lock().unwrap().clone();
            if current_settings.effective_shortcut_rules() != rules {
                pending_shortcuts.clear();
                continue;
            }
            if current_settings.cursor_enabled != settings.cursor_enabled
                || current_settings.cursor_debounce_ms != settings.cursor_debounce_ms
                || current_settings.cursor_shown != settings.cursor_shown
                || current_settings.cursor_hidden != settings.cursor_hidden
            {
                pending_edge = None;
            }
            let edge = pending_edge.take();
            let shortcuts = std::mem::take(&mut pending_shortcuts);
            let shortcut = !shortcuts.is_empty();
            if (shortcut || edge.is_some())
                && (state.shortcut_capture_revision.load(Ordering::Acquire)
                    != current_capture_revision
                    || state
                        .shortcut_capture_until
                        .lock()
                        .unwrap()
                        .is_some_and(|until| Instant::now() < until))
            {
                continue;
            }
            let latest_status = state.client.snapshot.lock().unwrap().status.clone();
            if !matches!(latest_status.state.as_str(), "running" | "observing")
                || latest_status.binding_capture
                || latest_status.ui_navigation
                || state.game_calibration.is_open()
                || latest_status.motion.measuring.is_some()
            {
                continue;
            }
            let current = latest_status.profile_id.clone();
            let Some(current) = current else {
                continue;
            };
            let mut history = state.history.lock().unwrap().clone();
            let target = if shortcut {
                Some(shortcuts.into_iter().fold(current.clone(), |from, index| {
                    let rule = &rules[index];
                    let next = if rule.return_on_repeat {
                        history.toggle(&from, &rule.target)
                    } else {
                        history.apply(&from, &SwitchTarget::Profile(rule.target.clone()))
                    };
                    next.unwrap_or(from)
                }))
            } else {
                edge.and_then(|shown| {
                    history.apply(
                        &current,
                        if shown {
                            &settings.cursor_shown
                        } else {
                            &settings.cursor_hidden
                        },
                    )
                })
            };
            if let Some(target) = target {
                if target == current {
                    *state.history.lock().unwrap() = history;
                    continue;
                }
                let result = if shortcut {
                    apply_profile(&state, &target)
                } else {
                    apply_cursor_profile(&state, &target)
                };
                match result {
                    Ok(()) => {
                        *state.history.lock().unwrap() = history;
                        state.client.snapshot.lock().unwrap().policy_error = None;
                        crate::notify_profile(&state, &target);
                    }
                    Err(error) => {
                        state.client.snapshot.lock().unwrap().policy_error =
                            Some(format!("自动切换未执行：{error}"));
                    }
                }
            }
        }
    });
}

#[derive(Default)]
struct PadShortcutCursor {
    source: Option<(u32, Vec<PadShortcut>)>,
    serial: u64,
}
impl PadShortcutCursor {
    fn update(
        &mut self,
        status: &Status,
        desired: &[PadShortcut],
        restart: u32,
        enabled: bool,
    ) -> Vec<usize> {
        if self.source.as_ref().is_none_or(|(previous_restart, bindings)| {
            *previous_restart != restart || bindings.as_slice() != desired
        }) {
            self.serial = status.pad_shortcut_baseline;
            self.source = Some((restart, desired.to_vec()));
        }
        let previous = self.serial.max(status.pad_shortcut_baseline);
        self.serial = status.pad_shortcut_serial;
        if !enabled || status.pad_shortcuts.as_slice() != desired {
            return Vec::new();
        }
        status
            .pad_shortcut_events
            .iter()
            .filter(|event| event.serial > previous && event.index < desired.len())
            .map(|event| event.index)
            .collect()
    }
}

// A quick capture can begin and end between two 50 ms polls. A revision still
// invalidates its old press baseline, including when the same key was recorded.
fn rebase_after_capture(edges: &mut [ShortcutEdges], last: &mut u64, revision: u64) -> bool {
    if *last == revision {
        return false;
    }
    edges.fill_with(ShortcutEdges::default);
    *last = revision;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use motion_core::shortcut::Shortcut;
    #[test]
    fn quick_capture_of_the_same_key_cannot_trigger_on_its_still_held_press() {
        let mut edges = ShortcutEdges::default();
        let mut revision = 0;
        let key = Shortcut::default();
        let m = Modifiers::default();
        assert!(!edges.update(&key, Some("menu"), false, m, true));
        // Start and finish both happened between polls; the key is still down.
        assert!(rebase_after_capture(std::slice::from_mut(&mut edges), &mut revision, 2));
        assert!(!edges.update(&key, Some("menu"), true, m, true));
        assert!(!rebase_after_capture(std::slice::from_mut(&mut edges), &mut revision, 2));
        assert!(!edges.update(&key, Some("menu"), false, m, true));
        assert!(edges.update(&key, Some("menu"), true, m, true));
    }
}

#[cfg(test)]
mod pad_tests {
    use super::*;
    use motion_core::protocol::PadShortcutEvent;
    #[test]
    fn events_preserve_rule_order_and_ignore_capture_baselines_and_old_sources() {
        let bindings = vec![
            PadShortcut { input: "south".into(), target: "aim".into() },
            PadShortcut { input: "east".into(), target: "aim".into() },
        ];
        let mut s = Status {
            pad_shortcuts: bindings.clone(),
            ..Default::default()
        };
        let mut cursor = PadShortcutCursor::default();
        assert!(cursor.update(&s, &bindings, 0, true).is_empty());
        let press = |status: &mut Status, index| {
            status.pad_shortcut_serial += 1;
            status.pad_shortcut_events.push(PadShortcutEvent { serial: status.pad_shortcut_serial, index });
        };
        press(&mut s, 0);
        press(&mut s, 1);
        press(&mut s, 0);
        assert_eq!(cursor.update(&s, &bindings, 0, true), [0, 1, 0]);
        assert!(cursor.update(&s, &bindings, 0, true).is_empty());
        press(&mut s, 1);
        s.pad_shortcut_baseline = s.pad_shortcut_serial;
        assert!(cursor.update(&s, &bindings, 0, true).is_empty());
        press(&mut s, 0);
        assert!(cursor.update(&s, &bindings, 0, false).is_empty());
        assert!(cursor.update(&s, &bindings, 0, true).is_empty());
        press(&mut s, 1);
        s.pad_shortcuts.reverse();
        assert!(cursor.update(&s, &bindings, 0, true).is_empty());
        s = Status { pad_shortcuts: bindings.clone(), ..Default::default() };
        assert!(cursor.update(&s, &bindings, 1, true).is_empty());
        press(&mut s, 1);
        assert_eq!(cursor.update(&s, &bindings, 1, true), [1]);
    }
    #[test]
    fn multiple_presses_resolve_a_toggle_and_restore_without_intermediate_devices() {
        let mut history = motion_core::switching::SwitchHistory::default();
        assert_eq!(
            history.toggle_presses("base", "aim", 2).as_deref(),
            Some("base")
        );
        assert_eq!(
            history.toggle_presses("base", "aim", 3).as_deref(),
            Some("aim")
        );
        assert_eq!(
            history.toggle_presses("aim", "aim", 1).as_deref(),
            Some("base")
        );
    }
}
