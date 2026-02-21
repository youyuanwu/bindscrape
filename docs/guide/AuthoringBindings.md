# Authoring Bindings with bnd-winmd

Generate Rust FFI bindings for any C library using `bnd-winmd` and
`windows-bindgen`. This guide is for **external users** who depend on
`bnd-winmd` as a library in their own project.

For adding a new bindings crate **inside this repo**, see
[ContributingBindings.md](ContributingBindings.md).

We'll use a hypothetical library called **zstd** as a running example.

---

## Overview

```
C headers ──→ bnd-winmd ──→ .winmd ──→ windows-bindgen ──→ Rust FFI module
```

You need two things:
1. A **TOML config** describing which headers to parse
2. A **`build.rs`** (or standalone script) that runs the pipeline

---

## Step 1: Add dependencies

```toml
[dependencies]
windows-link = "0.2"

[build-dependencies]
bnd-winmd = "0.1"
windows-bindgen = "0.66"
```

---

## Step 2: Write the TOML config

Create `bnd-winmd.toml` in your crate root:

```toml
# Optional: extra include search paths
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

| Field | Meaning |
|---|---|
| `name` | Assembly name — becomes the top-level Rust module name |
| `file` | Intermediate `.winmd` filename |
| `namespace` | WinMD namespace → Rust module path. Use dots for nesting (`zstd.dict`) |
| `library` | Shared library for `#[link(name = "...")]` |
| `headers` | Headers to `#include` (parsed by clang) |
| `traverse` | Headers whose declarations are **extracted**. Others provide types only |

### Multiple partitions

Split across headers or shared libraries with additional `[[partition]]` entries:

```toml
[[partition]]
namespace = "zstd.compress"
library = "zstd"
headers = ["zstd.h"]
traverse = ["zstd.h"]

[[partition]]
namespace = "zstd.dict"
library = "zstd"
headers = ["zstd.h", "zdict.h"]
traverse = ["zdict.h"]
```

### Cross-library type imports

If your library's headers reference types from another library that already
has a `bnd-*` crate (e.g. POSIX types), use `[[type_import]]` to import
those types instead of re-extracting them:

```toml
[[type_import]]
winmd = "path/to/bnd-posix.winmd"
namespace = "posix"
```

| Field | Meaning |
|---|---|
| `winmd` | Path to the external `.winmd` file (relative to the TOML file) |
| `namespace` | Root namespace filter — only types under this namespace are imported |

Imported types are emitted as cross-crate references in the generated
bindings (e.g. `bnd_posix::posix::…`). Pass `--reference <crate>` to
`windows-bindgen` for each external crate.

---

## Step 3: Generate bindings

### Option A: Flat mode (single output file)

In `build.rs`:

```rust
use std::path::Path;

fn main() {
    println!("cargo:rustc-link-lib=zstd");

    let winmd = bnd_winmd::run(Path::new("bnd-winmd.toml"), None).unwrap();
    windows_bindgen::bindgen([
        "--in",  winmd.to_str().unwrap(),
        "--out", "src/bindings.rs",
        "--filter", "zstd",
        "--flat",
        "--sys",
    ]).unwrap();
}
```

Then in `src/lib.rs`:

```rust
mod bindings;
pub use bindings::*;
```

### Option B: Package mode (feature-gated sub-modules)

For multi-partition configs, use `--package` to generate a module tree:

```rust
use std::path::Path;

fn main() {
    println!("cargo:rustc-link-lib=zstd");

    let winmd = bnd_winmd::run(Path::new("bnd-winmd.toml"), None).unwrap();
    windows_bindgen::bindgen([
        "--in",  winmd.to_str().unwrap(),
        "--out", env!("CARGO_MANIFEST_DIR"),
        "--filter", "zstd",
        "--sys",
        "--package",
    ]).unwrap();
}
```

This writes `src/zstd/*/mod.rs` and appends Cargo features. Your
`Cargo.toml` needs a marker:

```toml
[features]
Foundation = []
# generated features
```

And `src/lib.rs`:

```rust
pub mod zstd;
```

---

## Step 4: Use the bindings

```rust
use my_crate::zstd;

let v = unsafe { zstd::ZSTD_versionNumber() };
assert!(v > 0);
```

All function bindings are `unsafe` — they call directly into the C library.

---

## Traverse tips

- Start with just the main header in `traverse`.
- If bnd-winmd reports unresolved type references, add the header that
  defines each missing type to `traverse` (or add a `[[type_import]]` for
  types from an external library).
- Use `RUST_LOG=bnd_winmd=debug` to see what is extracted/skipped.

## Common issues

| Problem | Fix |
|---|---|
| "N unresolved type reference(s) found" | Add the header defining each type to `traverse`, or add a `[[type_import]]` for types from an external winmd |
| Variadic function warnings | Expected — variadic functions are auto-skipped |
| Struct with inline anonymous union | May need manual workaround |
| Wrong library linked | Check `library` in partition and `build.rs` link directives |

## Prerequisites

- **libclang** — `apt install libclang-dev` (or equivalent)
- The target C library's development headers installed
