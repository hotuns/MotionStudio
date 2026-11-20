#include "GamepadMotion.hpp"
#include <new>

// A single controller's sensor state. The C++ object contains internal pointers
// to its settings, so keep it at a stable address for the lifetime of the handle.
extern "C" {
GamepadMotion* studio_motion_new() noexcept {
    return new (std::nothrow) GamepadMotion();
}

void studio_motion_delete(GamepadMotion* motion) noexcept {
    delete motion;
}

void studio_motion_reset(GamepadMotion* motion) noexcept {
    motion->Reset();
}

void studio_motion_sample(GamepadMotion* motion, const float* gyro, const float* accel,
                          float seconds, bool auto_calibrate, float* rate, float* gravity) noexcept {
    using GamepadMotionHelpers::CalibrationMode;
    motion->SetCalibrationMode(auto_calibrate ? CalibrationMode::Stillness | CalibrationMode::SensorFusion
                                             : CalibrationMode::Manual);
    if (auto_calibrate) {
        motion->Settings.StillnessGyroDelta = 1.2f;
        motion->Settings.StillnessAccelDelta = 0.015f;
    }
    motion->ProcessMotion(gyro[0], gyro[1], gyro[2], accel[0], accel[1], accel[2], seconds);
    motion->GetCalibratedGyro(rate[0], rate[1], rate[2]);
    motion->GetGravity(gravity[0], gravity[1], gravity[2]);
}
}
