//! Timing from FlexInput c69f53ad, crates/ui/src/app/threads.rs::start_io_thread (MIT).
//! The headless entry supplies the clock readings and runs the due input/output work.

use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
pub struct IoPass {
    pub started: Instant,
    pub input_due: bool,
    pub output_due: bool,
    pub display_tick: Option<Instant>,
    pub wake: Instant,
    pub output_interval: Duration,
    poll_interval: Duration,
    lead: Duration,
}

pub struct IoSchedule {
    next_poll: Instant,
    next_output: Instant,
    last_flush: Instant,
    sync_work: Duration,
}

impl IoSchedule {
    pub fn new(now: Instant) -> Self {
        Self {
            next_poll: now,
            next_output: now,
            last_flush: now,
            sync_work: Duration::from_micros(800),
        }
    }

    pub fn display_after(&self, out_interval: Duration) -> Instant {
        self.last_flush + out_interval / 2
    }

    pub fn begin(
        &mut self,
        t0: Instant,
        poll_interval: Duration,
        out_interval: Duration,
        tick: Option<Instant>,
        timer_margin: Duration,
    ) -> IoPass {
        let interval = poll_interval.min(out_interval);
        let slack = interval / 4;
        let lead = match tick {
            Some(t) => {
                self.next_output = t;
                (self.sync_work.mul_f32(1.5) + timer_margin).min(out_interval.mul_f32(0.8))
            }
            None => Duration::ZERO,
        };
        IoPass {
            started: t0,
            input_due: t0 + slack >= self.next_poll,
            output_due: t0 + slack + lead >= self.next_output,
            display_tick: tick,
            wake: self.next_poll.min(
                self.next_output
                    .checked_sub(lead)
                    .unwrap_or(self.next_output),
            ),
            output_interval: out_interval,
            poll_interval,
            lead,
        }
    }

    pub fn prepared_output(&mut self, pass: IoPass, now: Instant) {
        if pass.display_tick.is_some() {
            let work = now
                .saturating_duration_since(pass.started)
                .min(Duration::from_millis(2));
            self.sync_work = self.sync_work.mul_f32(0.875) + work.mul_f32(0.125);
        }
    }

    pub fn flushed(&mut self, now: Instant) {
        self.last_flush = now;
    }

    pub fn finish(&mut self, pass: IoPass, now: Instant, next_tick: Option<Instant>) -> Instant {
        if pass.input_due {
            advance(&mut self.next_poll, pass.poll_interval, now);
        }
        if pass.output_due {
            advance(&mut self.next_output, pass.output_interval, now);
        }
        if let Some(t) = next_tick {
            self.next_output = t;
            self.next_poll
                .min(t.checked_sub(pass.lead).unwrap_or(t))
                .min(now + Duration::from_millis(1))
        } else {
            self.next_poll.min(self.next_output)
        }
    }
}

fn advance(deadline: &mut Instant, interval: Duration, now: Instant) {
    *deadline += interval;
    if now.saturating_duration_since(*deadline) > interval {
        *deadline = now;
    }
}

/// The same process-local Windows timer and thread setup as the upstream I/O loop.
pub fn prepare_io_thread() {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::Threading::{
            GetCurrentProcess, GetCurrentThread, ProcessPowerThrottling, SetProcessInformation,
            SetThreadPriority, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION, PROCESS_POWER_THROTTLING_STATE,
            THREAD_PRIORITY_TIME_CRITICAL,
        };
        let state = PROCESS_POWER_THROTTLING_STATE {
            Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            ControlMask: PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
            StateMask: 0,
        };
        SetProcessInformation(
            GetCurrentProcess(),
            ProcessPowerThrottling,
            (&state as *const PROCESS_POWER_THROTTLING_STATE).cast(),
            core::mem::size_of_val(&state) as u32,
        );
        SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_and_output_advance_on_independent_absolute_deadlines() {
        let start = Instant::now();
        let mut schedule = IoSchedule::new(start);
        let poll = Duration::from_millis(2);
        let output = Duration::from_millis(1);
        let pass = schedule.begin(start, poll, output, None, Duration::ZERO);
        assert!(pass.input_due && pass.output_due);
        let late = start + Duration::from_micros(100);
        assert_eq!(schedule.finish(pass, late, None), start + output);
        let pass = schedule.begin(start + output, poll, output, None, Duration::ZERO);
        assert!(!pass.input_due && pass.output_due);
        assert_eq!(schedule.finish(pass, start + output, None), start + poll);
    }

    #[test]
    fn a_late_display_tick_is_flushed_once_before_selecting_the_next_tick() {
        let start = Instant::now();
        let mut schedule = IoSchedule::new(start);
        let poll = Duration::from_millis(2);
        let output = Duration::from_nanos(1_000_000_000 / 960);
        let tick = start + output;
        let late = tick + Duration::from_micros(800);
        let pass = schedule.begin(late, poll, output, Some(tick), Duration::ZERO);
        assert!(pass.output_due);
        assert_eq!(pass.display_tick, Some(tick));
        schedule.prepared_output(pass, late);
        schedule.flushed(late);
        assert_eq!(schedule.display_after(output), late + output / 2);
        let future = start + output * 3;
        schedule.finish(pass, late, Some(future));
        assert_eq!(schedule.next_output, future);
    }

    #[test]
    fn absolute_deadlines_resync_after_a_stall_without_replaying_old_passes() {
        let start = Instant::now();
        let mut schedule = IoSchedule::new(start);
        let period = Duration::from_millis(2);
        let late = start + Duration::from_millis(80);
        let pass = schedule.begin(late, period, period, None, Duration::ZERO);
        assert_eq!(schedule.finish(pass, late, None), late);
        let fresh = schedule.begin(late, period, period, None, Duration::ZERO);
        assert_eq!(schedule.finish(fresh, late, None), late + period);
    }
}
