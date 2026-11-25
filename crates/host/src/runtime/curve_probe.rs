use super::Runtime;
use anyhow::{ensure, Context, Result};
use motion_core::{
    engine::{axis_to_i16, Frame, PadState},
    protocol::{CalibrationButton, CalibrationSessionId, CurveProbePhase, CurveProbeStatus},
};

const PHASE_TIMEOUT_NS: u64 = 60_000_000_000;

#[derive(Clone, Copy)]
pub(super) struct CurveProbe {
    pub(super) status: CurveProbeStatus,
    phase_since_ns: u64,
}

impl Runtime {
    pub(super) fn active_curve_probe(&self) -> Option<CurveProbe> {
        self.curve_probe.filter(|probe| {
            matches!(
                probe.status.phase,
                CurveProbePhase::WaitingForMark | CurveProbePhase::Timing
            )
        })
    }

    pub(super) fn start_curve_probe(
        &mut self,
        session_id: CalibrationSessionId,
        expected_revision: u64,
        percent: u8,
    ) -> Result<()> {
        ensure!(
            (1..=100).contains(&percent),
            "推杆量必须在 1% 到 100% 之间。"
        );
        ensure!(
            self.active_curve_probe().is_none(),
            "请先结束当前曲线测量。"
        );
        ensure!(
            self.calibration_controls
                .as_ref()
                .is_some_and(|controls| controls.session_id == session_id)
                && self.status.revision == expected_revision
                && self.status.state == "running"
                && self.status.emitting
                && !self.output_paused()
                && self.connected_input().is_some(),
            "校准所用的映射已改变，请重新打开校准悬浮窗。"
        );
        ensure!(
            self.active_layers.is_empty(),
            "请松开配置层按键，切回基础层后再校准。"
        );
        ensure!(
            !self.engine.as_ref().context("引擎未运行")?.is_measuring(),
            "请先结束或取消当前转角测量"
        );
        ensure!(
            self.gamepad_output_configured()
                && self
                    .output
                    .as_ref()
                    .and_then(|output| output.gamepad_status())
                    .is_some_and(|pad| pad.connected),
            "请先连接虚拟手柄并启用映射。"
        );
        self.clear_outputs_or_fault()?;
        if let Err(error) = self.emit_curve_frame(percent) {
            self.fault(format!("输出已暂停：{error:#}"));
            return Err(error);
        }
        self.curve_probe = Some(CurveProbe {
            status: CurveProbeStatus {
                session_id,
                percent,
                phase: CurveProbePhase::WaitingForMark,
            },
            phase_since_ns: self.sdl.as_ref().context("引擎未运行")?.now_ns(),
        });
        Ok(())
    }

    pub(super) fn emit_curve_frame(&mut self, percent: u8) -> Result<()> {
        let output = self.output.as_mut().context("引擎未运行")?;
        // Send through the configured virtual device after mapping/compensation.
        // All other axes, gyro and key/mouse outputs remain neutral for this pass.
        output.apply(&Frame {
            pad: Some(PadState {
                right_x: axis_to_i16(f32::from(percent) / 100.),
                ..Default::default()
            }),
            ..Default::default()
        })?;
        output.flush_output()
    }

    pub(super) fn handle_curve_button(
        &mut self,
        button: CalibrationButton,
        time_ns: u64,
    ) -> Result<bool> {
        let Some(probe) = self.active_curve_probe() else {
            return Ok(false);
        };
        // A press already in the SDL queue when the trial started is not a mark.
        if time_ns < probe.phase_since_ns {
            return Ok(true);
        }
        match button {
            CalibrationButton::Measure => {
                self.mark_curve_probe(probe.status.session_id, time_ns)?
            }
            CalibrationButton::Back => self.cancel_curve_probe(probe.status.session_id)?,
            CalibrationButton::Mode
                if probe.status.percent < 100
                    && probe.status.phase == CurveProbePhase::WaitingForMark =>
            {
                return Ok(false)
            }
            _ => return Ok(true),
        }
        self.publish_status();
        Ok(true)
    }

    pub(super) fn mark_curve_probe(
        &mut self,
        session_id: CalibrationSessionId,
        time_ns: u64,
    ) -> Result<()> {
        let Some(mut probe) = self
            .active_curve_probe()
            .filter(|p| p.status.session_id == session_id)
        else {
            return Ok(());
        };
        let Some(elapsed) = time_ns.checked_sub(probe.phase_since_ns) else {
            return Ok(());
        };
        if elapsed >= PHASE_TIMEOUT_NS {
            return self.finish_curve_probe(Some(CurveProbePhase::Interrupted));
        }
        if probe.status.phase == CurveProbePhase::WaitingForMark {
            probe.status.phase = CurveProbePhase::Timing;
            probe.phase_since_ns = time_ns;
            self.curve_probe = Some(probe);
            return Ok(());
        }
        if elapsed == 0 {
            return Ok(());
        }
        self.finish_curve_probe(Some(CurveProbePhase::Finished {
            seconds: elapsed as f64 / 1e9,
        }))
    }

    pub(super) fn expire_curve_probe(&mut self) -> Result<()> {
        if let Some(probe) = self.active_curve_probe() {
            let now = self.sdl.as_ref().context("引擎未运行")?.now_ns();
            if now.saturating_sub(probe.phase_since_ns) >= PHASE_TIMEOUT_NS {
                self.finish_curve_probe(Some(CurveProbePhase::Interrupted))?;
                self.publish_status();
            }
        }
        Ok(())
    }

    pub(super) fn cancel_curve_probe(&mut self, session_id: CalibrationSessionId) -> Result<()> {
        if self
            .curve_probe
            .is_some_and(|p| p.status.session_id == session_id)
        {
            if self.active_curve_probe().is_some() {
                return self.finish_curve_probe(None);
            }
            self.curve_probe = None;
        }
        Ok(())
    }

    pub(super) fn interrupt_curve_probe(&mut self) {
        if let Some(mut probe) = self.active_curve_probe() {
            probe.status.phase = CurveProbePhase::Interrupted;
            self.curve_probe = Some(probe);
        }
    }

    fn finish_curve_probe(&mut self, phase: Option<CurveProbePhase>) -> Result<()> {
        self.curve_probe = self.curve_probe.zip(phase).map(|(mut probe, phase)| {
            probe.status.phase = phase;
            probe
        });
        // Neutralize the virtual axis immediately and reseed physical controls
        // without replaying buttons or gyro motion from the isolated interval.
        if let Err(error) = self.begin_input_resync() {
            self.fault(format!("输出已暂停：{error:#}"));
            return Err(error);
        }
        Ok(())
    }
}
