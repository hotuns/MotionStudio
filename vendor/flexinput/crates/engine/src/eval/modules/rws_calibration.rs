// Extracted unchanged from upstream FlexInput cal.rs; independent RWS math only.
use glam::Vec2;

/// How far out a stick must be to start being followed. Short of the edge the
/// angle is mostly noise, and a thumb dropping back through the middle would
/// swing it half a turn in one tick.
pub const FLICK_EDGE: f32 = 0.7;

/// Once followed, the stick keeps being read down to here — a thumb circling
/// the rim drifts inwards on the way round, and letting go of the count at the
/// first wobble would lose part of the turn.
const FLICK_RELEASE: f32 = 0.35;

/// How long the stick must stay below `FLICK_RELEASE` to let go of it. A
/// Bluetooth gap reads as a stick at rest for a moment; that must not restart
/// the count from wherever the stick comes back.
const FLICK_RELEASE_HOLD_S: f32 = 0.06;

/// The swept angle is paid out over this long rather than in the tick it was
/// read. A stick polls slower than the engine ticks, so the raw per-tick change
/// comes as 0, 0, spike, 0… — and the mouse is handed only the latest tick's
/// displacement, so most spikes would simply never reach the game. Spread out,
/// it is a steady stream like the gyro's. The same window RWS Aim's flick uses.
const FLICK_SMOOTH_S: f32 = 0.025;

/// Follows a stick circling its edge, turning its angle into a rate.
#[derive(Default)]
pub struct Flick {
    /// The stick being followed (0 left, 1 right) and its last angle in degrees,
    /// clockwise from up. `None` while neither stick is being followed.
    prev: Option<(usize, f32)>,
    /// Swept degrees not yet paid out.
    pending: f32,
    /// How long the followed stick has been below the release floor.
    below_s: f32,
}

impl Flick {
    /// The clockwise rate to turn the camera at this tick, in degrees per second,
    /// from whichever stick is circling its edge. Pushing out only picks the
    /// starting angle; it is the sweep round that turns the camera, so unlike
    /// JSM's flick there is no jump to the pushed direction to spoil the count.
    /// Every degree swept is paid out exactly once, so the total is conserved.
    pub fn rate(&mut self, sticks: [Vec2; 2], dt: f32) -> f32 {
        match self.prev {
            Some((side, prev)) => {
                let v = sticks[side];
                if v.length() >= FLICK_RELEASE {
                    self.below_s = 0.0;
                    let angle = v.x.atan2(v.y).to_degrees();
                    // The short way round, so crossing ±180 isn't a whole turn back.
                    self.pending += (angle - prev + 540.0).rem_euclid(360.0) - 180.0;
                    self.prev = Some((side, angle));
                } else {
                    // Hold the last angle through a dropout; let go only once the
                    // stick has plainly been released.
                    self.below_s += dt;
                    if self.below_s >= FLICK_RELEASE_HOLD_S {
                        self.prev = None;
                    }
                }
            }
            None => {
                let side = if sticks[1].length() >= sticks[0].length() { 1 } else { 0 };
                let v = sticks[side];
                if v.length() >= FLICK_EDGE {
                    self.prev = Some((side, v.x.atan2(v.y).to_degrees()));
                    self.below_s = 0.0;
                }
            }
        }
        if dt <= 0.0 || self.pending == 0.0 {
            return 0.0;
        }
        let emit = if FLICK_SMOOTH_S <= dt {
            self.pending
        } else {
            self.pending * (dt / FLICK_SMOOTH_S)
        };
        self.pending -= emit;
        emit / dt
    }
}

