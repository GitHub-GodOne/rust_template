fn main() {
    // include_dir! embeds these assets; Cargo must rebuild when they change.
    println!("cargo:rerun-if-changed=frontend/dist");
    println!("cargo:rerun-if-changed=src/fixtures");
    let manifest =
        std::env::var("CARGO_MANIFEST_DIR").expect("Cargo provides the manifest directory");
    assert!(
        std::path::Path::new(&manifest)
            .join("frontend/dist/index.html")
            .is_file(),
        "Build frontend assets before Rust: npm --prefix frontend run build"
    );
}
