//! Native haptic PCM transport and selected-device lifecycle. Physical I/O is
//! isolated from SDL's timestamped event loop and the USB/IP pacing callback.
mod apex;
mod dsp;
mod protocol;
pub mod transport;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

const MAX_BYTES: usize = 40 * 48 * 8; // 40 ms of four-channel 48 kHz s16 PCM
const MAX_AGE: Duration = Duration::from_millis(40);
const PERIOD: Duration = Duration::from_millis(8);

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct HapticStatus {
    pub state: String,
    pub received_frames: u64,
    pub sent_packets: u64,
    pub dropped_frames: u64,
    pub underrun_samples: u64,
    pub clipped_samples: u64,
    pub queued_ms: usize,
    pub error: Option<String>,
}
struct Chunk {
    at: Instant,
    epoch: u64,
    pcm: Vec<u8>,
}
struct Inbox {
    chunks: VecDeque<Chunk>,
    bytes: usize,
    enabled: bool,
    gain: f32,
    rumble: [f32; 2],
    heartbeat: Instant,
    status: HapticStatus,
    uid: Option<Vec<u8>>,
}
#[derive(Clone)]
pub struct Bus {
    inbox: Arc<Mutex<Inbox>>,
    epoch: Arc<AtomicU64>,
}
impl Default for Bus {
    fn default() -> Self {
        Self {
            inbox: Arc::new(Mutex::new(Inbox {
                chunks: VecDeque::new(),
                bytes: 0,
                enabled: false,
                gain: 1.,
                rumble: [0.; 2],
                heartbeat: Instant::now(),
                status: HapticStatus {
                    state: "starting".into(),
                    ..Default::default()
                },
                uid: None,
            })),
            epoch: Arc::new(AtomicU64::new(0)),
        }
    }
}
impl Bus {
    pub fn pcm_sink(&self) -> flexinput_usbip::PcmSink {
        let bus = self.clone();
        Box::new(move |pcm| bus.push(pcm, Instant::now()))
    }
    fn push(&self, pcm: &[u8], now: Instant) {
        let Ok(mut inbox) = self.inbox.try_lock() else {
            self.epoch.fetch_add(1, Ordering::AcqRel);
            return;
        };
        if !inbox.enabled
            || now.saturating_duration_since(inbox.heartbeat) > Duration::from_millis(100)
        {
            return;
        }
        if pcm.is_empty() || pcm.len() % 8 != 0 {
            inbox.chunks.clear();
            inbox.bytes = 0;
            self.epoch.fetch_add(1, Ordering::AcqRel);
            return;
        }
        inbox.status.received_frames += (pcm.len() / 8) as u64;
        if pcm.len() > MAX_BYTES || inbox.bytes + pcm.len() > MAX_BYTES {
            inbox.status.dropped_frames += (inbox.bytes / 8) as u64;
            inbox.chunks.clear();
            inbox.bytes = 0;
            self.epoch.fetch_add(1, Ordering::AcqRel);
        }
        let skip = pcm.len().saturating_sub(MAX_BYTES);
        inbox.status.dropped_frames += (skip / 8) as u64;
        let pcm = pcm[skip..].to_vec();
        inbox.bytes += pcm.len();
        inbox.chunks.push_back(Chunk {
            at: now,
            epoch: self.epoch.load(Ordering::Acquire),
            pcm,
        });
    }
    pub fn update(&self, enabled: bool, gain: f32, rumble: [f32; 2]) {
        let mut i = self.inbox.lock().unwrap();
        if enabled != i.enabled {
            i.chunks.clear();
            i.bytes = 0;
            self.epoch.fetch_add(1, Ordering::AcqRel);
        }
        i.enabled = enabled;
        i.gain = if gain.is_finite() {
            gain.clamp(0., 2.)
        } else {
            0.
        };
        i.rumble = if enabled {
            rumble.map(|v| if v.is_finite() { v.clamp(0., 1.) } else { 0. })
        } else {
            [0.; 2]
        };
        i.heartbeat = Instant::now();
    }
    pub fn suspend(&self) {
        self.update(false, 0., [0.; 2]);
    }
    pub fn status(&self) -> HapticStatus {
        self.inbox.lock().unwrap().status.clone()
    }
    pub fn identity(&self) -> Option<Vec<u8>> {
        self.inbox.lock().unwrap().uid.clone()
    }
}

struct Renderer {
    filter: dsp::Resampler,
    samples: VecDeque<(Instant, [f32; 2])>,
    last_feed: Option<Instant>,
    sample_clock: u64,
}
impl Default for Renderer {
    fn default() -> Self {
        Self {
            filter: Default::default(),
            samples: VecDeque::with_capacity(40),
            last_feed: None,
            sample_clock: 0,
        }
    }
}
impl Renderer {
    fn reset(&mut self) {
        self.filter.reset();
        self.samples.clear();
        self.last_feed = None;
        self.sample_clock = 0;
    }
    fn feed(&mut self, chunk: Chunk, now: Instant) -> u64 {
        if now.saturating_duration_since(chunk.at) > MAX_AGE {
            self.reset();
            return (chunk.pcm.len() / 8) as u64;
        }
        if self
            .last_feed
            .is_some_and(|t| chunk.at.saturating_duration_since(t) > MAX_AGE)
        {
            self.reset();
        }
        self.last_feed = Some(chunk.at);
        self.filter
            .feed(&chunk.pcm, |s| self.samples.push_back((chunk.at, s)));
        let excess = self.samples.len().saturating_sub(40);
        self.samples.drain(..excess);
        excess as u64 * 48
    }
    fn render(
        &mut self,
        now: Instant,
        enabled: bool,
        gain: f32,
        rumble: [f32; 2],
    ) -> ([[f32; 2]; dsp::BLOCK], u64, u64) {
        if !enabled {
            self.reset();
            return ([[0.; 2]; dsp::BLOCK], 0, 0);
        }
        while self
            .samples
            .front()
            .is_some_and(|(at, _)| now.saturating_duration_since(*at) > MAX_AGE)
        {
            self.samples.pop_front();
        }
        let mut output = [[0.; 2]; dsp::BLOCK];
        let mut underrun = 0;
        let mut clipped = 0;
        for pair in &mut output {
            let sample = self.samples.pop_front().map(|s| s.1).unwrap_or_else(|| {
                underrun += 1;
                [0.; 2]
            });
            for side in 0..2 {
                // OpenFlydigi ds5_apex6.py's ordinary rumble approximation,
                // mixed by the one motor owner while stream mode is active.
                let phase = self.sample_clock as f64 / dsp::RATE as f64
                    * [80., 160.][side]
                    * std::f64::consts::TAU;
                let value = sample[side] * gain + 0.35 * rumble[side] * phase.sin() as f32;
                clipped += u64::from(value.abs() > 1.);
                pair[side] = value.clamp(-1., 1.);
            }
            self.sample_clock += 1;
        }
        (output, underrun, clipped)
    }
}

pub struct Session {
    pub bus: Bus,
    cancel: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
impl Session {
    pub fn start(factory: transport::Factory, expected: Option<Vec<u8>>) -> Result<Self> {
        let bus = Bus::default();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_bus = bus.clone();
        let worker_cancel = cancel.clone();
        let worker = std::thread::Builder::new()
            .name("apex-haptics".into())
            .spawn(move || {
                let run = || -> Result<()> {
                    let mut device = apex::Apex::new(factory()?, worker_cancel.clone(), expected)?;
                    {
                        let mut inbox = worker_bus.inbox.lock().unwrap();
                        inbox.uid = Some(device.uid.clone());
                        inbox.chunks.clear();
                        inbox.bytes = 0;
                    }
                    let waiter = flexinput_engine::HrWaiter::new();
                    let mut renderer = Renderer::default();
                    let mut epoch = worker_bus.epoch.load(Ordering::Acquire);
                    let mut next = Instant::now();
                    let result = (|| -> Result<()> {
                        while !worker_cancel.load(Ordering::Acquire) {
                            let now = Instant::now();
                            let (chunks, active, gain, rumble, current) = {
                                let mut i = worker_bus.inbox.lock().unwrap();
                                let chunks = std::mem::take(&mut i.chunks);
                                i.bytes = 0;
                                (
                                    chunks,
                                    i.enabled
                                        && now.saturating_duration_since(i.heartbeat)
                                            < Duration::from_millis(100),
                                    i.gain,
                                    i.rumble,
                                    worker_bus.epoch.load(Ordering::Acquire),
                                )
                            };
                            if current != epoch || now.saturating_duration_since(next) > MAX_AGE {
                                renderer.reset();
                                epoch = current;
                            }
                            let mut dropped = 0;
                            for chunk in chunks {
                                if chunk.epoch == current {
                                    dropped += renderer.feed(chunk, now);
                                } else {
                                    dropped += (chunk.pcm.len() / 8) as u64;
                                }
                            }
                            let (packet, underrun, clipped) =
                                renderer.render(now, active, gain, rumble);
                            // Recheck transitions after DSP, before emitting the packet.
                            let packet = if worker_cancel.load(Ordering::Acquire)
                                || epoch != worker_bus.epoch.load(Ordering::Acquire)
                            {
                                [[0.; 2]; 8]
                            } else {
                                packet
                            };
                            device.write(packet)?;
                            {
                                let mut i = worker_bus.inbox.lock().unwrap();
                                i.status.state = if active { "streaming" } else { "paused" }.into();
                                i.status.sent_packets += 1;
                                i.status.dropped_frames += dropped;
                                i.status.underrun_samples += underrun;
                                i.status.clipped_samples += clipped;
                                i.status.queued_ms = renderer.samples.len() + i.bytes / (48 * 8);
                            }
                            next += PERIOD;
                            if next <= Instant::now() {
                                next = Instant::now() + PERIOD;
                            }
                            waiter.wait(next.saturating_duration_since(Instant::now()));
                        }
                        Ok(())
                    })();
                    let stop = device.stop();
                    match (result, stop) {
                        (Err(e), Err(cleanup)) => {
                            anyhow::bail!("{e:#}; motor restoration failed: {cleanup:#}")
                        }
                        (Err(e), _) | (_, Err(e)) => Err(e),
                        _ => Ok(()),
                    }
                };
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(run));
                worker_bus.suspend();
                let mut i = worker_bus.inbox.lock().unwrap();
                i.status.queued_ms = 0;
                match result {
                    Ok(Ok(())) => i.status.state = "stopped".into(),
                    Ok(Err(e)) => {
                        i.status.state = "error".into();
                        i.status.error = Some(format!("{e:#}"));
                    }
                    Err(_) => {
                        i.status.state = "error".into();
                        i.status.error =
                            Some("Haptic worker panicked; motor restoration is unconfirmed".into());
                    }
                }
            })
            .context("Start haptic worker")?;
        Ok(Self {
            bus,
            cancel,
            worker: Some(worker),
        })
    }
    pub fn stop(&mut self) -> Result<()> {
        self.bus.suspend();
        self.cancel.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| anyhow::anyhow!("Haptic worker failed"))?;
        }
        if let Some(error) = self.bus.status().error {
            anyhow::bail!("{error}");
        }
        Ok(())
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mock_worker_relays_pcm_then_suspends_and_joins_without_replay() {
        let (factory, writes) = apex::tests::factory();
        let mut session = Session::start(factory, None).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while session.bus.identity().is_none() {
            assert!(
                Instant::now() < deadline,
                "{}",
                session.bus.status().error.unwrap_or_default()
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        session.bus.update(true, 1., [0.; 2]);
        let pcm: Vec<_> = (0..1920)
            .flat_map(|_| [0i16, 0, 20000, 0].into_iter().flat_map(i16::to_le_bytes))
            .collect();
        session.bus.push(&pcm, Instant::now());
        loop {
            if writes
                .lock()
                .unwrap()
                .iter()
                .any(|p| p[2] == 0x57 && (6..29).step_by(3).any(|i| p[i] > 150))
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "No stereo waveform reached mock HID"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(writes
            .lock()
            .unwrap()
            .iter()
            .filter(|p| p[2] == 0x57)
            .all(|p| (7..29).step_by(3).all(|i| p[i] == 128)));
        session.bus.suspend();
        std::thread::sleep(Duration::from_millis(24));
        assert!(writes
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|p| p[2] == 0x57)
            .unwrap()[5..29]
            .iter()
            .all(|v| *v == 128));
        session.stop().unwrap();
        let count = writes.lock().unwrap().len();
        std::thread::sleep(Duration::from_millis(12));
        assert_eq!(writes.lock().unwrap().len(), count);
        assert_eq!(session.bus.status().state, "stopped");
    }
    #[test]
    fn callback_is_bounded_and_suspend_discards_old_audio() {
        let bus = Bus::default();
        bus.update(true, 1., [1.; 2]);
        for _ in 0..12 {
            bus.push(&vec![0; 384 * 8], Instant::now());
        }
        let i = bus.inbox.lock().unwrap();
        assert!(i.bytes <= MAX_BYTES);
        assert!(i.status.dropped_frames > 0);
        drop(i);
        bus.suspend();
        bus.push(&[0; 384], Instant::now());
        let i = bus.inbox.lock().unwrap();
        assert!(i.chunks.is_empty());
        assert_eq!(i.rumble, [0.; 2]);
    }
    #[test]
    fn expired_waveforms_and_disabled_sessions_are_silent() {
        let now = Instant::now();
        let mut r = Renderer::default();
        let expired = Chunk {
            at: now - Duration::from_millis(50),
            epoch: 0,
            pcm: vec![127; 384 * 8],
        };
        assert_eq!(r.feed(expired, now), 384);
        assert_eq!(r.render(now, true, 1., [0.; 2]).0, [[0.; 2]; 8]);
        assert_eq!(r.render(now, false, 1., [1.; 2]).0, [[0.; 2]; 8]);
        assert!(r
            .render(now, true, 1., [1., 0.])
            .0
            .iter()
            .any(|s| s[0] != 0.));
        assert!(r
            .render(now, true, 1., [1., 0.])
            .0
            .iter()
            .all(|s| s[1] == 0.));
    }
}
