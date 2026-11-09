//! Bounded low-rate UI intents derived from SDL controls, never OS input.
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NavCommand {
    Up,
    Down,
    Left,
    Right,
    Confirm,
    Back,
    Edit,
    Save,
    Sidebar,
    Menu,
    PreviousZone,
    NextZone,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NavEvent {
    pub serial: u64,
    pub command: NavCommand,
}
#[derive(Default)]
pub struct Navigation {
    pub serial: u64,
    pub events: Vec<NavEvent>,
    pub held: Vec<NavCommand>,
}
impl Navigation {
    pub fn reset(&mut self) {
        self.held.clear();
        self.events.clear();
    }
    pub fn update(&mut self, buttons: &[bool], axes: &[f32; 6], snapshot: bool) {
        use NavCommand::*;
        let mut held = vec![];
        for (command, index, axis, sign) in [
            (Confirm, 0, None, 0.),
            (Back, 1, None, 0.),
            (Edit, 2, None, 0.),
            (Save, 3, None, 0.),
            (Sidebar, 4, None, 0.),
            (Menu, 6, None, 0.),
            (PreviousZone, 9, None, 0.),
            (NextZone, 10, None, 0.),
            (Up, 11, Some(1), -1.),
            (Down, 12, Some(1), 1.),
            (Left, 13, Some(0), -1.),
            (Right, 14, Some(0), 1.),
        ] {
            let threshold = if self.held.contains(&command) {
                0.35
            } else {
                0.65
            };
            let down = buttons.get(index).copied().unwrap_or(false)
                || axis.is_some_and(|i| axes[i].is_finite() && axes[i] * sign > threshold);
            if down {
                held.push(command);
                if !snapshot && !self.held.contains(&command) {
                    self.serial += 1;
                    self.events.push(NavEvent {
                        serial: self.serial,
                        command,
                    });
                }
            }
        }
        if self.events.len() > 48 {
            self.events.drain(..self.events.len() - 48);
        }
        self.held = held;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fast_presses_survive_release_snapshots_do_not_replay_and_the_queue_is_bounded() {
        let mut nav = Navigation::default();
        let mut buttons = [false; 26];
        let axes = [0.; 6];
        buttons[0] = true;
        nav.update(&buttons, &axes, true);
        assert!(nav.events.is_empty());
        buttons[0] = false;
        nav.update(&buttons, &axes, false);
        for _ in 0..60 {
            buttons[0] = true;
            nav.update(&buttons, &axes, false);
            buttons[0] = false;
            nav.update(&buttons, &axes, false);
        }
        assert_eq!(nav.serial, 60);
        assert_eq!(nav.events.len(), 48);
        assert!(nav.held.is_empty());
        assert_eq!(nav.events.last().unwrap().command, NavCommand::Confirm);
        nav.reset();
        assert_eq!(nav.serial, 60);
        assert!(nav.events.is_empty());
    }
    #[test]
    fn stick_navigation_has_a_release_band_and_gyro_values_cannot_repeat_it() {
        let mut nav = Navigation::default();
        let mut axes = [0.; 6];
        axes[0] = 0.7;
        nav.update(&[], &axes, false);
        assert_eq!(nav.serial, 1);
        axes[0] = 0.5;
        nav.update(&[], &axes, false);
        assert_eq!(nav.serial, 1);
        assert_eq!(nav.held, vec![NavCommand::Right]);
        axes[0] = 0.3;
        nav.update(&[], &axes, false);
        assert!(nav.held.is_empty());
        axes[0] = -0.7;
        nav.update(&[], &axes, false);
        assert_eq!(nav.serial, 2);
        assert_eq!(nav.held, vec![NavCommand::Left]);
    }
}
