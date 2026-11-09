use crate::storage::SwitchTarget;
use std::time::{Duration, Instant};

/// Cursor observations are debounced. Startup establishes a baseline; changing
/// profiles cannot itself fabricate a cursor edge.
#[derive(Default)]
pub struct CursorEdges {
    stable: Option<bool>,
    candidate: Option<(bool, Instant)>,
}
impl CursorEdges {
    pub fn update(&mut self, visible: bool, now: Instant) -> Option<bool> {
        self.update_with_delay(visible, now, Duration::from_millis(250))
    }
    pub fn update_with_delay(
        &mut self,
        visible: bool,
        now: Instant,
        delay: Duration,
    ) -> Option<bool> {
        if self.stable == Some(visible) {
            self.candidate = None;
            return None;
        }
        let since = match self.candidate {
            Some((value, since)) if value == visible => since,
            _ => {
                self.candidate = Some((visible, now));
                now
            }
        };
        if now.saturating_duration_since(since) < delay {
            return None;
        }
        let had_baseline = self.stable.replace(visible).is_some();
        self.candidate = None;
        had_baseline.then_some(visible)
    }
}
#[derive(Clone, Default)]
pub struct SwitchHistory {
    previous: Option<String>,
}
impl SwitchHistory {
    /// Call on a clone; commit only after the engine accepts the target profile.
    pub fn apply(&mut self, current: &str, target: &SwitchTarget) -> Option<String> {
        match target {
            SwitchTarget::None => None,
            SwitchTarget::Restore => self.previous.take().filter(|p| p != current),
            SwitchTarget::Profile(id) if id != current => {
                if self.previous.is_none() {
                    self.previous = Some(current.into());
                }
                Some(id.clone())
            }
            _ => None,
        }
    }
    pub fn toggle(&mut self, current: &str, target: &str) -> Option<String> {
        if current == target {
            self.apply(current, &SwitchTarget::Restore)
        } else {
            self.apply(current, &SwitchTarget::Profile(target.into()))
        }
    }
    pub fn clear(&mut self) {
        self.previous = None;
    }
    /// Multiple raw presses may arrive in one status frame. Resolve their final
    /// logical destination without opening intermediate output devices.
    pub fn toggle_presses(&mut self, current: &str, target: &str, presses: u64) -> Option<String> {
        if presses == 0 {
            return None;
        }
        let mut destination = current.to_string();
        for _ in 0..(1 + (presses - 1) % 2) {
            if let Some(next) = self.toggle(&destination, target) {
                destination = next;
            }
        }
        Some(destination)
    }
}
