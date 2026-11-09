fn main() {
    println!("cargo:rustc-check-cfg=cfg(studio_headless)");
    println!("cargo:rustc-cfg=studio_headless");
}
