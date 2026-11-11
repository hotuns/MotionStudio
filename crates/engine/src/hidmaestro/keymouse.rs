//! Ordered keyboard/mouse delivery off the SDL sampling thread.
//! Relative motion coalesces only between discrete input transitions. ACK waits
//! and descriptor-sized splitting happen exclusively on this I/O worker.
use crate::{
    output::{MouseTrace, MouseTracePoint, OutputStats},
    timing::{next_output_deadline, CONTINUOUS_OUTPUT_INTERVAL},
};
use anyhow::{bail, Context, Result};
use std::{
    collections::VecDeque,
    sync::{Arc, Condvar, Mutex, MutexGuard},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const MAX_QUEUE: usize = 256;
const MAX_MOTION: i64 = 8_000_000;
const MAX_QUEUE_AGE: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Delivery {
    Stream,
    Confirmed,
    Release,
}

pub(super) trait ReportWriter: Send + 'static {
    fn write(&mut self, report: &[u8], delivery: Delivery) -> Result<()>;
    fn finish(&mut self) -> Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub(super) enum Report {
    Keyboard([u8; 8]),
    Buttons(u8),
    Motion {
        buttons: u8,
        x: i64,
        y: i64,
    },
    Scroll {
        buttons: u8,
        horizontal: i32,
        vertical: i32,
    },
}
struct Pending {
    report: Report,
    queued: Instant,
}
#[derive(Default)]
struct State {
    queue: VecDeque<Pending>,
    generation: u64,
    reset: bool,
    reset_done: u64,
    closing: bool,
    stopped: bool,
    busy: bool,
    error: Option<String>,
    stats: OutputStats,
    mouse_trace: MouseTrace,
    last_mouse: [u8; 7],
}
struct Shared {
    state: Mutex<State>,
    mouse: Mutex<Box<dyn ReportWriter>>,
    wake: Condvar,
}
impl Shared {
    fn new(mouse: impl ReportWriter) -> Self {
        Self {
            state: Mutex::default(),
            mouse: Mutex::new(Box::new(mouse)),
            wake: Condvar::new(),
        }
    }
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn current(&self, generation: u64) -> bool {
        let s = self.lock();
        s.generation == generation && !s.closing && s.error.is_none()
    }
    fn with_mouse<T>(&self, run: impl FnOnce(&mut dyn ReportWriter) -> Result<T>) -> Result<T> {
        let mut mouse = self.mouse.lock().unwrap_or_else(|error| error.into_inner());
        run(mouse.as_mut())
    }
    fn error(s: &State) -> Result<()> {
        if let Some(error) = &s.error {
            bail!("{error}");
        }
        if s.stopped || s.closing {
            bail!("HIDMaestro 键鼠输出线程已停止");
        }
        Ok(())
    }
}
pub(super) struct KeyMouse {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}
impl KeyMouse {
    pub(super) fn start(keyboard: impl ReportWriter, mouse: impl ReportWriter) -> Result<Self> {
        let shared = Arc::new(Shared::new(mouse));
        let worker = shared.clone();
        let thread = thread::Builder::new()
            .name("hid-keymouse-output".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run(&worker, keyboard)
                }));
                let mut s = worker.lock();
                if result.is_err() && s.error.is_none() {
                    s.error = Some("HIDMaestro 键鼠输出线程异常，已暂停映射".into());
                }
                s.stopped = true;
                s.busy = false;
                s.queue.clear();
                worker.wake.notify_all();
            })
            .context("启动 HIDMaestro 键鼠发送线程")?;
        Ok(Self {
            shared,
            thread: Some(thread),
        })
    }
    pub(super) fn poll(&self) -> Result<()> {
        Shared::error(&self.shared.lock())
    }
    pub(super) fn submit(&self, report: Report) -> Result<()> {
        let s = self.shared.lock();
        Shared::error(&s)?;
        self.enqueue(s, report)
    }
    pub(super) fn motion(&self, buttons: u8, dx: i32, dy: i32) -> Result<()> {
        let mut s = self.shared.lock();
        Shared::error(&s)?;
        if !s.busy
            && !s.reset
            && s.queue.is_empty()
            && dx.unsigned_abs() <= 32767
            && dy.unsigned_abs() <= 32767
        {
            self.write_at_tick(&mut s, mouse_report(buttons, dx as i16, dy as i16, 0, 0))?;
            s.stats.direct_motion_reports += 1;
            return Ok(());
        }
        self.enqueue(
            s,
            Report::Motion {
                buttons,
                x: i64::from(dx),
                y: i64::from(dy),
            },
        )
    }
    pub(super) fn idle_motion(&self, buttons: u8) -> Result<()> {
        let mut s = self.shared.lock();
        Shared::error(&s)?;
        if !s.busy && !s.reset && s.queue.is_empty() && s.last_mouse[1..7] != [0; 6] {
            self.write_at_tick(&mut s, mouse_report(buttons, 0, 0, 0, 0))?;
        }
        Ok(())
    }
    fn write_at_tick(&self, s: &mut State, report: [u8; 7]) -> Result<()> {
        // An idle worker owns no pending edge or ACK. Hold the state lock
        // through the nonblocking raw write so it cannot start one mid-flush.
        let start = Instant::now();
        let result = self
            .shared
            .with_mouse(|writer| writer.write(&report, Delivery::Stream));
        record_write(s, &report, true, start, Instant::now(), result.is_ok());
        if let Err(error) = &result {
            s.error = Some(format!("HIDMaestro 键鼠输出失败：{error:#}"));
            s.generation += 1;
            s.reset = true;
            self.shared.wake.notify_one();
        }
        result
    }
    fn enqueue(&self, mut s: MutexGuard<'_, State>, report: Report) -> Result<()> {
        if let Report::Motion { buttons, x, y } = &report {
            if x.unsigned_abs() > MAX_MOTION as u64 || y.unsigned_abs() > MAX_MOTION as u64 {
                bail!("HIDMaestro 鼠标位移过大");
            }
            if let Some(Pending {
                report:
                    Report::Motion {
                        buttons: old_buttons,
                        x: old_x,
                        y: old_y,
                    },
                ..
            }) = s.queue.back_mut()
            {
                if buttons == old_buttons {
                    let next_x = old_x.checked_add(*x).context("HIDMaestro 鼠标位移溢出")?;
                    let next_y = old_y.checked_add(*y).context("HIDMaestro 鼠标位移溢出")?;
                    if next_x.unsigned_abs() > MAX_MOTION as u64
                        || next_y.unsigned_abs() > MAX_MOTION as u64
                    {
                        bail!("HIDMaestro 鼠标输出积压，已暂停映射");
                    }
                    *old_x = next_x;
                    *old_y = next_y;
                    s.stats.coalesced_motion += 1;
                    return Ok(());
                }
            }
        }
        if s.queue.len() >= MAX_QUEUE {
            bail!("HIDMaestro 输出队列已满，已暂停映射");
        }
        s.queue.push_back(Pending {
            report,
            queued: Instant::now(),
        });
        self.shared.wake.notify_one();
        Ok(())
    }
    pub(super) fn stats(&self) -> OutputStats {
        let s = self.shared.lock();
        let mut stats = s.stats.clone();
        stats.queued = s.queue.len() + usize::from(s.busy);
        stats
    }
    pub(super) fn mouse_trace(&self) -> Vec<MouseTracePoint> {
        self.shared.lock().mouse_trace.points()
    }
    pub(super) fn reset(&self) -> Result<()> {
        let mut s = self.shared.lock();
        if s.stopped {
            bail!("HIDMaestro 键鼠输出线程已停止");
        }
        s.generation += 1;
        let generation = s.generation;
        s.queue.clear();
        s.reset = true;
        self.shared.wake.notify_one();
        let deadline = Instant::now() + Duration::from_millis(250);
        while s.reset_done < generation && !s.stopped {
            let left = deadline
                .checked_duration_since(Instant::now())
                .context("HIDMaestro 键鼠释放超时")?;
            s = self
                .shared
                .wake
                .wait_timeout(s, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        Shared::error(&s)
    }
    pub(super) fn stop(&mut self) -> Result<()> {
        if let Some(thread) = self.thread.take() {
            {
                let mut s = self.shared.lock();
                s.generation += 1;
                s.queue.clear();
                s.closing = true;
                self.shared.wake.notify_one();
            }
            thread
                .join()
                .map_err(|_| anyhow::anyhow!("HIDMaestro 键鼠输出线程异常退出"))?;
        }
        let s = self.shared.lock();
        if let Some(error) = &s.error {
            bail!("{error}");
        }
        Ok(())
    }
    #[cfg(test)]
    fn drain(&self) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut s = self.shared.lock();
        while s.busy || !s.queue.is_empty() {
            Shared::error(&s)?;
            let left = deadline
                .checked_duration_since(Instant::now())
                .context("test output did not drain")?;
            s = self
                .shared
                .wake
                .wait_timeout(s, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        Shared::error(&s)
    }
}
impl Drop for KeyMouse {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn record_write(
    s: &mut State,
    report: &[u8],
    mouse: bool,
    start: Instant,
    completed: Instant,
    succeeded: bool,
) {
    let us = completed
        .duration_since(start)
        .as_micros()
        .min(u128::from(u64::MAX)) as u64;
    s.stats.wait_total_us = s.stats.wait_total_us.saturating_add(us);
    s.stats.wait_max_us = s.stats.wait_max_us.max(us);
    if succeeded {
        if mouse {
            s.stats.mouse_reports += 1;
            s.last_mouse.copy_from_slice(report);
            let dx = i32::from(i16::from_le_bytes([report[1], report[2]]));
            let dy = i32::from(i16::from_le_bytes([report[3], report[4]]));
            s.mouse_trace.record([dx, dy], completed);
        } else {
            s.stats.keyboard_reports += 1;
        }
    }
}
struct Devices<'a, K> {
    shared: &'a Shared,
    keyboard: K,
    timer: flexinput_engine::HrWaiter,
    next_scroll: Instant,
}
impl<K: ReportWriter> Devices<'_, K> {
    fn mouse(&mut self, report: [u8; 7], delivery: Delivery) -> Result<()> {
        let start = Instant::now();
        let result = self
            .shared
            .with_mouse(|writer| writer.write(&report, delivery));
        let completed = Instant::now();
        record_write(
            &mut self.shared.lock(),
            &report,
            true,
            start,
            completed,
            result.is_ok(),
        );
        result
    }
    fn keyboard(&mut self, report: [u8; 8], delivery: Delivery) -> Result<()> {
        let start = Instant::now();
        let result = self.keyboard.write(&report, delivery);
        let completed = Instant::now();
        record_write(
            &mut self.shared.lock(),
            &report,
            false,
            start,
            completed,
            result.is_ok(),
        );
        result
    }
    fn neutral(&mut self) -> Result<()> {
        let keys = self.keyboard([0; 8], Delivery::Release);
        let mouse = self.mouse([0; 7], Delivery::Release);
        self.next_scroll = Instant::now();
        keys.and(mouse)
    }
    fn wait_until(&self, deadline: Instant) {
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            self.timer.wait(left);
        }
    }
    fn pace_scroll(&mut self) {
        self.wait_until(self.next_scroll);
        self.next_scroll = next_output_deadline(self.next_scroll, Instant::now());
    }
    fn deliver(&mut self, generation: u64, report: Report) -> Result<()> {
        if !self.shared.current(generation) {
            return Ok(());
        }
        match report {
            Report::Keyboard(bytes) => {
                // A modifier cannot overtake an older mouse/scroll frame.
                self.shared.with_mouse(|writer| writer.finish())?;
                self.keyboard(bytes, Delivery::Confirmed)
            }
            Report::Buttons(buttons) => {
                self.mouse(mouse_report(buttons, 0, 0, 0, 0), Delivery::Confirmed)
            }
            Report::Motion {
                buttons,
                mut x,
                mut y,
            } => loop {
                if !self.shared.current(generation) {
                    return Ok(());
                }
                let dx = x.clamp(-32767, 32767) as i16;
                let dy = y.clamp(-32767, 32767) as i16;
                self.mouse(mouse_report(buttons, dx, dy, 0, 0), Delivery::Stream)?;
                x -= i64::from(dx);
                y -= i64::from(dy);
                if x == 0 && y == 0 {
                    return Ok(());
                }
                // Only descriptor-sized overflow needs an additional report.
                self.timer.wait(CONTINUOUS_OUTPUT_INTERVAL);
            },
            Report::Scroll {
                buttons,
                mut horizontal,
                mut vertical,
            } => loop {
                self.pace_scroll();
                if !self.shared.current(generation) {
                    return Ok(());
                }
                let h = horizontal.clamp(-127, 127) as i8;
                let v = vertical.clamp(-127, 127) as i8;
                self.mouse(mouse_report(buttons, 0, 0, h, v), Delivery::Confirmed)?;
                horizontal -= i32::from(h);
                vertical -= i32::from(v);
                if horizontal == 0 && vertical == 0 {
                    return Ok(());
                }
            },
        }
    }
}
fn mouse_report(buttons: u8, x: i16, y: i16, h: i8, v: i8) -> [u8; 7] {
    let mut r = [0; 7];
    r[0] = buttons;
    r[1..3].copy_from_slice(&x.to_le_bytes());
    r[3..5].copy_from_slice(&y.to_le_bytes());
    r[5] = v as u8;
    r[6] = h as u8;
    r
}
fn run(shared: &Shared, keyboard: impl ReportWriter) {
    // FlexInput raises only its bounded I/O loop; the UI remains at normal
    // priority. HrWaiter yields on an absolute grid rather than catch-up bursts.
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::Threading::{
            GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_TIME_CRITICAL,
        };
        SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL);
    }
    let mut devices = Devices {
        shared,
        keyboard,
        timer: flexinput_engine::HrWaiter::new(),
        next_scroll: Instant::now(),
    };
    loop {
        let mut s = shared.lock();
        while !s.closing && !s.reset && s.queue.is_empty() {
            s = shared.wake.wait(s).unwrap_or_else(|e| e.into_inner());
        }
        let generation = s.generation;
        if s.closing || s.reset {
            let closing = s.closing;
            s.reset = false;
            s.busy = true;
            drop(s);
            let result = devices.neutral();
            let mut s = shared.lock();
            s.busy = false;
            s.reset_done = generation;
            s.mouse_trace.clear();
            if let Err(e) = result {
                s.error
                    .get_or_insert_with(|| format!("HIDMaestro 输出释放失败：{e:#}"));
            }
            shared.wake.notify_all();
            if closing {
                break;
            }
            continue;
        }
        let Some(pending) = s.queue.pop_front() else {
            continue;
        };
        s.busy = true;
        let age = pending.queued.elapsed();
        s.stats.queue_max_us = s
            .stats
            .queue_max_us
            .max(age.as_micros().min(u128::from(u64::MAX)) as u64);
        drop(s);
        let result = if age > MAX_QUEUE_AGE {
            Err(anyhow::anyhow!(
                "HIDMaestro 输出积压超过 100 ms，已暂停映射"
            ))
        } else {
            devices.deliver(generation, pending.report)
        };
        if let Err(e) = result {
            {
                let mut s = shared.lock();
                s.error
                    .get_or_insert_with(|| format!("HIDMaestro 键鼠输出失败：{e:#}"));
                s.queue.clear();
            }
            let _ = devices.neutral();
        }
        let mut s = shared.lock();
        s.busy = false;
        shared.wake.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        engine::{Frame, PadState},
        output::{Dispatcher, Sink},
    };
    use std::sync::mpsc::{self, Receiver, Sender};
    type Log = Arc<Mutex<Vec<(bool, Vec<u8>)>>>;
    struct Fake {
        mouse: bool,
        log: Log,
        gate: Option<(Sender<()>, Receiver<()>)>,
        fail_press: bool,
    }
    impl ReportWriter for Fake {
        fn write(&mut self, report: &[u8], _delivery: Delivery) -> Result<()> {
            self.log.lock().unwrap().push((self.mouse, report.to_vec()));
            if self.mouse && report[1..5].iter().any(|b| *b != 0) {
                if let Some((entered, release)) = self.gate.take() {
                    entered.send(())?;
                    release.recv_timeout(Duration::from_secs(2))?;
                }
            }
            if self.fail_press && report.iter().any(|b| *b != 0) {
                self.fail_press = false;
                bail!("injected ACK failure");
            }
            Ok(())
        }
    }
    fn fixture(fail_key: bool) -> (KeyMouse, Log, Receiver<()>, Sender<()>) {
        let log = Arc::new(Mutex::new(vec![]));
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let keyboard = Fake {
            mouse: false,
            log: log.clone(),
            gate: None,
            fail_press: fail_key,
        };
        let mouse = Fake {
            mouse: true,
            log: log.clone(),
            gate: Some((entered_tx, release_rx)),
            fail_press: false,
        };
        (
            KeyMouse::start(keyboard, mouse).unwrap(),
            log,
            entered_rx,
            release_tx,
        )
    }
    fn move_by(io: &KeyMouse, x: i64, y: i64, buttons: u8) {
        io.submit(Report::Motion { buttons, x, y }).unwrap();
    }
    fn unblock(io: &KeyMouse, release: Sender<()>) {
        // This is a deterministic ACK gate, not a wall-clock driver. Test
        // staleness separately so host scheduling cannot change byte accounting.
        for p in &mut io.shared.lock().queue {
            p.queued = Instant::now();
        }
        release.send(()).unwrap();
    }
    fn wait_reset(shared: &Shared) {
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut s = shared.lock();
        while s.generation == 0 {
            let left = deadline
                .checked_duration_since(Instant::now())
                .expect("reset was not requested");
            s = shared.wake.wait_timeout(s, left).unwrap().0;
        }
    }
    fn xy(bytes: &[u8]) -> (i64, i64) {
        (
            i64::from(i16::from_le_bytes([bytes[1], bytes[2]])),
            i64::from(i16::from_le_bytes([bytes[3], bytes[4]])),
        )
    }

    #[test]
    fn a_blocked_writer_does_not_block_1000_input_frames_or_lose_fractional_motion() {
        let (io, log, entered, release) = fixture(false);
        let io = Arc::new(io);
        struct SampleSink(Arc<KeyMouse>);
        impl Sink for SampleSink {
            fn button(&mut self, _: &str, _: bool) -> Result<()> {
                Ok(())
            }
            fn gamepad(&mut self, _: PadState) -> Result<()> {
                Ok(())
            }
            fn mouse(&mut self, x: i32, y: i32) -> Result<()> {
                self.0.submit(Report::Motion {
                    buttons: 0,
                    x: i64::from(x),
                    y: i64::from(y),
                })
            }
            fn poll(&mut self) -> Result<()> {
                self.0.poll()
            }
            fn mouse_trace(&self) -> Option<Vec<MouseTracePoint>> {
                Some(self.0.mouse_trace())
            }
        }
        let mut dispatcher = Dispatcher::new(SampleSink(io.clone()));
        dispatcher
            .apply(&Frame {
                mouse_delta: [1.0, -1.0],
                ..Default::default()
            })
            .unwrap();
        entered.recv_timeout(Duration::from_secs(2)).unwrap();
        // The I/O thread cannot finish its first ACK until all input has run.
        // An inline wait in the sampling path deadlocks/fails this regression.
        for _ in 0..1000 {
            dispatcher
                .apply(&Frame {
                    mouse_delta: [0.25, -0.25],
                    ..Default::default()
                })
                .unwrap();
        }
        assert_eq!(dispatcher.mouse_counts, [251, -251]);
        assert!(dispatcher.preview().mouse.is_empty());
        assert_eq!(io.stats().queued, 2);
        assert_eq!(io.stats().coalesced_motion, 249);
        unblock(&io, release);
        io.drain().unwrap();
        assert_eq!(
            dispatcher.preview().mouse.last().unwrap().position,
            [251, -251]
        );
        let frames = log.lock().unwrap();
        let sum = frames
            .iter()
            .filter(|(m, _)| *m)
            .map(|(_, b)| xy(b))
            .fold((0, 0), |(x, y), (dx, dy)| (x + dx, y + dy));
        assert_eq!(sum, (251, -251));
        assert_eq!(
            frames
                .iter()
                .filter(|(mouse, b)| *mouse && b[1..5] != [0; 4])
                .count(),
            2
        );
    }
    #[test]
    fn motion_coalescing_never_crosses_modifier_wheel_or_side_button_edges() {
        let (io, log, entered, release) = fixture(false);
        move_by(&io, 1, 0, 0);
        entered.recv_timeout(Duration::from_secs(2)).unwrap();
        io.submit(Report::Keyboard([1, 0, 0, 0, 0, 0, 0, 0]))
            .unwrap();
        io.motion(0, 2, 0).unwrap();
        io.motion(0, 3, 0).unwrap();
        io.submit(Report::Scroll {
            buttons: 0,
            horizontal: 1,
            vertical: -1,
        })
        .unwrap();
        io.submit(Report::Buttons(8)).unwrap();
        io.motion(8, 4, 0).unwrap();
        assert_eq!(io.stats().direct_motion_reports, 0);
        io.submit(Report::Buttons(0)).unwrap();
        io.submit(Report::Keyboard([0; 8])).unwrap();
        io.submit(Report::Keyboard([0, 0, 4, 0, 0, 0, 0, 0]))
            .unwrap();
        io.submit(Report::Keyboard([0; 8])).unwrap();
        unblock(&io, release);
        io.drain().unwrap();
        let frames = log.lock().unwrap();
        assert_eq!(frames.len(), 10);
        assert_eq!(frames[1], (false, vec![1, 0, 0, 0, 0, 0, 0, 0]));
        assert_eq!(xy(&frames[2].1), (5, 0));
        assert_eq!(&frames[3].1[5..], &[255, 1]);
        assert_eq!(frames[4].1[0], 8);
        assert_eq!(frames[5].1[0], 8);
        assert_eq!(xy(&frames[5].1), (4, 0));
        assert_eq!(frames[6].1[0], 0);
        assert_eq!(frames[7], (false, vec![0; 8]));
        assert_eq!(frames[8].1[2], 4);
        assert_eq!(frames[9], (false, vec![0; 8]));
    }
    #[test]
    fn reset_cancels_queued_motion_and_presses_before_allowing_new_input() {
        let (io, log, entered, release) = fixture(false);
        let io = Arc::new(io);
        move_by(&io, 1, 0, 0);
        entered.recv_timeout(Duration::from_secs(2)).unwrap();
        move_by(&io, 2000, 0, 0);
        io.submit(Report::Keyboard([0, 0, 4, 0, 0, 0, 0, 0]))
            .unwrap();
        let reset_io = io.clone();
        let reset = thread::spawn(move || reset_io.reset());
        wait_reset(&io.shared);
        release.send(()).unwrap();
        reset.join().unwrap().unwrap();
        io.drain().unwrap();
        assert!(io.mouse_trace().is_empty());
        {
            let frames = log.lock().unwrap();
            assert_eq!(frames.len(), 3);
            assert_eq!(frames[1], (false, vec![0; 8]));
            assert_eq!(frames[2], (true, vec![0; 7]));
        }
        move_by(&io, 7, 0, 0);
        io.drain().unwrap();
        assert_eq!(io.mouse_trace().last().unwrap().position, [7, 0]);
        assert_eq!(xy(&log.lock().unwrap().last().unwrap().1), (7, 0));
    }
    #[test]
    fn stop_joins_the_writer_and_cannot_replay_pending_motion_or_short_presses() {
        let (mut io, log, entered, release) = fixture(false);
        move_by(&io, 1, 0, 0);
        entered.recv_timeout(Duration::from_secs(2)).unwrap();
        move_by(&io, 1000, 0, 0);
        io.submit(Report::Buttons(31)).unwrap();
        let shared = io.shared.clone();
        let stop = thread::spawn(move || io.stop());
        wait_reset(&shared);
        release.send(()).unwrap();
        stop.join().unwrap().unwrap();
        let frames = log.lock().unwrap();
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[1], (false, vec![0; 8]));
        assert_eq!(frames[2], (true, vec![0; 7]));
    }
    #[test]
    fn report_splitting_preserves_signed_movement_and_both_wheels() {
        let log = Arc::new(Mutex::new(vec![]));
        let fake = |mouse| Fake {
            mouse,
            log: log.clone(),
            gate: None,
            fail_press: false,
        };
        let io = KeyMouse::start(fake(false), fake(true)).unwrap();
        move_by(&io, 70000, -70000, 24);
        io.submit(Report::Scroll {
            buttons: 24,
            horizontal: 300,
            vertical: -300,
        })
        .unwrap();
        io.drain().unwrap();
        let frames = log.lock().unwrap();
        let mut sum = (0, 0, 0i32, 0i32);
        for (mouse, r) in &*frames {
            assert!(*mouse);
            assert_eq!(r[0], 24);
            let (x, y) = xy(r);
            sum.0 += x;
            sum.1 += y;
            sum.2 += i32::from(r[6] as i8);
            sum.3 += i32::from(r[5] as i8);
        }
        assert_eq!(sum, (70000, -70000, 300, -300));
        assert_eq!(frames.len(), 6);
    }
    #[test]
    fn async_failure_is_sticky_discards_pending_presses_and_attempts_both_releases() {
        let (mut io, log, entered, release) = fixture(true);
        move_by(&io, 1, 0, 0);
        entered.recv_timeout(Duration::from_secs(2)).unwrap();
        io.submit(Report::Keyboard([1, 0, 0, 0, 0, 0, 0, 0]))
            .unwrap();
        move_by(&io, 500, 0, 31);
        unblock(&io, release);
        assert!(io
            .drain()
            .unwrap_err()
            .to_string()
            .contains("injected ACK failure"));
        assert!(io.submit(Report::Buttons(31)).is_err());
        assert!(io.reset().is_err());
        assert!(io.stop().is_err());
        let frames = log.lock().unwrap();
        assert_eq!(frames[1], (false, vec![1, 0, 0, 0, 0, 0, 0, 0]));
        assert!(frames[2..].iter().any(|f| *f == (false, vec![0; 8])));
        assert!(frames[2..].iter().any(|f| *f == (true, vec![0; 7])));
        assert!(frames[2..].iter().all(|(_, b)| b.iter().all(|v| *v == 0)));
    }
    #[test]
    fn failed_direct_motion_releases_devices_and_latches_the_error() {
        let log = Arc::new(Mutex::new(vec![]));
        let fake = |mouse| Fake {
            mouse,
            log: log.clone(),
            gate: None,
            fail_press: mouse,
        };
        let io = KeyMouse::start(fake(false), fake(true)).unwrap();
        assert!(io.motion(0, 20, -10).is_err());
        assert!(io.motion(0, 1, 0).is_err());
        {
            let deadline = Instant::now() + Duration::from_secs(2);
            let mut state = io.shared.lock();
            while state.reset_done == 0 {
                let left = deadline.checked_duration_since(Instant::now()).unwrap();
                state = io.shared.wake.wait_timeout(state, left).unwrap().0;
            }
        }
        assert_eq!(
            *log.lock().unwrap(),
            [
                (true, mouse_report(0, 20, -10, 0, 0).to_vec()),
                (false, vec![0; 8]),
                (true, vec![0; 7]),
            ]
        );
        assert_eq!(io.stats().direct_motion_reports, 0);
    }

    #[test]
    fn queue_limits_are_explicit_and_stale_input_is_neutralized_instead_of_replayed() {
        let log = Arc::new(Mutex::new(vec![]));
        let fake = |mouse| Fake {
            mouse,
            log: log.clone(),
            gate: None,
            fail_press: false,
        };
        let io = KeyMouse {
            shared: Arc::new(Shared::new(fake(true))),
            thread: None,
        };
        for _ in 0..MAX_QUEUE {
            io.submit(Report::Buttons(1)).unwrap();
        }
        assert!(io.submit(Report::Buttons(0)).is_err());
        assert_eq!(io.stats().queued, MAX_QUEUE);
        let shared = Arc::new(Shared::new(fake(true)));
        shared.lock().queue.push_back(Pending {
            report: Report::Motion {
                buttons: 0,
                x: 999,
                y: 0,
            },
            queued: Instant::now() - Duration::from_secs(1),
        });
        let keyboard = fake(false);
        let worker_shared = shared.clone();
        let worker = thread::spawn(move || run(&worker_shared, keyboard));
        {
            let mut s = shared.lock();
            let deadline = Instant::now() + Duration::from_secs(2);
            while s.error.is_none() || s.busy {
                let left = deadline.checked_duration_since(Instant::now()).unwrap();
                s = shared.wake.wait_timeout(s, left).unwrap().0;
            }
            assert!(s.error.as_ref().unwrap().contains("100 ms"));
            s.closing = true;
            shared.wake.notify_one();
        }
        worker.join().unwrap();
        assert!(log
            .lock()
            .unwrap()
            .iter()
            .all(|(_, b)| b.iter().all(|v| *v == 0)));
    }
    #[test]
    fn continuous_motion_streams_discrete_edges_confirm_and_idle_reports_stop() {
        type Modes = Arc<Mutex<Vec<(bool, Vec<u8>, Delivery)>>>;
        struct Routed {
            mouse: bool,
            log: Modes,
        }
        impl ReportWriter for Routed {
            fn write(&mut self, bytes: &[u8], mode: Delivery) -> Result<()> {
                self.log
                    .lock()
                    .unwrap()
                    .push((self.mouse, bytes.into(), mode));
                Ok(())
            }
        }
        let log: Modes = Arc::new(Mutex::new(vec![]));
        let io = KeyMouse::start(
            Routed {
                mouse: false,
                log: log.clone(),
            },
            Routed {
                mouse: true,
                log: log.clone(),
            },
        )
        .unwrap();
        io.motion(0, 3, 0).unwrap();
        assert_eq!(io.stats().direct_motion_reports, 1);
        assert_eq!(io.stats().queued, 0);
        io.idle_motion(0).unwrap();
        io.idle_motion(0).unwrap();
        assert_eq!(io.stats().mouse_reports, 2);
        io.submit(Report::Keyboard([1, 0, 0, 0, 0, 0, 0, 0]))
            .unwrap();
        io.submit(Report::Scroll {
            buttons: 0,
            horizontal: 1,
            vertical: -1,
        })
        .unwrap();
        io.submit(Report::Buttons(8)).unwrap();
        io.submit(Report::Keyboard([0; 8])).unwrap();
        io.drain().unwrap();
        let records = log.lock().unwrap();
        assert!(records
            .iter()
            .any(|(m, b, d)| *m && b[1] != 0 && *d == Delivery::Stream));
        assert!(records
            .iter()
            .any(|(m, b, d)| *m && b.iter().all(|b| *b == 0) && *d == Delivery::Stream));
        assert!(records
            .iter()
            .filter(|(m, b, _)| !m || b[5..7] != [0; 2] || b[0] == 8)
            .all(|(_, _, d)| *d == Delivery::Confirmed));
    }
    #[test]
    fn absolute_mouse_deadlines_do_not_accumulate_drift_or_replay_a_stall_as_a_burst() {
        let start = Instant::now();
        assert_eq!(
            next_output_deadline(start, start + Duration::from_micros(100)),
            start + CONTINUOUS_OUTPUT_INTERVAL
        );
        let missed = start + CONTINUOUS_OUTPUT_INTERVAL;
        assert_eq!(
            next_output_deadline(start, missed),
            missed + CONTINUOUS_OUTPUT_INTERVAL
        );
        let stalled = start + Duration::from_millis(100);
        assert_eq!(
            next_output_deadline(start, stalled),
            stalled + CONTINUOUS_OUTPUT_INTERVAL
        );
    }
}
