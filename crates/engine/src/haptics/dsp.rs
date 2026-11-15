// SPDX-FileCopyrightText: 2026 Mikalai Kaliaha
// SPDX-License-Identifier: MIT
// Rust port of OpenFlydigi waveform.py::StereoResampler, commit 9ab15b4b.
// Studio adds bounded buffers and reset semantics in the session layer.
use std::f64::consts::PI;

pub const RATE: usize = 1000;
pub const BLOCK: usize = 8;
const TAPS: usize = 769;

pub struct Resampler {
    taps: [f32; TAPS],
    history: [[f32; 2]; TAPS],
    cursor: usize,
    phase: usize,
    pending: Vec<u8>,
}
impl Default for Resampler {
    fn default() -> Self {
        let mut taps = [0.; TAPS];
        for (i, tap) in taps.iter_mut().enumerate() {
            let x = i as f64 - 384.;
            let sinc = if x == 0. {
                700. / 48000.
            } else {
                (2. * PI * 350. * x / 48000.).sin() / (PI * x)
            };
            let window = 0.42 - 0.5 * (2. * PI * i as f64 / 768.).cos()
                + 0.08 * (4. * PI * i as f64 / 768.).cos();
            *tap = (sinc * window) as f32;
        }
        let sum: f32 = taps.iter().sum();
        for t in &mut taps {
            *t /= sum;
        }
        Self {
            taps,
            history: [[0.; 2]; TAPS],
            cursor: 0,
            phase: 0,
            pending: Vec::with_capacity(8),
        }
    }
}
impl Resampler {
    pub fn reset(&mut self) {
        self.history.fill([0.; 2]);
        self.cursor = 0;
        self.phase = 0;
        self.pending.clear();
    }
    pub fn feed(&mut self, pcm: &[u8], mut output: impl FnMut([f32; 2])) {
        for &byte in pcm {
            self.pending.push(byte);
            if self.pending.len() != 8 {
                continue;
            }
            self.history[self.cursor] = [
                i16::from_le_bytes([self.pending[4], self.pending[5]]) as f32 / 32768.,
                i16::from_le_bytes([self.pending[6], self.pending[7]]) as f32 / 32768.,
            ];
            self.pending.clear();
            self.cursor = (self.cursor + 1) % TAPS;
            self.phase += 1;
            if self.phase == 48 {
                self.phase = 0;
                let mut sample = [0.; 2];
                for (i, t) in self.taps.iter().enumerate() {
                    let h = self.history[(self.cursor + i) % TAPS];
                    sample[0] += h[0] * t;
                    sample[1] += h[1] * t;
                }
                output(sample);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sine(hz: f64) -> Vec<u8> {
        (0..48000)
            .flat_map(|i| {
                let left = ((2. * PI * hz * i as f64 / 48000.).sin() * 16000.) as i16;
                [1234i16, -4321, left, 0]
                    .into_iter()
                    .flat_map(i16::to_le_bytes)
            })
            .collect()
    }
    #[test]
    fn preserves_channels_timing_and_chunk_boundaries() {
        let pcm = sine(100.);
        let mut whole = Vec::new();
        Resampler::default().feed(&pcm, |s| whole.push(s));
        let mut split = Vec::new();
        let mut converter = Resampler::default();
        for chunk in pcm.chunks(107) {
            converter.feed(chunk, |s| split.push(s));
        }
        assert_eq!(whole, split);
        assert_eq!(whole.len(), 1000);
        assert!(whole.iter().all(|s| s[1] == 0.));
        let peak = whole[40..].iter().map(|s| s[0].abs()).fold(0., f32::max);
        assert!((0.44..0.51).contains(&peak));
        let crossings = whole[40..]
            .windows(2)
            .filter(|s| s[0][0] < 0. && s[1][0] >= 0.)
            .count();
        assert!((95..=97).contains(&crossings));
    }
    #[test]
    fn rejects_aliases_and_reset_erases_history() {
        let mut out = Vec::new();
        let mut converter = Resampler::default();
        converter.feed(&sine(1400.), |s| out.push(s));
        assert!(out[40..].iter().all(|s| s[0].abs() < 0.002));
        converter.reset();
        converter.feed(&vec![0; 4800 * 8], |s| assert_eq!(s, [0.; 2]));
    }
}
