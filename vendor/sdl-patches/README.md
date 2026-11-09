# Motion Studio SDL modifications

The bundled SDL 3.4.18 is a modified build, not an unmodified upstream release.

apex6-pro-gyro.patch corrects only Flydigi protocol device ID 150 (APEX 6 Pro): new gyro (X, Y, Z) = (-old Z, -old X, -old Y). Accelerometer conversion and other device IDs are unchanged.

The original before/after measurements used APEX 6 Pro, VID/PID 37d7/2502, 2.4 GHz receiver, XInput mode, third-party takeover. All six gyro directions and accelerometer orientations were checked. Firmware versions were not recorded; wired/Bluetooth modes and other models were not tested. This is an axis correction, not a drift or sensitivity calibration.

Motion Studio includes that previously verified DLL and its source patch. The resource check pins the DLL SHA-256 to 911f4f6747d40e3070aa4b454b72d515a1de9310804d794c57bc3a84698f984c. It does not rebuild SDL or modify Steam. To rebuild: check out libsdl-org/SDL release-3.4.18, apply apex6-pro-gyro.patch, build the Windows x64 shared SDL target with CMake/MSVC, and repeat the axis reports before replacing the pinned artifact. SDL upgrades must review the patch rather than blindly apply it to a new version.

Motion Studio uses standard SDL local gyro axes. No per-game vendor axis workaround or JSM axis command is parsed. The runtime manifest records the DLL and engine hashes; the build script verifies the pinned DLL before bundling.

The SDL source retains its zlib notice. This local modification was generated with AI assistance and is not represented as accepted upstream code. The hardware issue was reported upstream separately.

From 0.7.0, FlexInput's SDL provider uses sdl3-rs 0.20.0 and sdl3-sys 0.7.1 as Rust bindings to this same shared DLL. The build does not enable their static/source SDL features. `SDL3.lib` is the x64 MSVC import library from the shared build; it contains import stubs rather than another SDL runtime, SHA-256 `06f305d73771f0212fc409d8b487e7a0aa72ae9af98a62faa6f3e6db02ab0978`. The engine delay-loads SDL3.dll and explicitly loads the selected absolute path first. The module stays pinned across SDL session teardown/reconnect. The build verifies delayed PE imports in addition to DLL/import-library hashes.
