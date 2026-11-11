use crate::{apply_profile, AppState};
use motion_core::protocol::{Command, Status};
use motion_core::shortcut::{Modifiers, PadShortcut, ShortcutEdges};
use motion_core::switching::CursorEdges;
use std::{
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

pub fn start(state: AppState) {
    std::thread::spawn(move || {
        let mut cursor = CursorEdges::default();
        let mut shortcut_edges = ShortcutEdges::default();
        let mut capture_revision = 0;
        let mut last_binding = None;
        let mut last_cursor_settings = None;
        let mut pending_edge = None;
        let mut pending_shortcut = false;
        let mut pending_pad = 0u64;
        let mut pad_cursor = PadShortcutCursor::default();
        while !state.client.stopping.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(50));
            let settings = state.settings.lock().unwrap().clone();
            let snapshot = state.client.snapshot.lock().unwrap().clone();
            let desired_pad = settings
                .shortcut_pad
                .as_ref()
                .zip(settings.shortcut_profile.as_ref())
                .map(|(input, target)| PadShortcut {
                    input: input.clone(),
                    target: target.clone(),
                });
            if snapshot.engine_ready && snapshot.status.pad_shortcut != desired_pad {
                if let Ok(_operation) = state.operations.try_lock() {
                    let latest = state.settings.lock().unwrap().clone();
                    if latest.shortcut_pad == settings.shortcut_pad
                        && latest.shortcut_profile == settings.shortcut_profile
                    {
                        if let Err(error) = state.client.request(Command::SetPadShortcut {
                            binding: desired_pad.clone(),
                        }) {
                            state.client.snapshot.lock().unwrap().policy_error =
                                Some(format!("手柄快捷键未配置：{error}"));
                        }
                    }
                }
                pending_pad = 0;
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
                pending_shortcut = false;
                pending_pad = 0;
            }
            let down = |key: u16| unsafe {
                windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(i32::from(key))
                    < 0
            };
            let binding = (
                settings.shortcut_key.clone(),
                settings.shortcut_profile.clone(),
                settings.shortcut_pad.clone(),
            );
            let changed = last_binding.as_ref() != Some(&binding);
            if changed {
                shortcut_edges = ShortcutEdges::default();
            }
            if changed || capture {
                pending_shortcut = false;
                pending_pad = 0;
            }
            last_binding = Some(binding);
            let pad_presses = pad_cursor.update(
                &snapshot.status,
                desired_pad.as_ref(),
                snapshot.restart_count,
                running && !capture,
            );
            let shortcut = settings.shortcut_key.as_ref().is_some_and(|key| {
                shortcut_edges.update(
                    key,
                    settings.shortcut_profile.as_deref(),
                    key.virtual_key().is_some_and(down),
                    Modifiers {
                        ctrl: down(0x11),
                        alt: down(0x12),
                        shift: down(0x10),
                        windows: down(0x5b) || down(0x5c),
                    },
                    running && !capture,
                )
            });
            if !running || capture {
                cursor = CursorEdges::default();
                last_cursor_settings = None;
                pending_edge = None;
                pending_shortcut = false;
                pending_pad = 0;
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
            pending_shortcut |= shortcut && settings.shortcut_profile.is_some();
            pending_pad = pending_pad.saturating_add(pad_presses);
            if pending_edge.is_none() && !pending_shortcut && pending_pad == 0 {
                continue;
            }
            let Ok(_operation) = state.operations.try_lock() else {
                continue;
            };
            let current_settings = state.settings.lock().unwrap().clone();
            if current_settings.shortcut_key != settings.shortcut_key
                || current_settings.shortcut_profile != settings.shortcut_profile
                || current_settings.shortcut_pad != settings.shortcut_pad
            {
                pending_shortcut = false;
                pending_pad = 0;
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
            let keyboard = std::mem::take(&mut pending_shortcut);
            let pad = std::mem::take(&mut pending_pad);
            let presses = pad.max(u64::from(keyboard));
            let shortcut = presses > 0;
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
                settings
                    .shortcut_profile
                    .as_ref()
                    .and_then(|id| history.toggle_presses(&current, id, presses))
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
                match apply_profile(&state, &target) {
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
    source: Option<(u32, Option<PadShortcut>)>,
    serial: u64,
}
impl PadShortcutCursor {
    fn update(
        &mut self,
        status: &Status,
        desired: Option<&PadShortcut>,
        restart: u32,
        enabled: bool,
    ) -> u64 {
        let identity = (restart, desired.cloned());
        if self.source.as_ref() != Some(&identity) {
            self.serial = status.pad_shortcut_baseline;
            self.source = Some(identity);
        }
        let previous = self.serial.max(status.pad_shortcut_baseline);
        self.serial = status.pad_shortcut_serial;
        if !enabled || desired.is_none() || status.pad_shortcut.as_ref() != desired {
            return 0;
        }
        status.pad_shortcut_serial.saturating_sub(previous)
    }
}

// A quick capture can begin and end between two 50 ms polls. A revision still
// invalidates its old press baseline, including when the same key was recorded.
fn rebase_after_capture(edges: &mut ShortcutEdges, last: &mut u64, revision: u64) -> bool {
    if *last == revision {
        return false;
    }
    *edges = ShortcutEdges::default();
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
        assert!(rebase_after_capture(&mut edges, &mut revision, 2));
        assert!(!edges.update(&key, Some("menu"), true, m, true));
        assert!(!rebase_after_capture(&mut edges, &mut revision, 2));
        assert!(!edges.update(&key, Some("menu"), false, m, true));
        assert!(edges.update(&key, Some("menu"), true, m, true));
    }
}

#[cfg(test)]
mod pad_tests {
    use super::*;
    #[test]
    fn event_counter_preserves_short_taps_and_ignores_capture_baselines_and_old_sources() {
        let b = PadShortcut {
            input: "south".into(),
            target: "aim".into(),
        };
        let mut s = Status {
            pad_shortcut: Some(b.clone()),
            ..Default::default()
        };
        let mut cursor = PadShortcutCursor::default();
        assert_eq!(cursor.update(&s, Some(&b), 0, true), 0);
        s.pad_shortcut_serial = 2;
        assert_eq!(cursor.update(&s, Some(&b), 0, true), 2);
        assert_eq!(cursor.update(&s, Some(&b), 0, true), 0);
        s.pad_shortcut_serial = 3;
        s.pad_shortcut_baseline = 3;
        assert_eq!(cursor.update(&s, Some(&b), 0, true), 0);
        s.pad_shortcut_serial = 4;
        assert_eq!(cursor.update(&s, Some(&b), 0, false), 0);
        assert_eq!(cursor.update(&s, Some(&b), 0, true), 0);
        s.pad_shortcut_serial = 5;
        s.pad_shortcut = None;
        assert_eq!(cursor.update(&s, Some(&b), 0, true), 0);
        s.pad_shortcut = Some(b.clone());
        s.pad_shortcut_serial = 0;
        s.pad_shortcut_baseline = 0;
        assert_eq!(cursor.update(&s, Some(&b), 1, true), 0);
        s.pad_shortcut_serial = 1;
        assert_eq!(cursor.update(&s, Some(&b), 1, true), 1);
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
