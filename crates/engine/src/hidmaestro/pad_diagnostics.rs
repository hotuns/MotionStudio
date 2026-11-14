//! Nonblocking observation of the existing shared-memory delivery contract.
//! ACK means HID class requested its next report, not that a game rendered it.
use crate::output::{HidGamepadSample, HidGamepadStats};
use flexinput_hidmaestro::shm::DeliveryAck;
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

pub(super) struct PadDiagnostics {
    ack: Option<DeliveryAck>,
    started: Instant,
    last_submit: Option<Instant>,
    last_ack: Option<u32>,
    last_ack_progress: Instant,
    last_sample: Option<Instant>,
    stats: HidGamepadStats,
    samples: VecDeque<HidGamepadSample>,
    last_motion: VecDeque<HidGamepadSample>,
    stalls: VecDeque<HidGamepadSample>,
}

impl PadDiagnostics {
    pub(super) fn new(index: u32, identity: Option<[u16; 2]>) -> Self {
        let ack = DeliveryAck::open(index).ok();
        let now = Instant::now();
        Self {
            last_ack: ack.as_ref().map(DeliveryAck::delivered_sequence),
            stats: HidGamepadStats {
                ack_available: ack.is_some(),
                hid_identity: identity,
                ..Default::default()
            },
            ack,
            started: now,
            last_submit: None,
            last_ack_progress: now,
            last_sample: None,
            samples: VecDeque::with_capacity(128),
            last_motion: VecDeque::with_capacity(128),
            stalls: VecDeque::with_capacity(32),
        }
    }

    pub(super) fn submitted(
        &mut self,
        seq: u32,
        axes: Option<[u8; 2]>,
        began: Instant,
        now: Instant,
    ) {
        let us = |duration: Duration| duration.as_micros().min(u64::MAX as u128) as u64;
        // Device creation and visibility checks are not active streaming gaps.
        if self.last_submit.is_none() {
            self.started = now;
            self.last_ack_progress = now;
        }
        let gap = self
            .last_submit
            .map_or(0, |last| us(now.duration_since(last)));
        let call = us(now.duration_since(began));
        self.last_submit = Some(now);
        self.stats.submitted_reports += 1;
        self.stats.max_submit_gap_us = self.stats.max_submit_gap_us.max(gap);
        self.stats.max_submit_call_us = self.stats.max_submit_call_us.max(call);
        let acknowledged = self.ack.as_ref().map(DeliveryAck::delivered_sequence);
        let ack_gap = acknowledged.map(|_| us(now.duration_since(self.last_ack_progress)));
        if acknowledged.is_some() {
            self.stats.max_ack_gap_us = self
                .stats
                .max_ack_gap_us
                .max(us(now.duration_since(self.last_ack_progress)));
        }
        if acknowledged != self.last_ack {
            self.stats.ack_observations += 1;
            self.last_ack = acknowledged;
            self.last_ack_progress = now;
        }
        let age = acknowledged.map(|_| us(now.duration_since(self.last_ack_progress)));
        // A modulo-forward difference is valid across wrap. Do not report an
        // ahead/stale foreign sequence as billions of outstanding frames.
        let pending = acknowledged.and_then(|ack| {
            let distance = seq.wrapping_sub(ack);
            (distance < 0x8000_0000).then_some(distance / 2)
        });
        self.stats.max_pending_reports = self.stats.max_pending_reports.max(pending.unwrap_or(0));
        self.stats.max_ack_gap_us = self.stats.max_ack_gap_us.max(age.unwrap_or(0));
        let sample = HidGamepadSample {
            elapsed_us: us(now.duration_since(self.started)),
            submitted_seq: seq,
            acknowledged_seq: acknowledged,
            pending_reports: pending,
            since_ack_progress_us: age,
            ack_observation_gap_us: ack_gap,
            submit_gap_us: gap,
            submit_call_us: call,
            ds4_right_stick: axes,
        };
        // Retain slow observations after the user returns to Studio to export.
        // These are evidence, not a declaration that an 8 ms interval is a fault.
        if gap >= 8_000 || call >= 8_000 || ack_gap.is_some_and(|gap| gap >= 8_000) {
            if self.stalls.len() == 32 {
                self.stalls.pop_front();
            }
            self.stalls.push_back(sample.clone());
        }
        if self
            .last_sample
            .is_none_or(|last| now.duration_since(last) >= Duration::from_millis(8))
        {
            self.last_sample = Some(now);
            if self.samples.len() == 128 {
                self.samples.pop_front();
            }
            // The original DS4 encoder emits [127, 127] at rest. Preserve the
            // last deflected samples instead of replacing them with menu idle.
            // This says nothing about a game's deadzone or actual camera motion.
            if sample
                .ds4_right_stick
                .is_some_and(|axes| axes != [127, 127])
            {
                if self.last_motion.len() == 128 {
                    self.last_motion.pop_front();
                }
                self.last_motion.push_back(sample.clone());
            }
            self.samples.push_back(sample);
        }
    }

    pub(super) fn snapshot(&self) -> HidGamepadStats {
        let mut stats = self.stats.clone();
        stats.recent = self.samples.iter().cloned().collect();
        stats.last_motion = self.last_motion.iter().cloned().collect();
        stats.stalls = self.stalls.iter().cloned().collect();
        stats
    }
}
