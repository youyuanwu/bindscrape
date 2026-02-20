use std::path::Path;

/// Generate the bnd-openssl source tree at `output_dir`.
///
/// 1. Runs bnd-winmd on `openssl.toml` to produce a `.winmd`.
/// 2. Runs `windows-bindgen --package` to emit `src/openssl/*/mod.rs`.
/// 3. Deletes the intermediate `.winmd`.
pub fn generate(output_dir: &Path) {
    let gen_dir = Path::new(env!("CARGO_MANIFEST_DIR"));

    // Step 1: Generate .winmd
    let winmd_path = output_dir.join("bnd-openssl.winmd");
    bnd_winmd::run(&gen_dir.join("openssl.toml"), Some(&winmd_path))
        .expect("bnd-winmd failed to generate winmd");

    // Step 2: Generate crate source tree via windows-bindgen package mode
    windows_bindgen::bindgen([
        "--in",
        winmd_path.to_str().unwrap(),
        "--out",
        output_dir.to_str().unwrap(),
        "--filter",
        "openssl",
        "--sys",
        "--package",
    ])
    .unwrap();

    // Step 3: Clean up the intermediate winmd
    std::fs::remove_file(&winmd_path).ok();
}
