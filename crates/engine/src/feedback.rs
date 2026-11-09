//! Session policy around original virtual feedback and selected-device output.
use anyhow::{ensure, Result};
use flexinput_core::Signal;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct FeedbackConfig {
    pub rumble_enabled: bool,
    pub rumble_gain: f32,
    pub lightbar_enabled: bool,
}
impl Default for FeedbackConfig {
    fn default() -> Self {
        Self {
            rumble_enabled: true,
            rumble_gain: 1.,
            lightbar_enabled: true,
        }
    }
}
impl FeedbackConfig {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.rumble_gain.is_finite() && (0.0..=2.).contains(&self.rumble_gain),
            "震动强度必须在 0–2 之间"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
pub struct FeedbackFrame {
    pub sequence: u64,
    pub rumble: [f32; 2],
    pub lightbar: Option<[f32; 3]>,
}
impl FeedbackFrame {
    pub fn read(device: &mut dyn flexinput_virtual::VirtualDevice) -> Self {
        let pins = device.poll_outputs();
        let get = |pin| {
            pins.iter()
                .find(|p| p.0 == pin)
                .map(|p| p.1.as_float())
                .filter(|v| v.is_finite())
                .map(|v| v.clamp(0., 1.))
        };
        Self {
            sequence: device.feedback_sequence(),
            rumble: [
                get("rumble_strong").unwrap_or(0.),
                get("rumble_weak").unwrap_or(0.),
            ],
            lightbar: get("lightbar_r")
                .zip(get("lightbar_g"))
                .zip(get("lightbar_b"))
                .map(|((r, g), b)| [r, g, b]),
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FeedbackStatus {
    pub active: bool,
    pub awaiting_new_command: bool,
    pub rumble: [f32; 2],
    pub lightbar: Option<[f32; 3]>,
    pub forwarded: u64,
}
#[derive(Default, Debug)]
pub struct FeedbackPlan {
    pub rumble: Option<[f32; 2]>,
    pub lightbar: Option<[f32; 3]>,
    pub clear_lightbar: bool,
}
impl FeedbackPlan {
    pub fn pins(&self) -> Vec<(&'static str, Signal)> {
        let mut pins = vec![];
        if let Some(values) = self.rumble {
            for (pin, value) in ["rumble_strong", "rumble_weak"].into_iter().zip(values) {
                pins.push((pin, Signal::Float(value)));
            }
        }
        if let Some(values) = self.lightbar {
            for (pin, value) in ["lightbar_r", "lightbar_g", "lightbar_b"]
                .into_iter()
                .zip(values)
            {
                pins.push((pin, Signal::Float(value)));
            }
        }
        pins
    }
}
pub struct FeedbackRouter {
    sequence: u64,
    blocked: bool,
    last_send: u64,
    last_config: FeedbackConfig,
    pub status: FeedbackStatus,
}
impl Default for FeedbackRouter {
    fn default() -> Self {
        Self {
            sequence: 0,
            blocked: true,
            last_send: 0,
            last_config: FeedbackConfig::default(),
            status: FeedbackStatus::default(),
        }
    }
}
impl FeedbackRouter {
    pub fn suspend(&mut self, sequence: u64, cfg: &FeedbackConfig) {
        self.sequence = sequence;
        self.blocked = true;
        self.status.active = false;
        self.status.awaiting_new_command = true;
        self.status.rumble = [0.; 2];
        self.status.lightbar = None;
        self.last_send = 0;
        self.last_config = cfg.clone();
    }
    pub fn tick(
        &mut self,
        frame: &FeedbackFrame,
        cfg: &FeedbackConfig,
        active: bool,
        now_ms: u64,
    ) -> FeedbackPlan {
        let mut plan = FeedbackPlan::default();
        let reenabled = (!self.last_config.rumble_enabled && cfg.rumble_enabled)
            || (!self.last_config.lightbar_enabled && cfg.lightbar_enabled);
        if !active || reenabled {
            self.sequence = frame.sequence;
            self.blocked = true;
        } else if frame.sequence != self.sequence {
            self.sequence = frame.sequence;
            self.blocked = false;
        }
        let open = active && !self.blocked;
        let rumble = if open && cfg.rumble_enabled {
            frame.rumble.map(|v| {
                if v.is_finite() {
                    (v * cfg.rumble_gain).clamp(0., 1.)
                } else {
                    0.
                }
            })
        } else {
            [0.; 2]
        };
        if rumble != self.status.rumble
            || (rumble != [0.; 2] && now_ms.saturating_sub(self.last_send) >= 250)
        {
            plan.rumble = Some(rumble);
            self.last_send = now_ms;
        }
        let colour = if open && cfg.lightbar_enabled {
            frame.lightbar
        } else {
            None
        };
        if colour != self.status.lightbar {
            plan.lightbar = colour;
        }
        plan.clear_lightbar = !active || !cfg.lightbar_enabled;
        if plan.rumble.is_some() || plan.lightbar.is_some() {
            self.status.forwarded = self.status.forwarded.saturating_add(1);
        }
        self.status.active = active && (cfg.rumble_enabled || cfg.lightbar_enabled);
        self.status.awaiting_new_command = self.blocked;
        self.status.rumble = rumble;
        self.status.lightbar = colour;
        self.last_config = cfg.clone();
        plan
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_commands_at_the_same_level_resume_after_capture_but_old_levels_do_not() {
        let mut router = FeedbackRouter::default();
        let cfg = FeedbackConfig::default();
        let mut frame = FeedbackFrame {
            sequence: 1,
            rumble: [0.5, 1.],
            lightbar: Some([1., 0., 0.]),
        };
        assert_eq!(router.tick(&frame, &cfg, true, 10).rumble, Some([0.5, 1.]));
        router.suspend(1, &cfg);
        assert_eq!(router.tick(&frame, &cfg, true, 20).rumble, None);
        frame.sequence = 2;
        assert_eq!(router.tick(&frame, &cfg, true, 30).rumble, Some([0.5, 1.]));
    }
    #[test]
    fn disable_releases_motors_and_pending_led_without_replaying_on_enable() {
        let mut r = FeedbackRouter::default();
        let mut c = FeedbackConfig::default();
        let f = FeedbackFrame {
            sequence: 7,
            rumble: [1., 0.5],
            lightbar: Some([0., 1., 0.]),
        };
        r.tick(&f, &c, true, 0);
        c.rumble_enabled = false;
        c.lightbar_enabled = false;
        let plan = r.tick(&f, &c, true, 10);
        assert_eq!(plan.rumble, Some([0.; 2]));
        assert!(plan.clear_lightbar);
        c.rumble_enabled = true;
        assert_eq!(r.tick(&f, &c, true, 20).rumble, None);
        assert!(r.status.awaiting_new_command);
    }
    #[test]
    fn sustained_rumble_refresh_is_bounded_and_idle_polling_never_writes_motors() {
        let mut r = FeedbackRouter::default();
        let c = FeedbackConfig::default();
        let mut f = FeedbackFrame {
            sequence: 1,
            rumble: [1., 1.],
            lightbar: None,
        };
        assert!(r.tick(&f, &c, true, 0).rumble.is_some());
        for ms in 1..250 {
            assert!(r.tick(&f, &c, true, ms).rumble.is_none());
        }
        assert!(r.tick(&f, &c, true, 250).rumble.is_some());
        f.sequence += 1;
        f.rumble = [0.; 2];
        assert_eq!(r.tick(&f, &c, true, 251).rumble, Some([0.; 2]));
        for ms in 252..1000 {
            assert!(r.tick(&f, &c, true, ms).rumble.is_none());
        }
    }
    #[test]
    fn inactive_commands_form_the_baseline_and_gain_cannot_exceed_native_range() {
        let mut r = FeedbackRouter::default();
        let c = FeedbackConfig {
            rumble_gain: 2.,
            ..Default::default()
        };
        let mut f = FeedbackFrame {
            sequence: 2,
            rumble: [0.75, 0.2],
            lightbar: None,
        };
        assert!(r.tick(&f, &c, false, 0).rumble.is_none());
        assert!(r.tick(&f, &c, true, 10).rumble.is_none());
        f.sequence = 3;
        assert_eq!(r.tick(&f, &c, true, 20).rumble, Some([1., 0.4]));
    }
}
