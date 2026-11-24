//! Monotone-X cubic Bézier paths shared by the editor, preview and runtime.
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CurveModel {
    #[default]
    Points,
    Bezier,
}
impl CurveModel {
    pub fn is_points(&self) -> bool {
        *self == Self::Points
    }
    pub fn validate(self, domain: Domain, path: &[[f32; 2]]) -> Result<()> {
        if self == Self::Bezier || !path.is_empty() {
            domain.validate(path)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Domain {
    Game,
    Acceleration,
    Action,
    StickMouse,
}

impl Domain {
    fn max_points(self) -> usize {
        let anchors = if self == Self::Game { 256 } else { 32 };
        1 + 3 * (anchors - 1)
    }
    pub fn validate(self, points: &[[f32; 2]]) -> Result<()> {
        ensure!(
            (4..=self.max_points()).contains(&points.len()) && (points.len() - 1) % 3 == 0,
            "贝塞尔曲线至少需要两个锚点，每段需要两个手柄，锚点数不能超过当前曲线的上限。"
        );
        let (max_x, max_y) = if self == Self::Acceleration {
            (4000., 10.)
        } else {
            (1., 1.)
        };
        ensure!(
            points
                .iter()
                .all(|p| (0.0..=max_x).contains(&p[0]) && (0.0..=max_y).contains(&p[1])),
            "数值超出此轴范围。"
        );
        if matches!(self, Self::Game | Self::StickMouse) {
            ensure!(
                points.first() == Some(&[0., 0.]) && points.last() == Some(&[1., 1.]),
                "分段曲线的起点和终点必须分别是 (0%, 0%) 和 (100%, 100%)。"
            );
        }
        for s in points.windows(4).step_by(3) {
            ensure!(
                s[3][0] > s[0][0] && increasing(s, 0),
                "贝塞尔曲线的推杆量或输入值必须随横轴递增。"
            );
            if self == Self::Game {
                ensure!(
                    s[3][1] >= s[0][1] && increasing(s, 1),
                    "游戏转速可以持平或增加。"
                );
            }
        }
        Ok(())
    }
}

// The derivative is a quadratic in Bernstein form. This also accepts monotone
// paths whose inner handles cross, and their exact de Casteljau subdivisions.
fn increasing(s: &[[f32; 2]], axis: usize) -> bool {
    let a = f64::from(s[1][axis]) - f64::from(s[0][axis]);
    let b = f64::from(s[2][axis]) - f64::from(s[1][axis]);
    let c = f64::from(s[3][axis]) - f64::from(s[2][axis]);
    a >= 0. && c >= 0. && b >= -(a * c).sqrt() - (a + c) * f64::from(f32::EPSILON)
}

pub fn sample(points: &[[f32; 2]], value: f32, axis: usize) -> f32 {
    let last = points[points.len() - 1];
    if value <= points[0][axis] {
        return points[0][1 - axis];
    }
    if value > last[axis] {
        return last[1 - axis];
    }
    let end = bezier_upper(points, axis, value);
    if points[end][axis] == value {
        return points[end][1 - axis];
    }
    let s = &points[end - 3..=end];
    coordinate(s, 1 - axis, parameter_at(s, axis, value)) as f32
}

type Cubic = [[f64; 2]; 4];

fn segment(s: &[[f32; 2]]) -> Cubic {
    std::array::from_fn(|i| s[i].map(f64::from))
}

fn subdivide(s: Cubic, t: f64) -> (Cubic, Cubic) {
    let lerp = |a: [f64; 2], b: [f64; 2]| std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t);
    let a = lerp(s[0], s[1]);
    let b = lerp(s[1], s[2]);
    let c = lerp(s[2], s[3]);
    let d = lerp(a, b);
    let e = lerp(b, c);
    let knot = lerp(d, e);
    ([s[0], a, d, knot], [knot, e, c, s[3]])
}

// The residual from the line between the endpoints is itself a Bézier
// scalar. Its two inner controls bound the vertical error of the point table.
fn linear_error(s: &Cubic) -> f64 {
    let [a, b, c, d] = *s;
    let slope = (d[1] - a[1]) / (d[0] - a[0]);
    let error = |p: [f64; 2]| (p[1] - a[1] - (p[0] - a[0]) * slope).abs();
    error(b).max(error(c))
}

// FlexInput's vector-response node takes a table. Preserve every anchor,
// spend detail on curved spans, and bound the number of points on its hot path.
pub fn table(points: &[[f32; 2]]) -> Vec<[f32; 2]> {
    let with_error = |s: Cubic| (s, linear_error(&s));
    let mut spans: Vec<_> = points
        .windows(4)
        .step_by(3)
        .map(|s| with_error(segment(s)))
        .collect();
    let budget = 255.max(spans.len() * 4);
    while spans.len() < budget {
        let (i, &(s, error)) = spans
            .iter()
            .enumerate()
            .max_by(|(_, (_, a)), (_, (_, b))| a.total_cmp(b))
            .unwrap();
        if error <= 0.0001 {
            break;
        }
        let (left, right) = subdivide(s, 0.5);
        let x = left[3][0] as f32;
        if x <= s[0][0] as f32 || x >= s[3][0] as f32 {
            spans[i].1 = 0.;
            continue;
        }
        spans[i] = with_error(left);
        spans.insert(i + 1, with_error(right));
    }
    std::iter::once(points[0])
        .chain(spans.into_iter().map(|(s, _)| s[3].map(|v| v as f32)))
        .collect()
}

// Only anchors enter the binary search. At an exact plateau speed choose its first anchor.
fn bezier_upper(points: &[[f32; 2]], axis: usize, value: f32) -> usize {
    let mut lo = 0;
    let mut hi = (points.len() - 1) / 3;
    while lo < hi {
        let mid = (lo + hi) / 2;
        if points[mid * 3][axis] < value {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo * 3
}

fn coordinate(s: &[[f32; 2]], axis: usize, t: f64) -> f64 {
    let u = 1. - t;
    let a = f64::from(s[0][axis]);
    let b = f64::from(s[1][axis]);
    let c = f64::from(s[2][axis]);
    let d = f64::from(s[3][axis]);
    u * u * (u * a + 3. * t * b) + t * t * (3. * u * c + t * d)
}

// Solve in the monotone coordinate. Newton handles ordinary samples in a few
// iterations. Bracketing keeps flat tangents and very small f32 inputs valid.
fn parameter_at(s: &[[f32; 2]], axis: usize, value: f32) -> f64 {
    let value = f64::from(value);
    let [a, b, c, d] = std::array::from_fn(|i| f64::from(s[i][axis]));
    let (mut lo, mut hi) = (0., 1.);
    let mut t = (value - a) / (d - a);
    for _ in 0..160 {
        let sample = coordinate(s, axis, t);
        if sample == value {
            return t;
        }
        if sample < value {
            lo = t;
        } else {
            hi = t;
        }
        let u = 1. - t;
        let slope = 3. * (u * u * (b - a) + 2. * u * t * (c - b) + t * t * (d - c));
        let estimate = t - (sample - value) / slope;
        if estimate == t && slope > 0. {
            return t;
        }
        let next = if estimate > lo && estimate < hi {
            estimate
        } else {
            (lo + hi) * 0.5
        };
        if (next - t).abs() <= f64::EPSILON * t {
            return next;
        }
        t = next;
    }
    t
}

/// De Casteljau subdivision adds an editable knot without changing the response.
pub fn split(
    mut points: Vec<[f32; 2]>,
    travel: f32,
    domain: Domain,
) -> Result<(Vec<[f32; 2]>, usize)> {
    domain.validate(&points)?;
    let first = points[0];
    let last = points[points.len() - 1];
    let max_x = if domain == Domain::Acceleration {
        4000.
    } else {
        1.
    };
    ensure!((0.0..=max_x).contains(&travel), "数值超出此轴范围。");
    if travel < first[0] || travel > last[0] {
        ensure!(
            points.len() < domain.max_points(),
            "已达到当前曲线的锚点上限。"
        );
        // Extend an action/gain curve's constant tail without changing its response.
        let left = travel < first[0];
        let (a, b, y) = if left {
            (travel, first[0], first[1])
        } else {
            (last[0], travel, last[1])
        };
        let h1 = [a + (b - a) / 3., y];
        let h2 = [b - (b - a) / 3., y];
        let index = if left {
            points.splice(0..0, [[a, y], h1, h2]);
            0
        } else {
            points.extend([h1, h2, [b, y]]);
            points.len() - 1
        };
        domain.validate(&points)?;
        return Ok((points, index));
    }
    let end = bezier_upper(&points, 0, travel);
    if points[end][0] == travel {
        return Ok((points, end));
    }
    ensure!(
        points.len() < domain.max_points(),
        "已达到当前曲线的锚点上限。"
    );
    let s = &points[end - 3..=end];
    let t = parameter_at(s, 0, travel);
    let (left, right) = subdivide(segment(s), t);
    let knot = [f64::from(travel), left[3][1]];
    points.splice(
        end - 2..end,
        [left[1], left[2], knot, right[1], right[2]].map(|p| p.map(|v| v as f32)),
    );
    domain.validate(&points)?;
    Ok((points, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adding_a_knot_preserves_curvature_plateaus_and_the_inverse() {
        let game = vec![
            [0., 0.],
            [0.04, 0.],
            [0.1, 0.],
            [0.18, 0.],
            [0.5, 0.],
            [0.64, 1.],
            [0.85, 1.],
            [0.9, 1.],
            [0.96, 1.],
            [1., 1.],
        ];
        let (divided, i) = split(game.clone(), 0.42, Domain::Game).unwrap();
        assert_eq!(divided[i][0], 0.42);
        assert_eq!(sample(&divided, 0., 1), 0.);
        assert_eq!(sample(&divided, 1., 1), 0.85);
        for n in 0..=100 {
            let value = n as f32 / 100.;
            for axis in [0, 1] {
                assert!((sample(&divided, value, axis) - sample(&game, value, axis)).abs() < 1e-5);
            }
        }
    }

    #[test]
    fn gain_can_decrease_and_extending_a_flat_tail_preserves_gain() {
        let path = vec![[20., 2.], [40., 4.], [80., 0.], [120., 0.5]];
        let (extended, i) = split(path.clone(), 200., Domain::Acceleration).unwrap();
        assert_eq!(extended[i], [200., 0.5]);
        assert_eq!(sample(&extended, 0., 0), 2.);
        assert_eq!(sample(&extended, 400., 0), 0.5);
        assert!(Domain::Acceleration
            .validate(&[[0., 1.], [100., 1.], [150., 2.], [80., 2.]])
            .is_err());
        let stick = [[0., 0.], [1., 0.], [0., 1.], [1., 1.]];
        Domain::StickMouse.validate(&stick).unwrap();
        let sampled = table(&stick);
        assert!(sampled.len() <= 256);
        assert!(sampled.windows(2).all(|s| s[0][0] < s[1][0]));
        assert_eq!(sampled.last(), Some(&[1., 1.]));
    }
}
