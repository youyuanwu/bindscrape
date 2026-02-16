//! Golden-file test: regenerate and verify the checked-in sources are up to date.

use std::path::{Path, PathBuf};

/// Recursively collect all file paths under `dir`, sorted, relative to `dir`.
fn collect_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_files_recursive(dir, dir, &mut files);
    files.sort();
    files
}

fn collect_files_recursive(base: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            collect_files_recursive(base, &path, out);
        } else {
            out.push(path.strip_prefix(base).unwrap().to_path_buf());
        }
    }
}

#[test]
fn generated_sources_are_up_to_date() {
    let workspace_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let checked_in = workspace_dir.join("bnd-posix/src/posix");

    // Generate into a temp directory structured like bnd-posix
    let tmp = tempfile::tempdir().unwrap();
    let tmp_src = tmp.path().join("src/posix");
    std::fs::create_dir_all(&tmp_src).unwrap();

    // Need a Cargo.toml stub for --package (it reads up to "# generated features")
    let stub_toml = "[package]\nname = \"tmp\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[dependencies]\nwindows-link = \"0.2\"\n\n[features]\nFoundation = []\n# generated features\n";
    std::fs::write(tmp.path().join("Cargo.toml"), stub_toml).unwrap();

    bnd_posix_gen::generate(tmp.path());

    let generated_dir = tmp.path().join("src/posix");

    // Collect files from both directories
    let checked_in_files = collect_files(&checked_in);
    let generated_files = collect_files(&generated_dir);

    assert_eq!(
        checked_in_files, generated_files,
        "File lists differ.\nChecked in: {checked_in_files:?}\nGenerated: {generated_files:?}"
    );

    // Compare each file's content
    let mut diffs = Vec::new();
    for rel_path in &checked_in_files {
        let expected = std::fs::read_to_string(checked_in.join(rel_path)).unwrap();
        let actual = std::fs::read_to_string(generated_dir.join(rel_path)).unwrap();
        if expected != actual {
            diffs.push(rel_path.display().to_string());
        }
    }

    assert!(
        diffs.is_empty(),
        "The following checked-in files are out of date. Run `cargo run -p bnd-posix-gen` \
         to regenerate:\n  {}",
        diffs.join("\n  ")
    );
}
