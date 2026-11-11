# Official SDL runtime

Motion Studio bundles the unmodified Windows x64 SDL 3.4.18 release from
[libsdl-org/SDL](https://github.com/libsdl-org/SDL/releases/tag/release-3.4.18),
tag `release-3.4.18`, commit `829a65d769d935c4852f8159e964312c0957260a`.
The runtime is `src-tauri/resources/SDL3.dll`. `SDL3.lib` in this directory is
the matching official x64 MSVC import library, containing import stubs rather
than a second SDL runtime. The runtime archive and the VC development archive
contain identical x64 DLLs.

`manifest.json` records the official download URLs, archive sizes and SHA-256
digests supplied by the GitHub release, and the extracted DLL/import-library
paths, sizes and SHA-256 digests. The build verifies the extracted resources
against that manifest. To replace them, download the official assets, verify
the archive digests, extract exactly the recorded entries and review their
provenance before updating the manifest. No local SDL source patch is applied.

FlexInput's SDL provider uses sdl3-rs 0.20.0 and sdl3-sys 0.7.1 as bindings to
this shared DLL. Static/source SDL features remain disabled. MSVC delayed
imports let the engine load the selected absolute DLL path first. The module
stays pinned across session teardown/reconnect, and builds inspect the PE
imports. Timestamped raw sensor events and the standard SDL-to-FlexInput
coordinate conversions remain in the upstream provider.

SDL's original zlib notice is preserved in `licenses/SDL.txt`; its text matches
the official release license (the repository copy uses CRLF line endings).

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
