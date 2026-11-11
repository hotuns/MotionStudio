# Third-party notices

Motion Studio is an independent application. The project contains adapted open-source components and unmodified optional driver installers. See the included license texts before redistribution.

| Component | Source / version | Use and license |
| --- | --- | --- |
| FlexInput | https://github.com/x-iso/FlexInput/tree/c69f53ad3bca4ed34ef5f2d63509addaa7c767be | Core, evaluation, JSM Config, SDL input and supporting parsers, selected node descriptors and native output devices. MIT, licenses/FlexInput-MIT.txt |
| 3D Controller Overlay | https://github.com/larfingshnew/3d-controller-overlay via FlexInput c69f53ad | Xbox One, Xbox 360, DualSense, DualShock 4 and Switch Pro model parts packed as GLB. MIT, licenses/3D-Controller-Overlay-MIT.txt |
| three.js | https://github.com/mrdoob/three.js/tree/r187 | WebView controller rendering and model import. MIT, licenses/Three-MIT.txt |
| HIDMaestro | https://github.com/hifihedgehog/HIDMaestro/releases/tag/v2.0.1 | Embedded unmodified x64 DLLs/INFs and model data via pinned FlexInput; driver 2.0.0.2201; MIT, licenses/HIDMaestro-MIT.txt. Machine-local signing occurs only on explicit installation. |
| SDL | https://github.com/libsdl-org/SDL/tree/829a65d769d935c4852f8159e964312c0957260a | Unmodified official SDL 3.4.18 Windows x64 DLL and MSVC import library; zlib, licenses/SDL.txt; provenance and retired Apex 6 Pro workaround in vendor/sdl3 |
| sdl3-rs / sdl3-sys | 0.20.0 / 0.7.1+SDL-3.4.16, pinned in Cargo.lock | Rust bindings to the single shared SDL 3.4.18 runtime; MIT / zlib, licenses/sdl3-rs-MIT.txt and licenses/sdl3-sys-zlib.txt. No second SDL source/static runtime is embedded. |
| hidapi | 2.6.7, pinned in Cargo.lock | Dependency of the vendored input providers; MIT, licenses/hidapi-MIT.txt |
| Rhai | https://github.com/rhaiscript/rhai, 1.27.0 pinned in Cargo.lock | Optional numeric motion-script runtime; MIT OR Apache-2.0, licenses/Rhai-MIT.txt and licenses/Rhai-Apache-2.0.txt. No file/module or input/output-device API is exposed to scripts. |
| JSM Studio codebase | https://github.com/hotuns/JSM_Studio | Adapted React workspace, visual styles and controller artwork, Windows driver inventory, HidHide management and installer handling. Original notice retained in licenses/Studio-origin.txt |
| HidHide | 1.5.230.0, https://github.com/nefarius/HidHide | Optional original driver installer; MIT notice in src-tauri/drivers |
| ViGEmBus | 1.22.0, https://github.com/nefarius/ViGEmBus | Optional original driver installer; BSD-3-Clause notice in src-tauri/drivers |

Cargo.lock and package-lock.json pin application dependencies including Tauri, React, Vite, serde, glam, Windows bindings and vigem-client. Those dependencies retain their respective upstream licenses. Motion Studio does not ship the FlexInput graphical editor or adopt its profile file format.

The software updater uses tauri-plugin-updater 2.14.1, rustls 0.23.46, ring 0.17.14, minisign-verify 0.3.0 and zip 4.6.1. Their original license files are retained under licenses/software-updater, including ring's BoringSSL and other third-party notices.

From 0.6.1, the pure curve, One Euro, decay and brake algorithms are compiled directly from the pinned FlexInput crates/engine/src/eval/modules/jsm/cc.rs (original SHA-256 77a0e23fe3be21d06b1a74880fbb409ecc9160fc32c1e0407137c8f5a0981a08), under the same MIT license. crates/engine/src/motion_curves.rs is a compatibility re-export. Motion Studio uses the pinned FlexInput JSM Config parser for import and the JSM analog state machine for two-stage triggers at runtime. FlexInput credits JoyShockMapper by Julian “Jibb” Smart and Nicolas Lessard for the configuration language and rules, and the JSM Custom Curve fork by evan1mclean for its gyro additions. Motion Studio's time-weighted trackball and bounded turn scheduling adapt the pinned aim.rs and rws.rs algorithms. HID mouse streaming and pacing follow keymouse_hm.rs and the upstream I/O loop and reuse the original HrWaiter. The vendor manifest records the two original reference files. Studio adds timestamped-sample, ordered transition and cancellation adapters.
