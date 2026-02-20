# Design: Cross-WinMD Type References

> **Status: Implemented.** All phases completed. OpenSSL bindings now
> reference POSIX types from `bnd-posix` via cross-winmd TypeRefs.
> 236 tests passing, clippy clean.

## Problem

OpenSSL headers reference POSIX system types that are also defined in
`bnd-posix`. Today `openssl.toml` drags in glibc sub-headers
(`bits/types/struct_tm.h`, `bits/types/struct_FILE.h`) so that those types
exist locally inside the openssl winmd. This duplicates `struct tm`,
`_IO_FILE`, and their transitive dependencies (`__off_t`, `_IO_lock_t`, …)
across two crates.

Duplication means:
- **ABI mismatch risk** — the two copies are independent types in Rust;
  passing a `bnd_posix::posix::time::tm` to an `openssl::crypto` function
  that expects its own `tm` requires a transmute.
- **Binary bloat** — identical struct definitions emitted twice.
- **Maintenance burden** — traverse lists for system types must be kept in
  sync across every TOML that needs them.

The goal is to let `bnd-openssl` reference types from `bnd-posix` instead
of redefining them.

---

## How windows-bindgen Already Supports This

`windows-bindgen` has two relevant flags:

### `--in` (load metadata)

```
--in bnd-posix.winmd --in openssl.winmd
```

All `.winmd` files passed to `--in` are merged into a single
`Reader` — a flat `HashMap<namespace, HashMap<name, Vec<Type>>>`.
TypeRef resolution is purely by `(namespace, name)` lookup in this merged
map. The `AssemblyRef` table is written for ECMA-335 conformance but
**never read** during resolution.

This means a TypeRef in `openssl.winmd` pointing to `posix.time.tm` will
resolve successfully as long as `bnd-posix.winmd` is also passed via
`--in`.

### `--reference` (suppress codegen for external types)

```
--reference bnd_posix,full,posix
```

Format: `<crate>,<style>,<namespace-or-type>`

| Part | Meaning |
|---|---|
| `bnd_posix` | Rust crate name used in path prefixes (`bnd_posix::posix::time::tm`) |
| `full` | Keep the full namespace path as module segments |
| `posix` | Match all types under the `posix.*` namespace tree |

**Styles:**

| Style | Path for `posix.time.tm` |
|---|---|
| `flat` | `bnd_posix::tm` |
| `full` | `bnd_posix::posix::time::tm` |
| `skip-root` | `bnd_posix::time::tm` |

**Effect:** Types matching the reference pattern are used for dependency
resolution (understanding signatures, struct fields) but are **never
emitted** in the output. The generated code emits `use bnd_posix::...`
paths instead of local type definitions.

### Combined usage

```
--in bnd-posix.winmd       # metadata for resolution
--in openssl.winmd         # metadata to generate
--filter openssl           # only emit openssl.* types
--reference bnd_posix,full,posix  # posix.* types come from bnd_posix crate
```

windows-bindgen's `TypeMap::filter()` collects all types matching
`--filter`, walks their dependencies, and:
- Adds dependencies to codegen if they're not covered by a `--reference`
- Skips dependencies covered by a `--reference` — the generated code uses
  the external crate path

---

## What Needs to Change

### 1. bnd-winmd: emit TypeRefs for external types (no changes needed)

bnd-winmd already emits TypeRef rows for named types via
`ctype_to_wintype()`. When openssl's `OPENSSL_gmtime` takes a
`struct tm *` parameter:

- extract.rs: clang resolves `struct tm` → `CType::Named { name: "tm", .. }`
- emit.rs: `ctype_to_wintype` checks `TypeRegistry::contains("tm")`
  - If `"tm"` is in the registry → emit `Type::named(namespace, "tm")`
  - If not → fall back to resolved canonical type

The issue is that `struct tm` is only in the registry if the types
partition that traverses `bits/types/struct_tm.h` is part of *this* winmd.
When we remove the glibc traverse headers from openssl.toml, `tm` won't be
in the local registry and the emit will fall back to a primitive, losing
the struct information.

**Fix:** Pre-seed the `TypeRegistry` with types from the referenced winmd
via the `[[type_import]]` config. The existing `TypeImportConfig` stub in
config.rs (which has `assembly`/`version`/`types` fields) is replaced with
a simpler struct matching the TOML schema:

```toml
[[type_import]]
winmd = "../bnd-posix/winmd/bnd-posix.winmd"
namespace = "posix"
```

The `winmd` path is resolved relative to the TOML file's directory
(`base_dir`), using the same logic as `resolve_header`.

bnd-winmd reads the referenced winmd at extraction time, walks its TypeDef
table, and pre-registers every type found into the `TypeRegistry` with its
original namespace. When emit encounters `CType::Named { name: "tm" }`, it
finds `"tm"` → `"posix.time"` in the registry and emits:

```
TypeRef(namespace="posix.time", name="tm")
```

No local TypeDef is emitted — just a reference row. The assembly metadata
is valid ECMA-335: it points to a type defined in an external assembly.

### 2. bnd-openssl-gen: pass both winmds + `--reference`

```rust
pub fn generate(output_dir: &Path) {
    let gen_dir = Path::new(env!("CARGO_MANIFEST_DIR"));

    // Step 1: Generate openssl.winmd
    let winmd_dir = output_dir.join("winmd");
    std::fs::create_dir_all(&winmd_dir).expect("create winmd dir");
    let openssl_winmd = winmd_dir.join("bnd-openssl.winmd");
    bnd_winmd::run(&gen_dir.join("openssl.toml"), Some(&openssl_winmd))
        .expect("bnd-winmd failed");

    // Step 2: Locate posix winmd (produced by bnd-posix-gen)
    let posix_winmd = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../bnd-posix/winmd/bnd-posix.winmd");

    // Step 3: Generate Rust with cross-references
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
```

`--filter openssl` ensures only openssl types are generated.
`--reference bnd_posix,full,posix` tells bindgen that any `posix.*`
type comes from the `bnd_posix` crate with `full` path style, producing
paths like `bnd_posix::posix::time::tm`. The `full` style is required
because `bnd-posix` keeps the `posix` root module (`pub mod posix` in
lib.rs) — `skip-root` would generate `bnd_posix::time::tm` which doesn't
compile.

`--no-toml` is included because bnd-openssl already has its own
hand-authored `Cargo.toml`.

### 3. openssl.toml: remove system header traversal

Before:
```toml
[[partition]]
namespace = "openssl.crypto"
library = "crypto"
headers = ["openssl/crypto.h"]
traverse = ["openssl/crypto.h", "bits/types/struct_tm.h", "bits/types/struct_FILE.h"]
```

After:
```toml
[[partition]]
namespace = "openssl.crypto"
library = "crypto"
headers = ["openssl/crypto.h"]
traverse = ["openssl/crypto.h"]

[[type_import]]
winmd = "../bnd-posix/winmd/bnd-posix.winmd"
namespace = "posix"
```

The system types (`tm`, `_IO_FILE`, etc.) are no longer extracted locally —
they're referenced from the posix winmd via TypeRef rows.

### 4. bnd-openssl Cargo.toml: add dependency

```toml
[dependencies]
bnd-posix = { path = "../bnd-posix" }
windows-link.workspace = true
```

The generated code will contain `use bnd_posix::posix::time::tm;` (or
similar `super::` paths depending on `--package` mode), so the runtime
dependency is required.

Feature-gate the dependency to pull in only the needed modules:

```toml
bnd-posix = { path = "../bnd-posix", features = ["time", "stdio", "pthread", "types"] }
```

> **Note:** The initial design expected only `time` and `stdio`, but
> generated code also references `pthread` (for `CRYPTO_ONCE`,
> `CRYPTO_THREAD_ID`, `CRYPTO_THREAD_LOCAL`) and `types` (for `off_t`,
> `ssize_t`).

---

## Generated Output Example

**Before (local duplication):**
```rust
// openssl/crypto/mod.rs
windows_link::link!("crypto" "C" fn OPENSSL_gmtime(timer : *const i64, result : *mut tm) -> *mut tm);

pub struct tm {
    pub tm_sec: i32,
    pub tm_min: i32,
    // ... 9 fields duplicated from bnd-posix
}
```

**After (cross-reference):**
```rust
// openssl/crypto/mod.rs
#[cfg(feature = "types")]
windows_link::link!("crypto" "C" fn OPENSSL_gmtime(
    timer : *const i64,
    result : *mut bnd_posix::posix::time::tm
) -> *mut bnd_posix::posix::time::tm);

// No local `struct tm` — it lives in bnd_posix::posix::time
```

---

## Implementation Plan

All phases completed. See commits on the `dev` branch.

### Phase 1: TypeRegistry pre-seeding from external winmd

1. ✅ **Upgrade `windows-metadata` from 0.59 to 0.60** — the reader API
   renames `Index` → `TypeIndex` and `.all()` → `.types()`. Update the
   workspace `Cargo.toml` and the roundtrip tests. The 0.60 API adds
   `TypeIndex::contains()` which is useful for validation.

2. ✅ **Replace `TypeImportConfig` in config.rs** — the existing stub has
   `assembly`/`version`/`types` fields; replace with `winmd: PathBuf` +
   `namespace: String` to match the TOML schema.

3. ✅ **Add `type_import` processing to `lib.rs`** — after loading config,
   resolve each `[[type_import]]` winmd path (relative to `base_dir`),
   read its TypeDef table using `windows_metadata::reader`, and register
   each type in the `TypeRegistry` with its namespace. Pre-seeding
   happens *before* `build_type_registry` so imported types take priority.

4. ✅ **Emit TypeRef for imported types** — `ctype_to_wintype()` already does
   this when a name is in the registry. No emit.rs changes needed.

5. ✅ **Typedef dedup is a safety net** — `generate_from_config` already
   deduplicates typedefs by checking if the canonical namespace matches
   the local partition namespace. Pre-seeded types have their original
   namespace (e.g. `posix.time`), which never matches any openssl
   partition — so any accidentally-extracted local copies are
   automatically dropped. No dedup code changes needed.

### Phase 2: Gen crate changes

6. ✅ **Update `bnd-openssl-gen`** — pass both winmds to `windows_bindgen`,
   add `--reference bnd_posix,full,posix`.

7. ✅ **Update `openssl.toml`** — remove `bits/types/struct_tm.h` and
   `bits/types/struct_FILE.h` from traverse, add `[[type_import]]`.

8. ✅ **Add `bnd-posix` dependency** to `bnd-openssl/Cargo.toml` with
   feature gates: `features = ["time", "stdio", "pthread", "types"]`.

### Phase 3: Validation

9.  ✅ **Build ordering** — `bnd-posix-gen` must run before
    `bnd-openssl-gen` so the posix winmd exists. The gen crates run
    outside `cargo build` (manual `cargo run -p`), so add a clear error
    message when the referenced winmd file doesn't exist.

10. ✅ **Roundtrip test** — add `roundtrip_openssl.rs` assertion that
    `openssl.crypto` functions reference `posix.time.tm` as a TypeRef
    (not a local TypeDef).

11. ✅ **E2E test** — call `OPENSSL_gmtime` with a
    `bnd_posix::posix::time::tm` from the posix crate, verifying no
    transmute is needed.

---

## TypeRegistry Pre-seeding: Reading External WinMD

The `windows-metadata` reader module (available since bnd-winmd already
depends on `windows-metadata` for the writer) provides `File::new(bytes)`
and `TypeIndex` (0.60) for iterating TypeDef rows. This is the same API
used by the roundtrip tests.

```rust
use windows_metadata::reader::{File, TypeIndex};

fn seed_registry_from_winmd(registry: &mut TypeRegistry, winmd_path: &Path) {
    let bytes = std::fs::read(winmd_path)
        .unwrap_or_else(|e| panic!(
            "failed to read external winmd {}: {e}\n\
             Hint: run `cargo run -p bnd-posix-gen` first",
            winmd_path.display()
        ));
    let file = File::new(bytes).expect("parse external winmd");
    let index = TypeIndex::new(vec![file]);
    for td in index.types() {
        let ns = td.namespace();
        let name = td.name();
        if !ns.is_empty() && name != "<Module>" && name != "Apis" {
            registry.register(name, ns);
        }
    }
}
```

This avoids any manual type listing — the registry is populated
automatically from whatever the external winmd contains. The `Apis` class
(which holds functions and constants) is excluded since it's not a real
type.

> **Note:** Upgrading from 0.59 to 0.60 also requires updating the
> roundtrip tests: `Index` → `TypeIndex`, `.all()` → `.types()`.

---

## Dependency Graph

```
bnd-posix-gen
    │
    ▼
bnd-posix.winmd ──────────────┐
    │                         │
    ▼                         ▼
bnd-posix (crate)    bnd-openssl-gen
                         │    reads posix.winmd for
                         │    type_import + --reference
                         ▼
                    openssl.winmd
                         │
                         ▼
                    bnd-openssl (crate)
                         │
                         ▼
                    depends on bnd-posix (runtime)
```

---

## Scope of Shared Types

The openssl `crypto.h` partition currently traverses two glibc headers:

| System header | Types extracted | Already in bnd-posix |
|---|---|---|
| `bits/types/struct_tm.h` | `tm` (9 fields) | `posix.time` |
| `bits/types/struct_FILE.h` | `_IO_FILE` (30 fields), `_IO_lock_t` | `posix.stdio` |

Additional transitive types that may be pulled in:
- `__off_t`, `__off64_t` — from `_IO_FILE` fields → `posix.types`
- `__ssize_t` — from cookie callbacks → `posix.types`

After the cross-reference change, `openssl.toml` needs zero glibc traverse
headers. All system types flow through the posix winmd.

Future libraries (zlib, curl, etc.) that also reference `FILE*` or
`struct tm*` will follow the same pattern: import from `bnd-posix.winmd`,
never traverse glibc headers locally.

---

## Resolved Questions

### 1. `full` path style (not `skip-root`)

`bnd-posix` keeps the root `posix` module: `lib.rs` has `pub mod posix`,
and `--package` mode generates `src/posix/time/mod.rs` etc.  The crate
does not re-export modules at the crate root, so the correct path to
`tm` is `bnd_posix::posix::time::tm`.

This means `--reference bnd_posix,full,posix` is the correct flag.
`skip-root` would generate `bnd_posix::time::tm` which doesn't compile.

### 2. Build order enforcement

The gen crates run outside `cargo build` (manual `cargo run -p`), so there
is no automatic ordering. The implementation adds a clear panic message
when the referenced winmd doesn't exist, pointing users to run
`cargo run -p bnd-posix-gen` first.

### 3. Feature gating

`bnd-openssl` depends on `bnd-posix` with explicit features:
`features = ["time", "stdio", "pthread", "types"]`. This pulls in only
the modules whose types are actually referenced by openssl signatures.

The four modules cover all cross-referenced types:
- `time` — `struct tm` (used by `OPENSSL_gmtime`)
- `stdio` — `_IO_FILE` (used by `BIO_new_fp`, `ERR_print_errors_fp`, etc.)
- `pthread` — `pthread_once_t`, `pthread_t`, `pthread_key_t` (used by
  `CRYPTO_ONCE`, `CRYPTO_THREAD_ID`, `CRYPTO_THREAD_LOCAL`)
- `types` — `off_t`, `ssize_t` (used by BIO and other APIs)
