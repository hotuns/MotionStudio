//! A known game's radial right-stick travel → turn speed, both normalized to full scale.
use crate::bezier::{self, CurveModel};
use anyhow::{ensure, Result};
use flexinput_engine::eval::{jsm_aim::StickResponse, sample_curve};
use serde::{Deserialize, Serialize};

fn linear() -> f32 {
    1.
}

/// Travel, speed and the optional local exponent from this point to the next.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResponsePoint(pub f32, pub f32, #[serde(default = "linear")] pub f32);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RightStickCurve {
    pub enabled: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub linearize_stick: bool,
    pub points: Vec<ResponsePoint>,
    #[serde(skip_serializing_if = "CurveModel::is_points")]
    pub model: CurveModel,
    /// One anchor, then two handles and the next anchor for every cubic.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub bezier_points: Vec<[f32; 2]>,
}

impl Default for RightStickCurve {
    fn default() -> Self {
        Self {
            enabled: false,
            linearize_stick: false,
            points: vec![ResponsePoint(0., 0., 1.), ResponsePoint(1., 1., 1.)],
            model: CurveModel::Points,
            bezier_points: Vec::new(),
        }
    }
}

impl RightStickCurve {
    /// Build once for both the native rate mixer and ordinary virtual reports.
    pub(crate) fn configured(g: &crate::profile::Gyro) -> Option<Self> {
        let saved = g.right_stick_curve.as_ref()?;
        if saved.enabled {
            return Some(saved.clone());
        }
        if !saved.linearize_stick {
            return None;
        }
        let s = &g.jsm_response;
        let power = if s.right_unpower == 0. {
            1.
        } else {
            s.right_unpower
        };
        let mut points = Vec::new();
        if s.right_undeadzone_inner > 0. {
            points.push(ResponsePoint(0., 0., 1.));
        }
        points.push(ResponsePoint(s.right_undeadzone_inner, 0., power));
        points.push(ResponsePoint(1. - s.right_undeadzone_outer, 1., 1.));
        if 1. - s.right_undeadzone_outer < 1. {
            points.push(ResponsePoint(1., 1., 1.));
        }
        Some(Self {
            linearize_stick: true,
            points,
            ..Default::default()
        })
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (2..=256).contains(&self.points.len()),
            "分段曲线需包含 2 到 256 个点。"
        );
        let first = self.points[0];
        let last = self.points[self.points.len() - 1];
        ensure!(
            (first.0, first.1) == (0., 0.) && (last.0, last.1) == (1., 1.),
            "分段曲线的起点和终点必须分别是 (0%, 0%) 和 (100%, 100%)。"
        );
        for (i, &ResponsePoint(travel, speed, power)) in self.points.iter().enumerate() {
            ensure!(
                (0.0..=1.).contains(&travel) && (0.0..=1.).contains(&speed),
                "第 {} 行的推杆量和游戏转速必须在 0% 到 100% 之间。",
                i + 1
            );
            ensure!(
                power.is_finite() && power > 0. && power.recip().is_finite(),
                "第 {} 行的段指数必须是有限正数。",
                i + 1
            );
            if i > 0 {
                let prev = self.points[i - 1];
                ensure!(
                    travel > prev.0 && speed >= prev.1,
                    "第 {} 行的推杆量必须递增，游戏转速可以持平或增加。",
                    i + 1
                );
            }
        }
        ensure!(last.2 == 1., "终点没有下一段，段指数请留空或填 1。");
        self.model
            .validate(bezier::Domain::Game, &self.bezier_points)
    }

    fn sample(&self, input: f32, inverse: bool) -> f32 {
        if input <= 0. {
            return 0.;
        }
        let input = input.min(1.);
        if self.model == CurveModel::Bezier {
            return bezier::sample(&self.bezier_points, input, usize::from(inverse));
        }
        let axes = |p: &ResponsePoint| if inverse { (p.1, p.0) } else { (p.0, p.1) };
        // The first match chooses the start of a speed plateau at equality and
        // the interval after its far edge for a slightly higher request.
        let i = self.points.partition_point(|p| axes(p).0 < input);
        let Some(upper) = self.points.get(i) else {
            return 1.;
        };
        let (x1, y1) = axes(upper);
        if i == 0 || x1 == input {
            return y1;
        }
        let lower = &self.points[i - 1];
        let (x0, y0) = axes(lower);
        let mut t = (input - x0) / (x1 - x0);
        if lower.2 != 1. {
            t = t.powf(if inverse { lower.2.recip() } else { lower.2 });
        }
        sample_curve(&[[0., y0], [1., y1]], t, &[])
    }

    /// Include every knot, even when a short segment lies between two grid samples.
    pub fn preview(&self, target_percent: f32) -> Result<(Vec<[f32; 3]>, f32)> {
        self.validate()?;
        let mut graph = vec![[0.; 3]];
        if self.model == CurveModel::Bezier {
            graph = bezier::table(&self.bezier_points)
                .into_iter()
                .map(|[x, y]| [x * 100., x * 100., y * 100.])
                .collect();
            return Ok((graph, self.travel_at(target_percent / 100.) * 100.));
        }
        for segment in self.points.windows(2) {
            let from = segment[0];
            let to = segment[1];
            let steps = if from.2 == 1. || from.1 == to.1 {
                1
            } else {
                32
            };
            for step in 1..steps {
                let travel = from.0 + (to.0 - from.0) * step as f32 / steps as f32;
                graph.push([travel * 100., travel * 100., self.speed_at(travel) * 100.]);
            }
            graph.push([to.0 * 100., to.0 * 100., to.1 * 100.]);
        }
        Ok((graph, self.travel_at(target_percent / 100.) * 100.))
    }
}

impl StickResponse for RightStickCurve {
    fn speed_at(&self, travel: f32) -> f32 {
        self.sample(travel, false)
    }

    fn travel_at(&self, speed: f32) -> f32 {
        self.sample(speed, true)
    }
}
