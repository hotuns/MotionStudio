//! Volatile, identity-bound APEX grip mode. Never writes persistent settings.
use super::{
    protocol::{self, frame},
    transport::Transport,
};
use anyhow::{bail, ensure, Result};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    sync::Arc,
    time::{Duration, Instant},
};

pub struct Apex {
    io: Box<dyn Transport>,
    pub uid: Vec<u8>,
    slot: u8,
    dirty: bool,
    cancel: Arc<AtomicBool>,
    probe: Option<(u8, Instant)>,
    next_probe: Instant,
}
impl Apex {
    pub fn new(
        io: Box<dyn Transport>,
        cancel: Arc<AtomicBool>,
        expected: Option<Vec<u8>>,
    ) -> Result<Self> {
        let mut s = Self {
            io,
            uid: Vec::new(),
            slot: 0,
            dirty: false,
            cancel,
            probe: None,
            next_probe: Instant::now() + Duration::from_millis(500),
        };
        let info = s.ask(1, &[], 25, false, true)?;
        ensure!(
            matches!(info[0], 150 | 152) && matches!(info[1], 0 | 1) && info[24] & 0x80 != 0,
            "Selected APEX does not report supported Pro linear grips over USB/receiver"
        );
        s.uid = s.ask(4, &[], 16, false, true)?;
        ensure!(
            s.uid.iter().any(|b| *b != 0),
            "APEX returned an empty device identity"
        );
        ensure!(
            expected.as_ref().is_none_or(|id| *id == s.uid),
            "APEX identity changed; select the intended controller again"
        );
        s.slot = s.ask(0xa1, &[], 11, false, true)?[0];
        ensure!(s.slot <= 3, "Invalid APEX profile slot");
        // Validate restoration before altering any volatile motor mode.
        let map = s.motor_map(true)?;
        protocol::restore(&map)?;
        ensure!(!s.cancel.load(Ordering::Acquire), "Haptic setup cancelled");
        s.dirty = true;
        if let Err(error) =
            s.io.write(&frame(0x53, &[1, 2, 0]))
                .and_then(|_| s.io.write(&frame(0x53, &[1, 0x12, 2, 0x40, 0])))
        {
            return match s.stop() {
                Ok(()) => Err(error),
                Err(cleanup) => Err(anyhow::anyhow!(
                    "{error:#}; partial setup restoration failed: {cleanup:#}"
                )),
            };
        }
        Ok(s)
    }
    fn ask(
        &mut self,
        command: u8,
        payload: &[u8],
        size: usize,
        v21: bool,
        cancellable: bool,
    ) -> Result<Vec<u8>> {
        // Discard replies from earlier commands; ordinary EF input is untouched
        // in SDL's own HID handle. Bound the drain against continuous input.
        for _ in 0..128 {
            if self.io.read(0)?.is_empty() {
                break;
            }
        }
        self.io.write(&frame(command, payload))?;
        let until = Instant::now() + Duration::from_millis(300);
        while Instant::now() < until {
            ensure!(
                !cancellable || !self.cancel.load(Ordering::Acquire),
                "Haptic setup cancelled"
            );
            let data = self.io.read(5)?;
            if let Some(result) = protocol::reply(&data, command, size, v21)? {
                return Ok(result);
            }
        }
        bail!("APEX command {command:#x} timed out")
    }
    fn motor_map(&mut self, cancellable: bool) -> Result<Vec<u8>> {
        let slot = self.ask(0xa1, &[], 11, false, cancellable)?[0];
        ensure!(slot <= 3, "Invalid APEX motor profile slot");
        let mut mapping = Vec::new();
        for index in 0..4 {
            // The final chunk is four bytes. Native replies declare their
            // meaningful payload width, not the request's maximum of 20.
            let count = (64 - mapping.len()).min(20);
            let p = self.ask(0xa3, &[0, 6, index, 20], 8 + count, true, cancellable)?;
            ensure!(
                p[..7] == [0, 0, 6, slot, 64, 0, index] && p[7] as usize == count,
                "Invalid APEX motor RAM reply"
            );
            mapping.extend_from_slice(&p[8..]);
        }
        Ok(mapping)
    }
    pub fn write(&mut self, samples: [[f32; 2]; 8]) -> Result<()> {
        self.check_identity()?;
        self.io.write(&protocol::waveform(samples))
    }
    fn check_identity(&mut self) -> Result<()> {
        for _ in 0..128 {
            let data = self.io.read(0)?;
            if data.is_empty() {
                break;
            }
            if let Some((cmd, _)) = self.probe {
                if let Some(p) = protocol::reply(&data, cmd, if cmd == 4 { 16 } else { 11 }, false)?
                {
                    if cmd == 4 && p != self.uid {
                        self.dirty = false; // Never restore the former pad's modes onto its replacement.
                        bail!("APEX identity changed during haptic output");
                    }
                    ensure!(
                        cmd != 0xa1 || p[0] == self.slot,
                        "APEX profile changed; restart haptic output"
                    );
                    self.probe = None;
                    if cmd == 4 {
                        self.io.write(&frame(0xa1, &[]))?;
                        self.probe = Some((0xa1, Instant::now()));
                    }
                }
            }
        }
        if let Some((_, sent)) = self.probe {
            ensure!(
                sent.elapsed() < Duration::from_millis(300),
                "APEX identity heartbeat timed out"
            );
        } else if Instant::now() >= self.next_probe {
            self.io.write(&frame(4, &[]))?;
            self.probe = Some((4, Instant::now()));
            self.next_probe = Instant::now() + Duration::from_millis(500);
        }
        Ok(())
    }
    pub fn stop(&mut self) -> Result<()> {
        if !self.dirty {
            return Ok(());
        }
        self.dirty = false;
        // Check again on exit, including error exits. No writes to a replacement
        // paired to the same receiver. Timeout leaves restoration unconfirmed.
        let uid = self.ask(4, &[], 16, false, false)?;
        ensure!(
            uid == self.uid,
            "APEX identity changed before motor restore"
        );
        let first = self.io.write(&frame(0x53, &[1, 2, 0]));
        let second = self.io.write(&frame(0x53, &[1, 0x12, 0]));
        let mapping = self.motor_map(false)?;
        let mut error = first.err().or(second.err());
        for command in protocol::restore(&mapping)? {
            if let Err(e) = self.io.write(&command) {
                error.get_or_insert(e);
            }
        }
        error.map_or(Ok(()), Err)
    }
}
impl Drop for Apex {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::super::protocol::Layout;
    use super::*;
    use std::{collections::VecDeque, sync::Mutex};
    struct Mock {
        layout: Layout,
        writes: Arc<Mutex<Vec<[u8; 32]>>>,
        replies: VecDeque<Vec<u8>>,
        capable: bool,
        fail_enable: bool,
        replacement: bool,
    }
    impl Transport for Mock {
        fn layout(&self) -> &Layout {
            &self.layout
        }
        fn write(&mut self, packet: &[u8; 32]) -> Result<()> {
            self.writes.lock().unwrap().push(*packet);
            if self.fail_enable && packet[2] == 0x53 && packet[5..7] == [0x12, 2] {
                bail!("injected partial setup failure");
            }
            let cmd = packet[2];
            let mut p = match cmd {
                1 => {
                    let mut p = vec![0; 25];
                    p[0] = 150;
                    p[24] = if self.capable { 0x80 } else { 0 };
                    p
                }
                4 => vec![if self.replacement { 8 } else { 7 }; 16],
                0xa1 => {
                    let mut p = vec![0; 11];
                    p[0] = 1;
                    p
                }
                0xa3 => {
                    let index = packet[6];
                    let count = if index == 3 { 4 } else { 20 };
                    let mut p = vec![0, 0, 6, 1, 64, 0, index, count];
                    p.resize(8 + count as usize, 0);
                    if index == 0 {
                        p[8] = 2;
                    }
                    p
                }
                _ => return Ok(()),
            };
            let mut body = vec![0x5a, 0xa5, cmd];
            if cmd != 0xa3 {
                body.extend([1, 0]);
            }
            body.append(&mut p);
            body.push(body[2..].iter().fold(0u8, |s, b| s.wrapping_add(*b)));
            self.replies.push_back(body);
            Ok(())
        }
        fn read(&mut self, _: i32) -> Result<Vec<u8>> {
            Ok(self.replies.pop_front().unwrap_or_default())
        }
    }
    pub(crate) fn factory() -> (super::super::transport::Factory, Arc<Mutex<Vec<[u8; 32]>>>) {
        let writes = Arc::new(Mutex::new(Vec::new()));
        let recorded = writes.clone();
        (
            Box::new(move || {
                Ok(Box::new(Mock {
                    layout: Layout {
                        output_id: 0,
                        output_size: 32,
                        input_ids: vec![0],
                    },
                    writes: recorded,
                    replies: VecDeque::new(),
                    capable: true,
                    fail_enable: false,
                    replacement: false,
                }))
            }),
            writes,
        )
    }
    fn mock(capable: bool, fail_enable: bool) -> (Box<dyn Transport>, Arc<Mutex<Vec<[u8; 32]>>>) {
        let writes = Arc::new(Mutex::new(Vec::new()));
        (
            Box::new(Mock {
                layout: Layout {
                    output_id: 0,
                    output_size: 32,
                    input_ids: vec![0],
                },
                writes: writes.clone(),
                replies: VecDeque::new(),
                capable,
                fail_enable,
                replacement: false,
            }),
            writes,
        )
    }
    #[test]
    fn complete_session_disables_then_restores_all_four_motor_modes() {
        let (io, writes) = mock(true, false);
        let mut apex = Apex::new(io, Arc::new(AtomicBool::new(false)), None).unwrap();
        apex.write([[0.25, -0.25]; 8]).unwrap();
        apex.stop().unwrap();
        let w = writes.lock().unwrap();
        let modes: Vec<_> = w
            .iter()
            .filter(|p| p[2] == 0x53)
            .map(|p| p[4..7].to_vec())
            .collect();
        assert_eq!(
            modes,
            [
                vec![1, 2, 0],
                vec![1, 0x12, 2],
                vec![1, 2, 0],
                vec![1, 0x12, 0],
                vec![1, 0, 0],
                vec![1, 1, 0],
                vec![1, 0x10, 0],
                vec![1, 0x11, 0]
            ]
        );
        assert_eq!(w.iter().filter(|p| p[2] == 0x57).count(), 1);
    }
    #[test]
    fn partial_mode_setup_always_attempts_stop_and_restore() {
        let (io, writes) = mock(true, true);
        assert!(Apex::new(io, Arc::new(AtomicBool::new(false)), None).is_err());
        let w = writes.lock().unwrap();
        assert!(w.iter().any(|p| p[2] == 0x53 && p[4..7] == [1, 0x12, 0]));
        assert!(w.iter().any(|p| p[2] == 0x53 && p[4..7] == [1, 0x11, 0]));
    }
    #[test]
    fn unsupported_or_replaced_controller_never_enters_motor_mode() {
        for capable in [true, false] {
            let (io, writes) = mock(capable, false);
            assert!(Apex::new(io, Arc::new(AtomicBool::new(false)), Some(vec![9; 16])).is_err());
            assert!(writes
                .lock()
                .unwrap()
                .iter()
                .all(|p| p[2] != 0x53 && p[2] != 0x57));
        }
    }
}
