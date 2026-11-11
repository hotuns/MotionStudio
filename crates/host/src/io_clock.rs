#[cfg(windows)]
use motion_core::timing::display_clock;
use motion_core::{
    protocol::DisplaySyncStatus,
    timing::{
        io_schedule::{IoPass, IoSchedule},
        HrWaiter,
    },
};
use std::time::{Duration, Instant};

const POLL_HZ: u32 = 500;
const POLL_INTERVAL: Duration = Duration::from_nanos(1_000_000_000 / POLL_HZ as u64);

#[cfg(windows)]
const SYNC: display_clock::OutputSync = display_clock::OutputSync {
    per_frame: 4,
    phase: 0.125,
    follow_vrr: true,
};

pub(crate) struct IoClock {
    schedule: IoSchedule,
    waiter: HrWaiter,
    #[cfg(windows)]
    enabled: bool,
    #[cfg(windows)]
    precise: display_clock::PreciseWait,
}

impl IoClock {
    pub fn new(now: Instant) -> Self {
        Self {
            schedule: IoSchedule::new(now),
            waiter: HrWaiter::new(),
            #[cfg(windows)]
            enabled: false,
            #[cfg(windows)]
            precise: display_clock::PreciseWait::default(),
        }
    }

    pub fn reset(&mut self, now: Instant) {
        self.schedule = IoSchedule::new(now);
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        #[cfg(windows)]
        if self.enabled != enabled {
            self.enabled = enabled;
            display_clock::set_sync(enabled.then_some(SYNC));
        }
        #[cfg(not(windows))]
        let _ = enabled;
    }

    fn output_hz(&self) -> u32 {
        #[cfg(windows)]
        if self.enabled {
            return display_clock::output_hz(SYNC.per_frame).clamp(30, 4000);
        }
        POLL_HZ
    }

    fn display_tick(&self, interval: Duration) -> Option<Instant> {
        #[cfg(windows)]
        if self.enabled {
            return display_clock::next_tick(self.schedule.display_after(interval), SYNC);
        }
        #[cfg(not(windows))]
        let _ = interval;
        None
    }

    pub fn begin(&mut self, now: Instant) -> IoPass {
        let interval = Duration::from_nanos(1_000_000_000 / u64::from(self.output_hz()));
        let tick = self.display_tick(interval);
        #[cfg(windows)]
        let margin = self.precise.margin();
        #[cfg(not(windows))]
        let margin = Duration::ZERO;
        self.schedule
            .begin(now, POLL_INTERVAL, interval, tick, margin)
    }

    pub fn wait_until(&mut self, deadline: Instant) {
        self.waiter
            .wait(deadline.saturating_duration_since(Instant::now()));
    }

    pub fn begin_flush(&mut self, pass: IoPass) {
        self.schedule.prepared_output(pass, Instant::now());
        #[cfg(windows)]
        if let Some(deadline) = pass.display_tick {
            self.precise
                .until(deadline, |duration| self.waiter.wait(duration));
        }
        self.schedule.flushed(Instant::now());
    }

    pub fn finish(&mut self, pass: IoPass) {
        let tick = self.display_tick(pass.output_interval);
        let wake = self.schedule.finish(pass, Instant::now(), tick);
        self.wait_until(wake);
    }

    pub fn status(&self) -> Option<DisplaySyncStatus> {
        #[cfg(windows)]
        if self.enabled {
            let status = display_clock::status()?;
            return Some(DisplaySyncStatus {
                live: status.live,
                frame_hz: status.frame_hz.round() as u32,
                reports_per_frame: SYNC.per_frame,
                vrr: status.vrr,
                fallback_hz: self.output_hz(),
            });
        }
        None
    }
}
