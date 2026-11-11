fn main() {
    println!("cargo:rustc-check-cfg=cfg(studio_headless)");
    println!("cargo:rustc-cfg=studio_headless");
    println!("cargo:rustc-check-cfg=cfg(feature, values(\"joycon2\"))");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        let lib_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../sdl3")
            .canonicalize()
            .expect("SDL3 import library");
        println!("cargo:rerun-if-changed={}", lib_dir.join("SDL3.lib").display());
        println!("cargo:rustc-link-search=native={}", lib_dir.display());
        println!("cargo:rustc-link-lib=delayimp");
        println!("cargo:rustc-link-arg=/DELAYLOAD:SDL3.dll");
    }
}
