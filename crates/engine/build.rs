fn main() {
    println!("cargo:rerun-if-changed=src/engine/gamepad_motion.cpp");
    println!("cargo:rerun-if-changed=../../vendor/gamepadmotionhelpers/GamepadMotion.hpp");
    cc::Build::new()
        .cpp(true)
        .std("c++17")
        .include("../../vendor/gamepadmotionhelpers")
        .file("src/engine/gamepad_motion.cpp")
        .compile("studio_gamepad_motion");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        // Load the explicitly selected, verified DLL before the first SDL call.
        println!("cargo:rustc-link-arg=/DELAYLOAD:SDL3.dll");
    }
}
