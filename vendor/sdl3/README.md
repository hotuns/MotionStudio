# Official SDL runtime

Motion Studio bundles the Windows x64 SDL 3.5.0 development build from SDL's
[`main` commit `e21b424`](https://github.com/libsdl-org/SDL/commit/e21b424c76b3c27cf07b420cfd565292c876d554).
Both binaries come from the `SDL-VC-x64` artifact in
[SDL's Windows CI run `44651637592`](https://github.com/libsdl-org/SDL/actions/runs/44651637592).
The build includes the upstream GameSir Tarantula 8K HIDAPI driver and its
firmware compatibility check. The runtime is `src-tauri/resources/SDL3.dll`.
`SDL3.lib` in this directory supplies the matching x64 MSVC import stubs.

`manifest.json` records the official CI artifact, its GitHub SHA-256, the
SDK archive nested inside it, and the paths, sizes and SHA-256 hashes of the
two extracted files. For an entry in `files`, first extract the archive's
`package.entry` from the CI artifact, then extract the file's `entry` from that
SDK archive. The build verifies the installed repository resources against the
manifest. The two binaries are used as built by upstream.

FlexInput's SDL provider uses sdl3-rs 0.20.0 and sdl3-sys 0.7.1 as bindings to
this shared DLL. Static/source SDL features remain disabled. MSVC delayed
imports let the engine load the selected absolute DLL path first. The module
stays pinned across session teardown/reconnect, and builds inspect the PE
imports. Timestamped raw sensor events and the standard SDL-to-FlexInput
coordinate conversions remain in the upstream provider.

SDL's original zlib notice is preserved in `licenses/SDL.txt`. Its text matches
the license in the official CI package.

## Previous official pin

The previous release was SDL 3.4.18 at `829a65d769d935c4852f8159e964312c0957260a`.

| File | SHA-256 |
| --- | --- |
| `SDL3-3.4.18-win32-x64.zip` | `75c2c0fc74e7d1206aaedc22893a35887332f4f85f5c8b74fddc10d177985f2c` |
| `SDL3-devel-3.4.18-VC.zip` | `78d84602ae616cfe26b33a73b7d3b9b6b56e8a6650e53d92c9c6ca938f747acd` |
| `SDL3.dll` | `2b4b128d8d510cc2d442693cf848c050d700d43dc667afe9e5fd82b0570d667e` |
| `SDL3.lib` | `c60802d826fe9a724f824ecdeac1f422bdf53a53705ef32eb12dd73e5d566e2c` |

## Retired Apex 6 Pro workaround

On 2026-11-11, the maintainer reported that controller firmware had fixed the
Apex 6 Pro gyro-axis issue and requested removal of the workaround. Motion
Studio therefore replaced its custom SDL build with the official DLL and
removed `vendor/sdl-patches/apex6-pro-gyro.patch`. The exact firmware version
was not supplied. This records the reason for retirement; it does not claim a
new hardware validation.

The retired patch targeted only Flydigi protocol device ID 150 in
`src/joystick/hidapi/SDL_hidapi_flydigi.c::HIDAPI_DriverFlydigi_HandleStatePacketV2`.
It changed gyro `(X, Y, Z)` to `(-old Z, -old X, -old Y)` and left accelerometer
conversion and other device IDs alone. Its original measurements used VID/PID
`37d7/2502`, a 2.4 GHz receiver, XInput mode and third-party takeover; firmware
versions were not recorded. The old files remain available in Git history:

- Patched DLL SHA-256: `911f4f6747d40e3070aa4b454b72d515a1de9310804d794c57bc3a84698f984c`.
- Custom import-library SHA-256: `06f305d73771f0212fc409d8b487e7a0aa72ae9af98a62faa6f3e6db02ab0978`.

There is no firmware-dependent fallback or replacement device-specific axis
correction in the Studio engine. Ordinary user calibration remains available.
