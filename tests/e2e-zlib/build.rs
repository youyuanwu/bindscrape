fn main() {
    let manifest_dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let fixtures = manifest_dir.join("../../bindscrape/tests/fixtures/zlib");

    // Step 1: Generate winmd from the zlib config
    let winmd_path = out_dir.join("zlib.winmd");
    bindscrape::run(&fixtures.join("zlib.toml"), Some(&winmd_path)).expect("bindscrape failed");

    // Step 2: Generate Rust bindings (flat + sys for single partition)
    let bindings_path = manifest_dir.join("src/bindings.rs");
    windows_bindgen::bindgen([
        "--in",
        winmd_path.to_str().unwrap(),
        "--out",
        bindings_path.to_str().unwrap(),
        "--filter",
        "Zlib",
        "--flat",
        "--sys",
    ])
    .unwrap();

    // Step 3: Link system libz
    println!("cargo:rustc-link-lib=dylib=z");

    // Rerun if sources change
    println!("cargo:rerun-if-changed=../../bindscrape/tests/fixtures/zlib/");
    println!("cargo:rerun-if-changed=../../bindscrape/src/");
}
