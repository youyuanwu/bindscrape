# Contributing a Bindings Crate to This Repo

How to add a new C-library bindings crate **inside this workspace**.
For standalone usage outside this repo, see
[AuthoringBindings.md](AuthoringBindings.md).

We'll use **zstd** as a running example.

---

## Overview

Each library follows a three-piece pattern:

```
tests/fixtures/zstd/zstd.toml    ← extraction config (what to parse)
bnd-zstd-gen/                    ← generator crate (runs the pipeline)
bnd-zstd/                        ← product crate (checked-in generated sources)
```

```
C headers ──→ bnd-winmd ──→ .winmd ──→ windows-bindgen --package ──→ bnd-zstd/src/
```

---

## Step 1: Write the TOML config

Create `tests/fixtures/zstd/zstd.toml`:

```toml
# Optional: extra include search paths for platform headers
# include_paths = ["/usr/include/x86_64-linux-gnu"]

[output]
name = "zstd"
file = "zstd.winmd"

[[partition]]
namespace = "zstd"
library = "zstd"
headers = ["zstd.h"]
traverse = ["zstd.h"]
```

See [AuthoringBindings.md § TOML config](AuthoringBindings.md#step-2-write-the-toml-config)
for field reference and multi-partition examples.

If your library depends on types from another `bnd-*` crate (e.g. POSIX
types), add a `[[type_import]]` section — see
[AuthoringBindings.md § Cross-library type imports](AuthoringBindings.md#cross-library-type-imports).

---

## Step 2: Create the product crate

Create `bnd-zstd/`:

**`bnd-zstd/Cargo.toml`**:

```toml
[package]
name = "bnd-zstd"
version = "0.1.0"
edition = "2024"

[dependencies]
windows-link = "0.2"

[features]
default = ["zstd"]
Foundation = []
# generated features
```

The `# generated features` marker must be the last line of `[features]` —
`windows-bindgen --package` appends feature entries after it.

**`bnd-zstd/src/lib.rs`**:

```rust
pub mod zstd;
```

The module name must match the `name` field in the TOML config.

**`bnd-zstd/build.rs`** (if the library needs link directives):

```rust
fn main() {
    println!("cargo:rustc-link-lib=zstd");
}
```

Add `bnd-zstd` to the workspace `Cargo.toml` `members` list.

---

## Step 3: Create the generator crate

Create `bnd-zstd-gen/`:

**`bnd-zstd-gen/Cargo.toml`**:

```toml
[package]
name = "bnd-zstd-gen"
version = "0.1.0"
edition = "2024"
publish = false

[dependencies]
bnd-winmd = { path = "../bnd-winmd" }
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
windows-bindgen = "0.66"

[dev-dependencies]
tempfile = "3"
```

**`bnd-zstd-gen/src/lib.rs`**:

```rust
use std::path::Path;

pub fn generate(output_dir: &Path) {
    let workspace_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let fixtures = workspace_dir.join("tests/fixtures/zstd");

    let winmd_path = output_dir.join("bnd-zstd.winmd");
    bnd_winmd::run(&fixtures.join("zstd.toml"), Some(&winmd_path))
        .expect("bnd-winmd failed");

    windows_bindgen::bindgen([
        "--in",  winmd_path.to_str().unwrap(),
        "--out", output_dir.to_str().unwrap(),
        "--filter", "zstd",
        "--sys",
        "--package",
    ]).unwrap();

    std::fs::remove_file(&winmd_path).ok();
}
```

The `--filter` value must match the `name` in the TOML config.

**`bnd-zstd-gen/src/main.rs`**:

```rust
use std::path::PathBuf;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let workspace_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let output_dir = workspace_dir.join("bnd-zstd");

    bnd_zstd_gen::generate(&output_dir);
    println!("Generated bnd-zstd crate at {}", output_dir.display());
}
```

Add `bnd-zstd-gen` to the workspace `Cargo.toml` `members` list.

---

## Step 4: Generate

```sh
cargo run -p bnd-zstd-gen
```

Use debug logging to troubleshoot:

```sh
RUST_LOG=bnd_winmd=debug cargo run -p bnd-zstd-gen
```

If bnd-winmd reports unresolved type references, add the header that
defines each missing type to the partition's `traverse` list, or add a
`[[type_import]]` for types from an external winmd.

Inspect `bnd-zstd/src/zstd/` — verify function signatures, struct layouts,
and constants.

---

## Step 5: Add an up-to-date test

Create `bnd-zstd-gen/tests/up_to_date.rs` to ensure checked-in sources
match the generator output. This catches stale bindings in CI.

```rust
use std::path::{Path, PathBuf};

fn collect_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_recursive(dir, dir, &mut files);
    files.sort();
    files
}

fn collect_recursive(base: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            collect_recursive(base, &path, out);
        } else {
            out.push(path.strip_prefix(base).unwrap().to_path_buf());
        }
    }
}

#[test]
fn generated_sources_are_up_to_date() {
    let workspace_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let checked_in = workspace_dir.join("bnd-zstd/src/zstd");

    let tmp = tempfile::tempdir().unwrap();
    let stub_toml = "[package]\nname = \"tmp\"\nversion = \"0.0.0\"\n\
        edition = \"2024\"\n\n[dependencies]\nwindows-link = \"0.2\"\n\n\
        [features]\nFoundation = []\n# generated features\n";
    std::fs::write(tmp.path().join("Cargo.toml"), stub_toml).unwrap();

    bnd_zstd_gen::generate(tmp.path());

    let generated_dir = tmp.path().join("src/zstd");
    let checked_in_files = collect_files(&checked_in);
    let generated_files = collect_files(&generated_dir);

    assert_eq!(checked_in_files, generated_files,
        "File lists differ");

    let mut diffs = Vec::new();
    for rel in &checked_in_files {
        let expected = std::fs::read_to_string(checked_in.join(rel)).unwrap();
        let actual = std::fs::read_to_string(generated_dir.join(rel)).unwrap();
        if expected != actual {
            diffs.push(rel.display().to_string());
        }
    }

    assert!(diffs.is_empty(),
        "Out of date — run `cargo run -p bnd-zstd-gen` to regenerate:\n  {}",
        diffs.join("\n  "));
}
```

---

## Step 6: Write E2E tests

Add tests in `bnd-zstd/tests/` that call the generated bindings against
the real shared library:

```rust
use bnd_zstd::zstd;

#[test]
fn version_returns_nonzero() {
    let v = unsafe { zstd::ZSTD_versionNumber() };
    assert!(v > 0);
}
```

---

## Reference implementations

- [`bnd-openssl`](../../bnd-openssl/) / [`bnd-openssl-gen`](../../bnd-openssl-gen/) — multi-partition, two shared libraries
- [`bnd-posix`](../../bnd-posix/) / [`bnd-posix-gen`](../../bnd-posix-gen/) — 15 partitions, single shared library
- [`tests/fixtures/simple/simple.toml`](../../tests/fixtures/simple/simple.toml) — minimal single-partition example
