use std::path::Path;

/// Generate the bnd-openssl source tree at `output_dir`.
///
/// 1. Runs bnd-winmd on `openssl.toml` to produce a `.winmd`.
/// 2. Runs `windows-bindgen --package` to emit `src/openssl/*/mod.rs`.
///    Passes both the openssl and posix winmds so that cross-winmd type
///    references resolve correctly.  `--reference` suppresses codegen for
///    `posix.*` types; the generated code uses `bnd_posix::posix::…` paths.
/// 3. Saves the `.winmd` under `output_dir/winmd/`.
pub fn generate(output_dir: &Path) {
    let gen_dir = Path::new(env!("CARGO_MANIFEST_DIR"));

    // Step 1: Generate .winmd
    let winmd_dir = output_dir.join("winmd");
    std::fs::create_dir_all(&winmd_dir).expect("failed to create winmd directory");
    let openssl_winmd = winmd_dir.join("bnd-openssl.winmd");
    bnd_winmd::run(&gen_dir.join("openssl.toml"), Some(&openssl_winmd))
        .expect("bnd-winmd failed to generate winmd");

    // Step 2: Locate posix winmd (produced by bnd-posix-gen)
    let posix_winmd = gen_dir.join("../bnd-posix/winmd/bnd-posix.winmd");
    assert!(
        posix_winmd.exists(),
        "posix winmd not found at {}\n\
         Hint: run `cargo run -p bnd-posix-gen` first",
        posix_winmd.display()
    );

    // Step 3: Generate crate source tree via windows-bindgen package mode
    windows_bindgen::bindgen([
        "--in",
        openssl_winmd.to_str().unwrap(),
        "--in",
        posix_winmd.to_str().unwrap(),
        "--out",
        output_dir.to_str().unwrap(),
        "--filter",
        "openssl",
        "--reference",
        "bnd_posix,full,posix",
        "--sys",
        "--package",
        "--no-toml",
    ])
    .unwrap();
}
