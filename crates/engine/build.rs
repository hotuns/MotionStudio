fn main() {
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        // Load the explicitly selected, verified DLL before the first SDL call.
        println!("cargo:rustc-link-arg=/DELAYLOAD:SDL3.dll");
    }
}
