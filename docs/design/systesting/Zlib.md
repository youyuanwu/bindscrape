# System Header E2E Testing

## Goal

Validate bindscrape against **real system headers** — not hand-written
fixtures. This proves the pipeline handles production-grade C APIs with
macro-heavy declarations, transitive includes, platform typedefs, opaque
pointers, and typedef-to-typedef aliases.

First (and current) target: **zlib** (`<zlib.h>`).

---

## Why zlib

- Small, stable, well-known C API — hasn't changed materially in years
- Good mix: 3 structs, ~81 functions, ~29 `#define` constants, 13 typedefs
  (including 4 function-pointer delegates and typedef-to-typedef aliases)
- Exercises real-world patterns that simpler fixture headers don't cover:
  forward-declared opaque structs, system typedefs (`off_t`, `va_list`),
  struct definitions without matching `typedef struct X X;` patterns,
  `FAR`-expanded typedef chains
- Ships as a shared library on every Linux distro (`libz.so`)

---

## Architecture

### Two-Partition Design

The original single-partition approach (traverse only `zlib.h`) dropped
critical typedefs like `Bytef`, `uInt`, `uLong` that are declared in
`zconf.h`. Resolving typedefs to primitives was considered but rejected —
the requirement is that **typedefs remain as distinct named types** in the
generated bindings.

Solution: **two partitions** with cross-partition TypeRef resolution.

```
/usr/include/zconf.h  ──→  Zlib.Types partition  (13 typedefs, 2 constants)
                                  │
                                  ▼  TypeRef
/usr/include/zlib.h   ──→  Zlib partition         (3 structs, 81 functions,
                                                    9 typedefs, 27 constants)
                                  │
                            zlib.winmd
                                  │
                          windows-bindgen --sys --flat
                                  │
                                  ▼
                          e2e-zlib/src/bindings.rs  ──→  links libz.so
```

### Crate Layout

```
bindscrape/tests/
├── fixtures/zlib/
│   └── zlib.toml               ← two-partition config
├── roundtrip_zlib.rs           ← 6 winmd roundtrip tests (own LazyLock)

tests/e2e-zlib/
├── Cargo.toml
├── build.rs                    ← bindscrape + windows-bindgen + link libz
├── src/
│   ├── lib.rs                  ← mod bindings; pub use bindings::*;
│   └── bindings.rs             ← generated (216 lines)
└── tests/
    └── zlib_e2e.rs             ← 12 FFI integration tests
```

---

## Config

**`tests/fixtures/zlib/zlib.toml`**:
```toml
[output]
name = "Zlib"
file = "zlib.winmd"

# Partition 1: base typedefs from zconf.h
[[partition]]
namespace = "Zlib.Types"
library = "z"
headers = ["/usr/include/zconf.h"]
traverse = ["/usr/include/zconf.h"]

# Partition 2: zlib.h structs, functions, constants
[[partition]]
namespace = "Zlib"
library = "z"
headers = ["/usr/include/zlib.h"]
traverse = ["/usr/include/zlib.h"]
```

Key points:
- **Absolute paths** — `wrapper_header()` resolves relative to the TOML
  file's directory, so system paths must be absolute
- **`library = "z"`** — matches the shared library name (`libz.so`)
- **Two traverse scopes** — `zconf.h` typedefs go to `Zlib.Types`,
  `zlib.h` structs/functions go to `Zlib`
- **No `clang_args`** — clang finds `/usr/include` by default

---

## API Surface Captured

### Zlib.Types Partition (zconf.h) — 13 typedefs, 2 constants

| Typedef | Resolves to | Winmd type |
|---|---|---|
| `Byte` | `unsigned char` | `U8` |
| `Bytef` | `Byte` (typedef-to-typedef) | alias of `Byte` |
| `uInt` | `unsigned int` | `U32` |
| `uLong` | `unsigned long` | `U64` (Linux LP64) |
| `uIntf` | `uInt` (typedef-to-typedef) | alias of `uInt` |
| `uLongf` | `uLong` (typedef-to-typedef) | alias of `uLong` |
| `charf` | `char` | `I8` |
| `intf` | `int` | `I32` |
| `voidpc` | `const void *` | `*const c_void` |
| `voidpf` | `void *` | `*mut c_void` |
| `voidp` | `void *` | `*mut c_void` |
| `z_size_t` | `size_t` (typedef-to-typedef) | `U64` (Linux LP64) |
| `z_crc_t` | `unsigned int` | `U32` |

Constants: `MAX_MEM_LEVEL=9`, `MAX_WBITS=15`.

### Zlib Partition (zlib.h)

**Structs (3):**
- `z_stream_s` — 14 fields, 112 bytes. Includes function-pointer fields
  (`zalloc`, `zfree`), opaque `state` pointer, and cross-partition
  typedef references (`Bytef`, `uInt`, `uLong`, `voidpf`)
- `gz_header_s` — 13 fields, 80 bytes
- `gzFile_s` — 3 fields, 24 bytes (semi-opaque, defined late in zlib.h)

**Typedefs (9):**
- `z_stream`, `z_streamp` — struct wrapper + pointer
- `gz_header`, `gz_headerp` — struct wrapper + pointer
- `gzFile` — `struct gzFile_s *`
- `alloc_func`, `free_func`, `in_func`, `out_func` — function-pointer
  delegates

**Functions (81):** Full zlib API including `compress`, `uncompress`,
`deflate*`, `inflate*`, `gz*`, `crc32*`, `adler32*`.

**Constants (27):** `Z_OK`, `Z_STREAM_END`, `Z_DEFLATED`, `Z_FINISH`,
`Z_BEST_COMPRESSION`, etc. Negative constants (`Z_ERRNO=-1` etc.) are
NOT captured — sonar's `find_definitions` cannot evaluate `(-1)` syntax.

---

## Challenges Encountered & Solutions

### 1. Typedef-to-typedef aliases dropped by sonar

**Problem**: `sonar::find_typedefs` filters out any typedef whose
underlying type is "elaborated". On Linux, clang marks typedef-to-typedef
aliases like `typedef Byte Bytef` as `Elaborated(Byte)`, causing sonar
to drop `Bytef`, `uIntf`, `uLongf`, and `z_size_t`. These are genuine
aliases, not struct pass-throughs.

**Solution**: Replaced `sonar::find_typedefs` with custom typedef
discovery in `collect_typedefs()`. The helper iterates `TypedefDecl`
entities directly and only skips trivial struct pass-throughs
(`typedef struct foo foo;`) via `is_struct_passthrough()`. This correctly
extracts all 13 zconf.h typedefs (up from 9 with sonar).

### 2. Struct without matching typedef missed by sonar

**Problem**: `sonar::find_structs` discovers structs through `TypedefDecl`
entities (e.g., `typedef struct z_stream_s z_stream` → extracts
`z_stream_s`). But `gzFile_s` only has a pointer typedef
(`typedef struct gzFile_s *gzFile`), so sonar never finds it as a struct.
The struct IS fully defined (3 fields, 24 bytes) at line 1837 of zlib.h.

**Solution**: `collect_structs()` runs a supplemental pass after
`sonar::find_structs` that iterates `StructDecl` entities directly. It
catches any structs with full definitions (`entity.is_definition()`) that
sonar missed.

### 3. System typedefs (`off_t`, `int32_t`, `size_t`, etc.)

**Problem**: System typedefs like `off_t`, `int32_t`, `size_t` are not
extracted by any partition but appear in function signatures and struct
fields. Without handling, they become `CType::Named` references that
windows-bindgen can't resolve.

**Solution**: Extraction preserves all typedef names as
`CType::Named { name, resolved }`. The `resolved` field holds the
clang-resolved canonical type (via `get_canonical_type()`). At emit
time, `ctype_to_wintype()` checks `TypeRegistry::contains()` — if the
type is registered (user-extracted), it emits a TypeRef; if not, it
falls back to the `resolved` canonical primitive (e.g. `off_t` → `I64`,
`size_t` → `USize`).

This keeps extraction simple and eliminates the need for a hardcoded
system typedef table — clang resolves every typedef automatically.

### 4. `va_list` — compiler built-in type

**Problem**: `gzvprintf` takes a `va_list` parameter. `va_list` is a
compiler built-in (`__builtin_va_list`), not defined in any header we
traverse. Unlike `off_t`, it has no portable canonical primitive type.

**Solution**: `va_list` / `__builtin_va_list` / `__gnuc_va_list` are the
one special case handled directly in `map_clang_type()` during
extraction, mapped to `*mut c_void` (opaque pointer). This is the only
hardcoded typedef in the extraction layer.

### 5. Opaque / incomplete struct pointers

**Problem**: `z_stream.state` points to `struct internal_state` which is
forward-declared but never defined. Pointers to incomplete types must not
produce `CType::Named` for the struct (windows-bindgen would panic with
"type not found").

**Solution**: In the `TypeKind::Record` branch of `map_clang_type`, check
`ty.get_sizeof()`. If it returns `Err(Incomplete)`, map to `CType::Void`
so the pointer becomes `*mut c_void`.

### 6. Macro-defined functions (`deflateInit`, `inflateInit`)

These are `#define` macros that call `deflateInit_` / `inflateInit_`.
libclang sees the macro but `sonar::find_functions()` only finds the
`_`-suffixed actual functions. This is fine — the generated bindings
expose `deflateInit_` etc. and tests call them with the extra
version/sizeof args.

### 7. `FAR` / `ZEXTERN` / `ZEXPORT` macros

On Linux these expand to nothing. libclang resolves them before the AST
is visible. No special handling needed.

### 8. Negative constants (`Z_ERRNO = -1`)

`sonar::find_definitions` cannot evaluate `(-1)` expressions (only bare
integer and float literals). Constants like `Z_ERRNO`, `Z_STREAM_ERROR`,
`Z_DATA_ERROR`, `Z_MEM_ERROR`, `Z_BUF_ERROR`, `Z_VERSION_ERROR`, and
`Z_DEFAULT_COMPRESSION` are NOT captured. This is a known limitation.

### 9. Header path resolution

`PartitionConfig::wrapper_header()` resolves relative paths against the
TOML directory. For system headers like `/usr/include/zlib.h`, absolute
paths are required in the config.

### 10. `uLong` size — Linux LP64 ABI

`unsigned long` is 8 bytes on Linux x64 (LP64). The type mapping uses
`TypeKind::Long/ULong → I64/U64` to match the host platform. This
produces correct struct sizes and function signatures for Linux. If
cross-compilation to Windows is needed in the future, the mapping
would need to become platform-conditional (Windows LLP64: `long` = 32-bit).

---

## Test Results

### Roundtrip Tests — `bindscrape/tests/roundtrip_zlib.rs` ✅

Own `LazyLock<Vec<u8>>` — separate binary, no Clang singleton race.

| Test | Assertion | Status |
|---|---|---|
| `zlib_structs_present` | `z_stream_s`, `gz_header_s`, `Apis` exist in `Zlib` namespace | ✅ |
| `zlib_delegates_present` | `alloc_func`, `free_func`, `in_func`, `out_func` delegates exist | ✅ |
| `zlib_functions_present` | `Apis` has `compress`, `uncompress`, `crc32`, `adler32`, `deflateInit_`, etc. | ✅ |
| `zlib_constants_present` | `Z_OK=0`, `Z_STREAM_END`, `Z_DEFLATED=8`, etc. exist as `Apis` fields | ✅ |
| `zlib_z_stream_fields` | `z_stream_s` has 14 fields: `next_in`, `avail_in`, ..., `reserved` | ✅ |
| `zlib_pinvoke` | `compress` method has ImplMap with `import_scope.name() == "z"` | ✅ |

### E2E Tests — `tests/e2e-zlib/tests/zlib_e2e.rs` ✅

These exercise the generated Rust FFI bindings against the real `libz.so`.

| Test | What it does | Status |
|---|---|---|
| `zlib_version_returns_string` | `zlibVersion()` returns non-null string starting with "1." | ✅ |
| `z_ok_is_zero` | `Z_OK == 0` | ✅ |
| `z_stream_end_is_one` | `Z_STREAM_END == 1` | ✅ |
| `z_deflated_is_eight` | `Z_DEFLATED == 8` | ✅ |
| `max_wbits_is_fifteen` | `MAX_WBITS == 15` | ✅ |
| `crc32_known_value` | `crc32(0, "hello", 5) == 0x3610a686` | ✅ |
| `adler32_known_value` | `adler32(1, "hello", 5) == 0x062c0215` | ✅ |
| `compress_uncompress_roundtrip` | Compress → uncompress → assert equality | ✅ |
| `compress_bound_is_reasonable` | `compressBound(1000)` ∈ [1000, 2000) | ✅ |
| `z_stream_s_size` | `size_of::<z_stream_s>() > 0` | ✅ |
| `gz_header_s_size` | `size_of::<gz_header_s>() > 0` | ✅ |
| `gz_file_s_has_three_fields` | `gzFile_s` fields `have`, `next`, `pos` accessible | ✅ |

---

## Build.rs Design

```rust
fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let fixtures = manifest_dir.join("../../tests/fixtures/zlib");

    // Step 1: Generate winmd
    let winmd_path = out_dir.join("zlib.winmd");
    bindscrape::run(&fixtures.join("zlib.toml"), Some(&winmd_path))
        .expect("bindscrape failed");

    // Step 2: Generate bindings (flat + sys for multi-partition)
    let bindings_path = manifest_dir.join("src/bindings.rs");
    windows_bindgen::bindgen([
        "--in", winmd_path.to_str().unwrap(),
        "--out", bindings_path.to_str().unwrap(),
        "--filter", "Zlib",
        "--flat", "--sys",
    ]).unwrap();

    // Step 3: Link system libz
    println!("cargo:rustc-link-lib=dylib=z");
}
```

`--filter Zlib` matches both `Zlib` and `Zlib.Types` namespaces.
`--flat` avoids nested module paths. `--sys` uses `windows_link::link!`
macro-based FFI.

---

## Dependencies

- `libclang-dev` package — provides `libclang` for `clang-sys` and C
  header parsing
- `zlib1g-dev` package — provides `/usr/include/zlib.h`,
  `/usr/include/zconf.h`, and `/usr/lib/x86_64-linux-gnu/libz.so`
- CI: `apt-get install libclang-dev zlib1g-dev` (in
  `.github/workflows/CI.yml`)

---

## Implementation Steps

1. ✅ Create `tests/fixtures/zlib/zlib.toml` (two partitions)
2. ✅ Add zlib roundtrip tests in `roundtrip_zlib.rs`
3. ✅ Create `tests/e2e-zlib/` crate (Cargo.toml, build.rs, src/, tests/)
4. ✅ Fix `sonar::find_typedefs` — custom typedef discovery for typedef-to-typedef aliases
5. ✅ Fix supplemental struct discovery — catch `gzFile_s` missed by sonar
6. ✅ Fix system typedefs — clang canonical type resolution in `CType::Named { resolved }`
7. ✅ Fix incomplete record types — `internal_state` mapped to Void
8. ✅ Fix `emit_function` namespace — pass partition namespace, not empty string
9. ✅ Add `e2e-zlib` to workspace members
10. ✅ Refactor `extract_partition` — `collect_*` helpers, merge duplicate `extract_struct`
11. ✅ All 47 workspace tests passing, clippy clean

---

## Lessons Learned

1. **sonar is a leaky abstraction.** Its typedef/struct discovery is
   designed for Windows SDK headers with rigid `typedef struct X X;`
   patterns. Real-world C headers use typedef chains, pointer typedefs,
   and standalone `struct` definitions that sonar doesn't handle. Both
   `find_typedefs` and `find_structs` needed supplemental passes.

2. **Two-partition is the right model for split headers.** Many C
   libraries have a `types.h`/`conf.h` + `api.h` pattern. The
   TypeRegistry cross-partition TypeRef mechanism handles this cleanly.

3. **Absolute paths for system headers.** The config path resolution
   is relative to the TOML file, which is nested in the test fixtures
   directory. System headers need absolute paths.

4. **System typedef resolution belongs in emit, not extraction.**
   Extraction preserves all typedef names as `CType::Named { resolved }`
   where `resolved` holds the clang-resolved canonical primitive. The
   emit layer checks the TypeRegistry and falls back to the resolved
   type for unregistered names (`int32_t`, `off_t`, `size_t`, etc.).
   Only `va_list` (compiler built-in with no canonical primitive) is
   special-cased during extraction. No hardcoded typedef table needed —
   new system headers (sockets, threads, etc.) work automatically.
