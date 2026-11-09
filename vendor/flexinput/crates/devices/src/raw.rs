//! Timestamped event seam added to the pinned provider before poll aggregation.
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SampleTime {
    pub host_ns: u64,
    pub sensor_ns: u64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RawEvent {
    Button {
        time: SampleTime,
        index: u8,
        down: bool,
    },
    Axis {
        time: SampleTime,
        index: u8,
        value: i16,
    },
    Touch {time:SampleTime,pad:u8,finger:u8,active:bool,x:f32,y:f32},
    Gyro {
        time: SampleTime,
        value: [f32; 3],
    },
    Accel {
        time: SampleTime,
        value: [f32; 3],
    },
    Disconnected {
        time: SampleTime,
    },
}

#[derive(Debug, Clone)]
pub struct RawDevice {
    pub id: u32,
    pub name: String,
    pub path: String,
    pub vendor: u16,
    pub product: u16,
    pub gamepad: bool,
    pub gyro: bool,
    pub accelerometer: bool,
    pub touchpads:u16,
    pub rumble:bool,
    pub lightbar:bool,
    pub buttons: Vec<u8>,
    pub virtual_device: bool,
    pub open_error: Option<String>,
}

const CAPACITY: usize = 4096;
#[derive(Default)]
struct Capture {
    selected: Option<u32>,
    events: VecDeque<RawEvent>,
    overflowed: bool,
}
impl Capture {
    fn push(&mut self, id: u32, event: RawEvent) {
        if self.selected != Some(id) || self.overflowed {
            return;
        }
        if self.events.len() == CAPACITY {
            self.events.clear();
            self.overflowed = true;
        } else {
            self.events.push_back(event);
        }
    }
    fn take(&mut self) -> Result<Option<RawEvent>, String> {
        if self.overflowed {
            return Err("FlexInput raw input queue overflow; stop and release output".into());
        }
        Ok(self.events.pop_front())
    }
}
static CAPTURE: Mutex<Option<Capture>> = Mutex::new(None);
static CLAIMED: AtomicBool = AtomicBool::new(false);
pub(crate) fn claim() -> bool {
    CLAIMED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
}
pub(crate) fn release() {
    *CAPTURE.lock().unwrap_or_else(|e| e.into_inner()) = None;
    CLAIMED.store(false, Ordering::Release);
}

pub(crate) fn select(id: Option<u32>) {
    *CAPTURE.lock().unwrap_or_else(|e| e.into_inner()) = Some(Capture {
        selected: id,
        ..Capture::default()
    });
}
pub(crate) fn take() -> Result<Option<RawEvent>, String> {
    let mut guard = CAPTURE.lock().unwrap_or_else(|e| e.into_inner());
    guard.get_or_insert_with(Capture::default).take()
}

/// Runs inside the upstream SDL filter. Preserve SI values, FIFO order and
/// both clocks; do not pair, average or deduplicate sensor events here.
pub(crate) unsafe fn capture(event: &sdl3::sys::events::SDL_Event) -> bool {
    use sdl3::sys::events::*;
    use sdl3::sys::sensor::{SDL_SENSOR_ACCEL, SDL_SENSOR_GYRO};
    let mut guard = CAPTURE.lock().unwrap_or_else(|e| e.into_inner());
    let Some(capture) = guard.as_mut() else {
        return false;
    };
    let ty = unsafe { event.r#type };
    let time = |host_ns, sensor_ns| SampleTime { host_ns, sensor_ns };
    if ty == SDL_EVENT_GAMEPAD_SENSOR_UPDATE.0 {
        let e = unsafe { event.gsensor };
        let t = time(e.timestamp, e.sensor_timestamp);
        let sample = if e.sensor == SDL_SENSOR_GYRO.0 {
            RawEvent::Gyro {
                time: t,
                value: e.data,
            }
        } else if e.sensor == SDL_SENSOR_ACCEL.0 {
            RawEvent::Accel {
                time: t,
                value: e.data,
            }
        } else {
            return true;
        };
        capture.push(e.which.into(), sample);
    } else if ty == SDL_EVENT_GAMEPAD_AXIS_MOTION.0 {
        let e = unsafe { event.gaxis };
        capture.push(
            e.which.into(),
            RawEvent::Axis {
                time: time(e.timestamp, 0),
                index: e.axis,
                value: e.value,
            },
        );
    } else if ty == SDL_EVENT_GAMEPAD_BUTTON_DOWN.0 || ty == SDL_EVENT_GAMEPAD_BUTTON_UP.0 {
        let e = unsafe { event.gbutton };
        capture.push(
            e.which.into(),
            RawEvent::Button {
                time: time(e.timestamp, 0),
                index: e.button,
                down: e.down,
            },
        );
    } else if [SDL_EVENT_GAMEPAD_TOUCHPAD_DOWN.0,SDL_EVENT_GAMEPAD_TOUCHPAD_MOTION.0,SDL_EVENT_GAMEPAD_TOUCHPAD_UP.0].contains(&ty) {
        let e=unsafe{event.gtouchpad};
        if (0..2).contains(&e.touchpad)&&(0..2).contains(&e.finger) {
            capture.push(e.which.into(),RawEvent::Touch{time:time(e.timestamp,0),pad:e.touchpad as u8,finger:e.finger as u8,active:ty!=SDL_EVENT_GAMEPAD_TOUCHPAD_UP.0,x:e.x,y:e.y});
        }
    } else if ty == SDL_EVENT_GAMEPAD_REMOVED.0 {
        let e = unsafe { event.gdevice };
        capture.push(
            e.which.into(),
            RawEvent::Disconnected {
                time: time(e.timestamp, 0),
            },
        );
    } else {
        // SDL's internal gamepad watcher must still see joystick events in
        // order to translate them into mapped gamepad buttons and axes.
        return false;
    }
    // The raw path never uses SDL's event queue or the sampled poll cache.
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identical_samples_and_button_edges_keep_clocks_and_order() {
        let mut c = Capture {
            selected: Some(7),
            ..Default::default()
        };
        let events = [
            RawEvent::Gyro {
                time: SampleTime {
                    host_ns: 100,
                    sensor_ns: 10,
                },
                value: [1., 0., 0.],
            },
            RawEvent::Button {
                time: SampleTime {
                    host_ns: 101,
                    sensor_ns: 0,
                },
                index: 4,
                down: true,
            },
            RawEvent::Gyro {
                time: SampleTime {
                    host_ns: 102,
                    sensor_ns: 12,
                },
                value: [1., 0., 0.],
            },
            RawEvent::Button {
                time: SampleTime {
                    host_ns: 103,
                    sensor_ns: 0,
                },
                index: 4,
                down: false,
            },
        ];
        for e in events {
            c.push(7, e);
        }
        c.push(8, events[0]);
        for expected in events {
            assert_eq!(c.take().unwrap(), Some(expected));
        }
        assert_eq!(c.take().unwrap(), None);
    }
    #[test]
    fn overflow_is_sticky_and_never_replays_a_partial_chord() {
        let mut c = Capture {
            selected: Some(1),
            ..Default::default()
        };
        let e = RawEvent::Button {
            time: SampleTime::default(),
            index: 1,
            down: true,
        };
        for _ in 0..=CAPACITY {
            c.push(1, e);
        }
        assert!(c.events.is_empty());
        assert!(c.take().is_err());
        c.push(1, e);
        assert!(c.take().is_err());
    }
    #[test]
    fn raw_filter_leaves_joystick_events_for_sdls_gamepad_translator() {
        use sdl3::sys::events::*;
        select(Some(1));
        let mut event: SDL_Event = unsafe { std::mem::zeroed() };
        event.r#type = SDL_EVENT_JOYSTICK_BUTTON_DOWN.0;
        assert!(!unsafe { capture(&event) });
        event.r#type = SDL_EVENT_JOYSTICK_AXIS_MOTION.0;
        assert!(!unsafe { capture(&event) });
        event.r#type = SDL_EVENT_JOYSTICK_REMOVED.0;
        assert!(!unsafe { capture(&event) });
        release();
    }
}
