//! Core generation logic for producing the `bnd-posix` crate source tree.

use std::path::Path;

/// Generate the bnd-posix source tree at `output_dir`.
///
/// 1. Runs bnd-winmd on `bnd-posix.toml` to produce a `.winmd`.
/// 2. Runs `windows-bindgen --package` to emit `src/posix/*/mod.rs`.
/// 3. Deletes the intermediate `.winmd`.
pub fn generate(output_dir: &Path) {
    let workspace_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let fixtures = workspace_dir.join("tests/fixtures/bnd-posix");

    // Step 1: Generate .winmd
    let winmd_path = output_dir.join("bnd-posix.winmd");
    bnd_winmd::run(&fixtures.join("bnd-posix.toml"), Some(&winmd_path))
        .expect("bnd-winmd failed to generate winmd");

    // Step 2: Generate crate source tree via windows-bindgen package mode
    windows_bindgen::bindgen([
        "--in",
        winmd_path.to_str().unwrap(),
        "--out",
        output_dir.to_str().unwrap(),
        "--filter",
        "posix",
        "--sys",
        "--package",
        "--no-toml",
    ])
    .unwrap();

    // Step 3: Clean up the intermediate winmd
    std::fs::remove_file(&winmd_path).ok();
}
