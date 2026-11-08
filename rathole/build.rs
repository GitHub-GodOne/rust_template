fn main() {
    // The web host only needs stable build metadata. Avoid invoking libgit2 in
    // the embedded build script, which is unavailable in some Windows linker
    // environments.
    println!("cargo:rustc-env=VERGEN_BUILD_TIMESTAMP=embedded");
    println!("cargo:rustc-env=VERGEN_BUILD_SEMVER=0.5.0");
    println!(
        "cargo:rustc-env=VERGEN_CARGO_TARGET_TRIPLE={}",
        std::env::var("TARGET").unwrap_or_default()
    );
    println!(
        "cargo:rustc-env=VERGEN_CARGO_PROFILE={}",
        std::env::var("PROFILE").unwrap_or_default()
    );
    println!("cargo:rustc-env=VERGEN_CARGO_FEATURES=embedded");
}
