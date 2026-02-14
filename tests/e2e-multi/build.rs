fn main() {
    let manifest_dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let fixtures = manifest_dir.join("../../bindscrape/tests/fixtures/multi");

    // Step 1: Generate winmd from the multi-partition config
    let winmd_path = out_dir.join("multi_test.winmd");
    bindscrape::run(&fixtures.join("multi.toml"), Some(&winmd_path)).expect("bindscrape failed");

    // Step 2: Generate Rust bindings with namespace modules (no --flat)
    let bindings_path = manifest_dir.join("src/bindings.rs");
    windows_bindgen::bindgen([
        "--in",
        winmd_path.to_str().unwrap(),
        "--out",
        bindings_path.to_str().unwrap(),
        "--filter",
        "MultiTest",
        "--sys",
    ])
    .unwrap();

    // Point the linker at the directory containing libsimple.so (built by
    // the simple-impl cdylib crate). Cargo places cdylib output in
    // target/<profile>/, which we can derive from OUT_DIR.
    let target_dir = out_dir
        .ancestors()
        .nth(3) // OUT_DIR -> build/<hash>/out -> build -> debug
        .expect("cannot derive target dir from OUT_DIR");
    println!("cargo:rustc-link-search=native={}", target_dir.display());
    println!("cargo:rustc-link-lib=dylib=simple");
    // Embed rpath so the test binary can find the .so at runtime
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}", target_dir.display());

    // Rerun if sources change
    println!("cargo:rerun-if-changed=../../bindscrape/tests/fixtures/multi/");
    println!("cargo:rerun-if-changed=../../bindscrape/src/");
}
