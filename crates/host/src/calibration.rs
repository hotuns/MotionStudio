use anyhow::{bail, Result};
use motion_core::motion_config::SensorCalibration;
use std::time::{Duration, Instant};

pub struct Calibration {
    start: Instant,
    n: u64,
    sum: [f64; 3],
    squares: [f64; 3],
    accel_n: u64,
    accel_sum: [f64; 3],
    accel_squares: [f64; 3],
    face: Option<(usize, bool)>,
}
impl Calibration {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
            n: 0,
            sum: [0.0; 3],
            squares: [0.0; 3],
            accel_n: 0,
            accel_sum: [0.; 3],
            accel_squares: [0.; 3],
            face: None,
        }
    }
    pub fn face(axis: usize, positive: bool) -> Result<Self> {
        if axis > 2 {
            bail!("校准轴必须为 X、Y 或 Z");
        }
        let mut s = Self::new();
        s.face = Some((axis, positive));
        Ok(s)
    }
    pub fn sample_accel(&mut self, v: [f32; 3]) {
        if self.start.elapsed() < Duration::from_secs(2) || !v.iter().all(|x| x.is_finite()) {
            return;
        }
        self.accel_n += 1;
        for (i, v) in v.into_iter().enumerate() {
            self.accel_sum[i] += f64::from(v);
            self.accel_squares[i] += f64::from(v).powi(2);
        }
    }
    pub fn result_full(
        &self,
        previous: &SensorCalibration,
    ) -> Result<([f32; 3], SensorCalibration)> {
        let bias = self.result()?;
        let mut sensor = previous.clone();
        sensor.gyro_noise_dps = (0..3)
            .map(|i| {
                (self.squares[i] / self.n as f64 - (self.sum[i] / self.n as f64).powi(2)).max(0.)
            })
            .sum::<f64>()
            .sqrt() as f32
            * 3.
            * 180.
            / std::f32::consts::PI;
        if self.accel_n >= 100 {
            let mean = self.accel_sum.map(|v| (v / self.accel_n as f64) as f32);
            let variance = std::array::from_fn::<_, 3, _>(|i| {
                (self.accel_squares[i] / self.accel_n as f64 - f64::from(mean[i]).powi(2)).max(0.)
            });
            let noise = variance.into_iter().sum::<f64>().sqrt() as f32;
            if noise > 0.3 {
                bail!("加速度计检测到移动，请放稳后重试");
            }
            sensor.accel_noise_g = noise / 9.80665 * 3.;
            if let Some((axis, positive)) = self.face {
                let expected = if positive { 9.80665 } else { -9.80665 };
                if (mean[axis] - expected).abs() > 3.
                    || mean
                        .iter()
                        .enumerate()
                        .any(|(i, v)| i != axis && v.abs() > 3.)
                {
                    bail!("方向不符，请按提示将指定传感器轴朝上，再采集这一面");
                }
                sensor.accel_faces[axis][usize::from(positive)] = Some(mean[axis]);
                if let [Some(neg), Some(pos)] = sensor.accel_faces[axis] {
                    sensor.accel_offset[axis] = (neg + pos) / 2.;
                    sensor.accel_scale[axis] = 2. * 9.80665 / (pos - neg);
                }
            }
        } else if self.face.is_some() {
            bail!("加速度样本不足，无法记录这一面");
        }
        sensor.validate()?;
        Ok((bias, sensor))
    }
    pub fn sample(&mut self, value: [f32; 3]) {
        if self.start.elapsed() < Duration::from_secs(2) || !value.iter().all(|v| v.is_finite()) {
            return;
        }
        self.n += 1;
        for (i, v) in value.into_iter().enumerate() {
            self.sum[i] += f64::from(v);
            self.squares[i] += f64::from(v).powi(2);
        }
    }
    pub fn progress(&self) -> f32 {
        (self.start.elapsed().as_secs_f32() / 5.0).min(1.0)
    }
    pub fn done(&self) -> bool {
        self.start.elapsed() >= Duration::from_secs(5)
    }
    pub fn result(&self) -> Result<[f32; 3]> {
        calculate(self.n, self.sum, self.squares)
    }
}
fn calculate(n: u64, sum: [f64; 3], squares: [f64; 3]) -> Result<[f32; 3]> {
    if n < 100 {
        bail!("体感样本不足，请检查手柄连接后重试");
    }
    let mut bias = [0.0; 3];
    for i in 0..3 {
        let mean = sum[i] / n as f64;
        let deviation = (squares[i] / n as f64 - mean * mean).max(0.0).sqrt();
        if !mean.is_finite() || mean.abs() > 0.15 || deviation > 0.03 {
            bail!("手柄未保持静止，校准结果未保存");
        }
        bias[i] = mean as f32;
    }
    Ok(bias)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_motion_and_short_calibration() {
        assert!(calculate(99, [0.0; 3], [0.0; 3]).is_err());
        assert!(calculate(1000, [0.0; 3], [1.0; 3]).is_err());
        assert_eq!(calculate(1000, [10.0; 3], [0.1; 3]).unwrap(), [0.01; 3]);
    }
    #[test]
    fn six_faces_learn_each_axis_offset_and_scale_and_measure_sensor_noise() {
        let mut sensor = SensorCalibration::default();
        for axis in 0..3 {
            for positive in [false, true] {
                let mut c = Calibration::face(axis, positive).unwrap();
                c.start = Instant::now() - Duration::from_secs(6);
                for n in 0..200 {
                    c.sample([0.01, 0., 0.]);
                    let mut a = [0.; 3];
                    a[axis] = (if positive { 9.80665 } else { -9.80665 }) / 1.1 + 0.2;
                    a[(axis + 1) % 3] = if n % 2 == 0 { 0.001 } else { -0.001 };
                    c.sample_accel(a);
                }
                let (bias, next) = c.result_full(&sensor).unwrap();
                assert!((bias[0] - 0.01).abs() < 1e-6);
                sensor = next;
                assert!(sensor.accel_faces[axis][usize::from(positive)].is_some());
            }
            assert!((sensor.accel_offset[axis] - 0.2).abs() < 0.001);
            assert!((sensor.accel_scale[axis] - 1.1).abs() < 0.001);
        }
        assert!(sensor.accel_noise_g > 0.);
        sensor.validate().unwrap();
    }
    #[test]
    fn a_face_in_the_wrong_direction_is_rejected_without_changing_saved_calibration() {
        let mut c = Calibration::face(0, true).unwrap();
        c.start = Instant::now() - Duration::from_secs(6);
        for _ in 0..200 {
            c.sample([0.; 3]);
            c.sample_accel([0., 9.80665, 0.]);
        }
        assert!(c.result_full(&SensorCalibration::default()).is_err());
        assert!(Calibration::face(3, true).is_err());
    }
}
