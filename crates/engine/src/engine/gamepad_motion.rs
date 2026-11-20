use flexinput_core::imu::{BUS_ACCEL_G, BUS_GYRO_DPS};
use flexinput_engine::eval::jsm_aim::{Gravity, Gyro};
use glam::Vec3;
use std::{ffi::c_void, ptr::NonNull};

extern "C" {
    fn studio_motion_new() -> *mut c_void;
    fn studio_motion_delete(motion: *mut c_void);
    fn studio_motion_reset(motion: *mut c_void);
    fn studio_motion_sample(
        motion: *mut c_void,
        gyro: *const f32,
        accel: *const f32,
        seconds: f32,
        auto_calibrate: bool,
        rate: *mut f32,
        gravity: *mut f32,
    );
}

pub(super) struct GamepadMotion(NonNull<c_void>);

// The handle has one owner. Each operation requires exclusive access, and the
// upstream object has no thread-local state or device handles.
unsafe impl Send for GamepadMotion {}

impl Default for GamepadMotion {
    fn default() -> Self {
        Self(NonNull::new(unsafe { studio_motion_new() }).expect("allocate GamepadMotion"))
    }
}

impl Drop for GamepadMotion {
    fn drop(&mut self) {
        unsafe { studio_motion_delete(self.0.as_ptr()) }
    }
}

impl GamepadMotion {
    pub fn reset(&mut self) {
        unsafe { studio_motion_reset(self.0.as_ptr()) }
    }

    pub fn sample(
        &mut self,
        raw: Vec3,
        accel: Option<Vec3>,
        dt: f32,
        auto: bool,
    ) -> (Gyro, Gravity) {
        // Invert the SDL provider's two different bus transforms. Standalone
        // JSM feeds SDL X/Y/Z directly to ProcessMotion in degrees/s and g.
        let rate = (Vec3::new(raw.y, -raw.z, -raw.x) * BUS_GYRO_DPS).to_array();
        let a = accel.unwrap_or_default();
        let acceleration = (Vec3::new(-a.y, a.z, -a.x) * BUS_ACCEL_G).to_array();
        let mut corrected = [0.; 3];
        let mut gravity = [0.; 3];
        unsafe {
            studio_motion_sample(
                self.0.as_ptr(),
                rate.as_ptr(),
                acceleration.as_ptr(),
                dt,
                auto && accel.is_some(),
                corrected.as_mut_ptr(),
                gravity.as_mut_ptr(),
            );
        }
        // ProcessMotion skips an all-zero packet. Never replay its last rate on
        // a gyro-only device that has just stopped turning.
        if rate == [0.; 3] && acceleration == [0.; 3] {
            corrected = [0.; 3];
        }
        let [x, y, z] = corrected;
        let v = Vec3::from_array(gravity).normalize_or_zero();
        (
            Gyro {
                pitch: x,
                yaw: -y,
                roll: z,
            },
            Gravity {
                v,
                known: v != Vec3::ZERO,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gravity_follows_gyro_when_accelerometer_has_no_new_reading() {
        let mut motion = GamepadMotion::default();
        let flat = Vec3::Z / BUS_ACCEL_G;
        let (_, down) = motion.sample(Vec3::ZERO, Some(flat), 0.01, false);
        assert!(down.known && down.v.y < -0.99);
        let mut gravity = down;
        for _ in 0..100 {
            (_, gravity) = motion.sample(Vec3::Y * (90. / BUS_GYRO_DPS), None, 0.01, false);
        }
        assert!(gravity.v.z > 0.99 && gravity.v.y.abs() < 0.02);
        motion.reset();
        let (_, unknown) = motion.sample(Vec3::ZERO, None, 0.01, false);
        assert!(!unknown.known);
    }
}
