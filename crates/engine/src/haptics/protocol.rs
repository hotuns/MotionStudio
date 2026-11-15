// SPDX-FileCopyrightText: 2026 Mikalai Kaliaha
// SPDX-License-Identifier: MIT
// Bounded Rust port of OpenFlydigi gpa6.py (9ab15b4b), volatile motor operations only.
use anyhow::{bail, ensure, Result};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug)]
pub struct Layout {
    pub output_id: u8,
    pub output_size: usize,
    pub input_ids: Vec<u8>,
}
impl Layout {
    pub fn parse(descriptor: &[u8]) -> Result<Self> {
        // page, id, report-size, count. Report size includes constant padding.
        let mut state = [0u32; 4];
        let mut stack = Vec::new();
        let mut collections = Vec::new();
        let mut sizes = BTreeMap::new();
        let mut vendor = BTreeSet::new();
        let mut pos = 0;
        while pos < descriptor.len() {
            let prefix = descriptor[pos];
            pos += 1;
            ensure!(prefix != 0xfe, "Unsupported HID long item");
            let size = [0, 1, 2, 4][(prefix & 3) as usize];
            let data = descriptor
                .get(pos..pos + size)
                .ok_or_else(|| anyhow::anyhow!("Truncated HID descriptor"))?;
            pos += size;
            let value = data
                .iter()
                .enumerate()
                .fold(0u32, |n, (i, b)| n | ((*b as u32) << (i * 8)));
            match ((prefix >> 2) & 3, prefix >> 4) {
                (1, 0) => state[0] = value,
                (1, 8) => state[1] = value,
                (1, 7) => state[2] = value,
                (1, 9) => state[3] = value,
                (1, 10) => stack.push(state),
                (1, 11) => {
                    state = stack
                        .pop()
                        .ok_or_else(|| anyhow::anyhow!("Unbalanced HID globals"))?
                }
                (0, 10) => collections.push(state[0] == 0xffa0 || collections.contains(&true)),
                (0, 12) => {
                    ensure!(collections.pop().is_some(), "Unbalanced HID collection");
                }
                (0, tag @ (8 | 9)) => {
                    let key = (tag, state[1]);
                    let total = sizes.entry(key).or_insert(0u32);
                    *total = total
                        .checked_add(
                            state[2]
                                .checked_mul(state[3])
                                .ok_or_else(|| anyhow::anyhow!("HID size overflow"))?,
                        )
                        .ok_or_else(|| anyhow::anyhow!("HID size overflow"))?;
                    if collections.contains(&true) {
                        vendor.insert(key);
                    }
                }
                _ => {}
            }
        }
        let outputs: Vec<_> = vendor.iter().filter(|(tag, _)| *tag == 9).collect();
        let inputs: Vec<_> = vendor.iter().filter(|(tag, _)| *tag == 8).collect();
        ensure!(
            collections.is_empty() && stack.is_empty() && outputs.len() == 1 && !inputs.is_empty(),
            "Select the APEX vendor HID interface for haptics"
        );
        for key in outputs.iter().chain(&inputs) {
            let bits = sizes[key];
            ensure!(
                key.1 <= 255 && (256..=512).contains(&bits) && bits % 8 == 0,
                "Unsupported APEX report size"
            );
        }
        let input_ids: Vec<_> = inputs.iter().map(|k| k.1 as u8).collect();
        ensure!(
            !input_ids.contains(&0) || input_ids.len() == 1,
            "Mixed HID report IDs"
        );
        Ok(Self {
            output_id: outputs[0].1 as u8,
            output_size: sizes[outputs[0]] as usize / 8,
            input_ids,
        })
    }
    pub fn wrap(&self, frame: &[u8; 32]) -> Vec<u8> {
        let mut report = vec![0; self.output_size + 1];
        report[0] = self.output_id;
        report[1..33].copy_from_slice(frame);
        report
    }
    pub fn unwrap<'a>(&self, report: &'a [u8]) -> &'a [u8] {
        if self.input_ids == [0] {
            report
        } else if report.first().is_some_and(|id| self.input_ids.contains(id)) {
            &report[1..]
        } else {
            &[]
        }
    }
}
pub fn frame(command: u8, payload: &[u8]) -> [u8; 32] {
    assert!(payload.len() <= 27);
    let mut data = [0u8; 32];
    data[..4].copy_from_slice(&[0x5a, 0xa5, command, payload.len() as u8 + 2]);
    data[4..4 + payload.len()].copy_from_slice(payload);
    data[31] = data[2..31].iter().fold(0u8, |s, b| s.wrapping_add(*b));
    data
}
pub fn reply(body: &[u8], command: u8, size: usize, v21: bool) -> Result<Option<Vec<u8>>> {
    if body.len() < 4 || body[..3] != [0x5a, 0xa5, command] {
        return Ok(None);
    }
    let start = if v21 { 3 } else { 5 };
    let error = !v21 && body[3] == 0;
    let end = start + if error { 1 } else { size };
    for check in [end, 31, body.len() - 1] {
        if check < end || check >= body.len() {
            continue;
        }
        if body[end..check]
            .iter()
            .chain(&body[check + 1..])
            .any(|b| *b != 0)
        {
            continue;
        }
        if body[2..check].iter().fold(0u8, |s, b| s.wrapping_add(*b)) != body[check] {
            continue;
        }
        ensure!(!error, "APEX refused command {command:#x}");
        ensure!(v21 || body[3..5] == [1, 0], "Unexpected APEX reply index");
        return Ok(Some(body[start..end].to_vec()));
    }
    bail!("Malformed APEX reply for {command:#x}")
}
pub fn restore(mapping: &[u8]) -> Result<Vec<[u8; 32]>> {
    ensure!(
        mapping.len() == 64 && mapping[0] == 2,
        "Unsupported APEX motor map"
    );
    let mut result = Vec::new();
    for (target, offset, widths) in [
        (0, 1, &[0usize, 10, 3][..]),
        (1, 12, &[0, 10, 3][..]),
        (0x10, 23, &[0, 1][..]),
        (0x11, 25, &[0, 1][..]),
    ] {
        let mode = mapping[offset];
        let width = *widths
            .get(mode as usize)
            .ok_or_else(|| anyhow::anyhow!("Unsupported APEX motor mode"))?;
        let mut params = vec![1, target, mode];
        params.extend_from_slice(&mapping[offset + 1..offset + 1 + width]);
        result.push(frame(0x53, &params));
    }
    Ok(result)
}
pub fn waveform(samples: [[f32; 2]; 8]) -> [u8; 32] {
    let mut payload = vec![0x98]; // left/right enabled; trigger column disabled
    for pair in samples {
        payload.push(128);
        for v in pair {
            payload.push(if v.is_finite() {
                ((v.clamp(-1., 1.) + 1.) * 127.5).round() as u8
            } else {
                128
            });
        }
    }
    frame(0x57, &payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn packet_fixture_has_signed_stereo_and_neutral_trigger() {
        let packet = waveform([[-1., 1.]; 8]);
        assert_eq!(&packet[..8], &[0x5a, 0xa5, 0x57, 27, 0x98, 128, 0, 255]);
        assert_eq!(
            packet[31],
            packet[2..31].iter().fold(0u8, |s, b| s.wrapping_add(*b))
        );
        assert!(waveform([[f32::NAN, 0.]; 8])[5..29]
            .iter()
            .all(|b| *b == 128));
    }
    #[test]
    fn descriptor_accepts_unnumbered_and_checks_vendor_collection() {
        let descriptor = [
            0x06, 0xa0, 0xff, 0x09, 1, 0xa1, 1, 0x75, 8, 0x95, 32, 0x81, 2, 0x91, 2, 0xc0,
        ];
        let layout = Layout::parse(&descriptor).unwrap();
        assert_eq!(layout.output_id, 0);
        assert_eq!(layout.wrap(&frame(1, &[])).len(), 33);
        let mut wrong = descriptor;
        wrong[2] = 1;
        assert!(Layout::parse(&wrong).is_err());
        assert!(Layout::parse(&descriptor[..10]).is_err());
    }
    #[test]
    fn motor_restore_covers_all_motors_and_rejects_unknown_layout() {
        let mut map = [0u8; 64];
        map[0] = 2;
        map[23] = 1;
        map[24] = 87;
        let commands = restore(&map).unwrap();
        assert_eq!(commands.len(), 4);
        assert_eq!(&commands[2][4..8], &[1, 0x10, 1, 87]);
        map[1] = 3;
        assert!(restore(&map).is_err());
    }
}
