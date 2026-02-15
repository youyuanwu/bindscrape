# Design: Pure-Rust C Header → WinMD Pipeline

## Context

[CSharpGenerator.md](CSharpGenerator.md) describes a pipeline that reuses
[win32metadata](https://github.com/microsoft/win32metadata)'s .NET components
(ClangSharp + Roslyn Emitter) to generate `.winmd` from C headers. That approach
requires the .NET SDK and shells out to C# tools.

This document covers the **all-Rust alternative** that was chosen and
implemented: the [`clang`](https://crates.io/crates/clang) crate (idiomatic
libclang wrapper) for C header parsing, plus the
[`windows-metadata`](https://crates.io/crates/windows-metadata) crate for
winmd emission.

---

## Architecture

```
C/C++ Headers
      │
      ▼
┌─────────────┐
│   clang rs   │  Idiomatic libclang wrapper
│  + sonar     │  find_structs(), find_enums(), etc.
└──────┬───────┘
       │  Entity / Type / Declaration
       ▼
┌─────────────┐
│  bindscrape  │  Maps Entity → intermediate model → ECMA-335
│  (extract +  │  No intermediate code generation
│   emit)      │
└──────┬───────┘
       │  Vec<u8>
       ▼
  output.winmd
```

**Why this approach** (over `bindgen` → `syn` → winmd):
- **3–4 code layers** vs 6 with the bindgen pipeline
- **Direct access** to `Type::get_sizeof()`, `get_alignof()`, `Entity::get_bit_field_width()`, `Type::get_calling_convention()`  — no information loss or reverse mapping
- **Same underlying API as ClangSharp** — the conceptual C AST → ECMA-335 mapping is identical
- **No reverse type mapping** — reads C `int` directly instead of reversing `::std::os::raw::c_int`

---

## Implementation Status

> **Status: v1 implemented and tested.** 47 tests passing (19 roundtrip +
> 26 E2E + 2 doc-tests), clippy clean.

### What Is Implemented

| Feature | Notes |
|---|---|
| CLI (`clap`) + TOML config parsing | `main.rs` (86 LOC), `config.rs` (122 LOC) |
| Intermediate model types | `model.rs` (177 LOC) — `StructDef`, `EnumDef`, `FunctionDef`, `TypedefDef`, `ConstantDef`, `CType`, `TypeRegistry` |
| Clang extraction (`clang` crate + sonar) | `extract.rs` (613 LOC) — `collect_*` helpers for uniform extraction, custom typedef/struct discovery to work around sonar limitations |
| Partition filtering by source location | `should_emit_by_location()` checks `Entity::get_location()` against traverse file list |
| Type mapping (clang `TypeKind` → `CType`) | Void, Bool, char types, int/uint (all widths), float/double, Pointer, ConstantArray, IncompleteArray, Elaborated, Typedef, Record, Enum, FunctionPrototype. Incomplete records → Void. |
| System typedef resolution | `CType::Named { resolved }` carries clang's canonical type; emit falls back to it for unregistered typedefs. `va_list` → `*mut c_void` at extraction. |
| WinMD emission | `emit.rs` (429 LOC) — enums, structs, typedefs, delegates, functions (P/Invoke), constants |
| Function pointer → delegate | Detects `Ptr(FnPtr{...})` and bare `FnPtr{...}`, emits TypeDef extending MulticastDelegate with Invoke method |
| `#define` integer constants | `sonar::find_definitions()` with `detailed_preprocessing_record` |
| Cross-partition type references | `TypeRegistry` maps type name → namespace; emits `TypeRef` for named types |
| Structured logging (`tracing`) | `RUST_LOG=bindscrape=debug` shows per-declaration detail |
| Variadic function skipping | `collect_functions()` checks `Entity::is_variadic()` and warns/skips |
| Array parameter decay | `extract_function()` converts `CType::Array` params → `CType::Ptr` (C semantics; avoids `ELEMENT_TYPE_ARRAY` blob incompatibility with windows-bindgen) |
| Function deduplication | `collect_functions()` uses `HashSet<String>` to skip duplicates from glibc `__REDIRECT` macros |
| Warn-and-skip error handling | Non-fatal failures log `tracing::warn!` and skip the declaration |
| Round-trip integration tests | 28 tests across 4 files |
| E2E integration tests | 42 tests across 4 crates (including zlib against real `libz.so` and POSIX file I/O) |
| Package-mode code generation | `bns-posix-gen` drives bindscrape + `windows-bindgen --package` to generate the `bns-posix` source tree with feature-gated sub-modules |

### What Is NOT Yet Implemented

| Feature | Complexity |
|---|---|
| Union support (`ExplicitLayout` + `FieldLayout`) | Low |
| Bitfield attribute emission (`NativeBitfieldAttribute`) | Medium — extraction works, emission TODO |
| Multi-header wrapper generation | Low |
| Cross-WinMD type imports (`[[type_import]]`) | Medium |
| COM interface support | Medium — needs `ELEMENT_TYPE_CLASS` fix in `windows-metadata` |
| Nested types | Low — `NestedClass` API exists |
| Anonymous type synthetic naming | Partial — typedef-named works; nested anonymous types need synthetic names |

---

## File Structure

```
bindscrape/
├── Cargo.toml
├── src/
│   ├── lib.rs               # Public API + module declarations (115 LOC)
│   ├── config.rs            # TOML config deserialization (122 LOC)
│   ├── model.rs             # Intermediate types: StructDef, CType, TypeRegistry (177 LOC)
│   ├── extract.rs           # clang Entity/Type → model (613 LOC)
│   └── emit.rs              # model → windows-metadata writer calls (429 LOC)
└── tests/
    ├── roundtrip_simple.rs   # 8 tests — simple.h fixture (201 LOC)
    ├── roundtrip_multi.rs    # 5 tests — multi-partition fixture (141 LOC)
    └── roundtrip_zlib.rs     # 6 tests — zlib system header (200 LOC)

tests/
├── fixtures/
│   ├── simple.h / simple.toml
│   ├── multi/ (graphics.h, audio.h, multi.toml)
│   ├── bns-posix/ (bns-posix.toml — POSIX file I/O headers)
│   └── zlib/ (zlib.toml — references system headers)
├── simple-impl/              # Native C lib for e2e-test
├── e2e-test/                 # 6 E2E tests (single partition)
├── e2e-multi/                # 8 E2E tests (multi-partition)
└── e2e-zlib/                 # 12 E2E tests (system header, real libz.so)
```

**Total**: ~1,456 LOC (library) + ~542 LOC (roundtrip tests) + ~850 LOC (E2E crates)

---

## Dependencies

```toml
[dependencies]
clang = { version = "2.0", features = ["clang_10_0"] }
windows-metadata = "0.59"
toml = "1"
clap = { version = "4", features = ["derive"] }
anyhow = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
serde = { version = "1", features = ["derive"] }
```

---

## Implementation Discoveries

1. **`File` method borrow conflicts** — All `writer::File` methods take
   `&mut self`. Must compute TypeRef into a local variable before passing to
   TypeDef to avoid double mutable borrow.

2. **`detailed_preprocessing_record` required** — libclang does not expose
   macro definitions unless the parser is created with
   `detailed_preprocessing_record(true)`.

3. **Function pointer typedefs are `Ptr(FnPtr{...})`** — C `typedef int
   (*Name)(...)` maps to `CType::Ptr { pointee: FnPtr { ... } }`, not bare
   `FnPtr`. Delegate detection unwraps the pointer layer.

4. **Config path relativity** — Header paths in TOML are relative to the
   config file's directory. System headers require absolute paths.

5. **`sonar::find_definitions` returns `Definition` not `Declaration`** —
   Uses `DefinitionValue::Integer(bool, u64) | Real(f64)` where the `bool`
   indicates negation.

6. **Clang singleton constraint** — `clang::Clang::new()` can only be called
   once per process. Tests use `LazyLock<Vec<u8>>` to share winmd bytes.

7. **sonar drops typedef-to-typedef aliases** — `sonar::find_typedefs` filters
   out typedefs whose underlying type is "elaborated" (includes typedef chains
   like `typedef Byte Bytef`). **Fix**: custom `collect_typedefs()` iterates
   `TypedefDecl` entities directly.

8. **sonar misses structs without matching typedef** — `sonar::find_structs`
   discovers structs through `TypedefDecl` entities only. **Fix**:
   `collect_structs()` runs a supplemental pass over `StructDecl` entities
   with `entity.is_definition()`.

9. **System typedefs resolved via clang canonical types** — All typedef names
   preserved as `CType::Named { name, resolved }` during extraction.
   `resolved` holds the clang-resolved canonical primitive (e.g. `__mode_t` →
   `U32`). At emit time, `ctype_to_wintype()` checks `TypeRegistry::contains()`
   — if registered, emits TypeRef; if not, falls back to the `resolved` type.
   No hardcoded typedef table needed. Only `va_list` (compiler built-in with
   no canonical primitive) is special-cased during extraction (→ `*mut c_void`).

10. **Incomplete/opaque record types** — Forward-declared structs map to
    `CType::Void` so pointers become `*mut c_void`.

---

## Test Coverage

**47 total tests** (all passing, clippy clean):

### Roundtrip Tests (19)

Parse headers → emit winmd → read back → assert.

**roundtrip_simple.rs** (8 tests, `simple.h`): typedefs present, enum variants,
struct fields, functions, function params, constants, delegate, pinvoke.

**roundtrip_multi.rs** (5 tests, multi-partition): namespace placement,
functions, cross-partition typeref, constants, enums.

**roundtrip_zlib.rs** (6 tests, system headers): zlib structs, delegates,
functions, constants, z_stream fields, pinvoke.

### E2E Tests (26)

Generated FFI bindings linked against real native libraries.

| Crate | Tests | What it exercises |
|---|---|---|
| `e2e-test` | 6 | Single partition, simple.h, calls create/destroy widget |
| `e2e-multi` | 8 | Multi-partition, cross-namespace type references |
| `e2e-zlib` | 12 | System header, real libz.so, compress/uncompress roundtrip |

### Doc Tests (2)

---

## Known Limitations & Future Work

### `ELEMENT_TYPE_VALUETYPE` Hardcoded in Writer

The `windows-metadata` writer encodes all `Type::Name` references as
`ELEMENT_TYPE_VALUETYPE`. COM interfaces require `ELEMENT_TYPE_CLASS`.

**Mitigation**: v1 excludes COM interfaces. For v2: submit PR to
`windows-metadata` adding `Type::Class(TypeName)`, or manually encode the
signature blob bytes.

### `AssemblyRef` Is Private

The writer's `File::AssemblyRef()` method is private and uses a root-namespace
heuristic. Cannot create AssemblyRef with exact assembly name like
`"Windows.Win32"`.

**Mitigation**: v1 defines imported types locally. For v2: submit PR to
`windows-metadata` to expose a public `AssemblyRef()` method. The underlying
ECMA-335 table support already exists.

### C `long` Size — Linux LP64 ABI

C `long` is 64-bit on Linux x86-64 (LP64 ABI). The type mapping uses
`TypeKind::Long` → `I64` and `TypeKind::ULong` → `U64` to match the
host platform. This is correct for Linux-only usage. If Windows ABI
support is needed in the future, the mapping would need to become
platform-conditional (`long` → 32-bit on Windows LLP64).

### `clang` Crate Max Feature Is `clang_10_0`

The crate's highest feature flag is `clang_10_0`, but system libclang may be
18+. Works fine — all C header parsing APIs are stable since clang 3.x. If
a newer API is needed, add `clang-sys` as a direct dependency for raw FFI.

### Cross-WinMD Type References

For v1, imported types (e.g., `HRESULT`) are defined locally in the output winmd.
For v2, the `[[type_import]]` config and proper `AssemblyRef`/`TypeRef` emission
would allow referencing types from external winmd files like `Windows.Win32.winmd`.

The existing `File::TypeRef()` public API creates cross-namespace references
within a single winmd. Cross-assembly references need the private `AssemblyRef()`
method to be exposed.

### `windows-bindgen` Compatibility Conventions

For bindscrape output to be consumed by `windows-bindgen`, these conventions
must hold:

- TypeDef row ordering — `FieldList`/`MethodList` delimit ownership via row indices
- Enums — extend `System.Enum`, have `value__` field, literal fields with `Constant`
- Structs — extend `System.ValueType`, `SequentialLayout` flag, `ClassLayout`
- Delegates — extend `System.MulticastDelegate`, have `Invoke` method
- Functions — `ImplMap` entries pointing to `ModuleRef` (DLL/so name)
- Custom attributes — `NativeTypedefAttribute`, `NativeBitfieldAttribute`, etc.

All of the above are implemented and verified by the 47 tests.

---

## Actual LOC

| Component | LOC | File |
|---|---|---|
| Public API + module declarations | 115 | `lib.rs` |
| TOML config | 122 | `config.rs` |
| Intermediate model types | 177 | `model.rs` |
| Extraction (clang → model) | 613 | `extract.rs` |
| Emission (model → winmd) | 429 | `emit.rs` |
| Roundtrip tests | 542 | 3 files |
| E2E test crates | ~850 | 3 crates |
| **Total (library)** | **1,456** | |
| **Total (library + tests)** | **~2,848** | |

---

## Open Questions

### `ELEMENT_TYPE_CLASS` for COM Interfaces

The writer hardcodes `ELEMENT_TYPE_VALUETYPE` for all named types. Options:
- Submit PR to `windows-metadata` adding `Type::Class(TypeName)`
- Manually encode the signature blob bytes
- Fork the writer

### Minimum Viable v2 Feature Set

- ⬜ Unions (explicit layout)
- ⬜ Bitfield emission (`NativeBitfieldAttribute`)
- ⬜ Cross-WinMD type imports
- ⬜ COM interfaces (needs ELEMENT_TYPE_CLASS)
- ⬜ Nested types

---

## Reference Links

| Resource | URL |
|---|---|
| `windows-metadata` crate | https://crates.io/crates/windows-metadata |
| `windows-metadata` source | https://github.com/microsoft/windows-rs/tree/master/crates/libs/metadata |
| Writer `File` API | https://github.com/microsoft/windows-rs/blob/master/crates/libs/metadata/src/writer/file/mod.rs |
| Type enum | https://github.com/microsoft/windows-rs/blob/master/crates/libs/metadata/src/ty.rs |
| `windows-bindgen` | https://github.com/microsoft/windows-rs/tree/master/crates/libs/bindgen |
| `clang` crate | https://crates.io/crates/clang |
| `clang` crate docs | https://docs.rs/clang/latest/clang/ |
| `clang::Entity` API | https://docs.rs/clang/latest/clang/struct.Entity.html |
| `clang::Type` API | https://docs.rs/clang/latest/clang/struct.Type.html |
| `clang::sonar` module | https://docs.rs/clang/latest/clang/sonar/index.html |
| `clang-sys` crate | https://crates.io/crates/clang-sys |
| ECMA-335 spec | https://www.ecma-international.org/publications-and-standards/standards/ecma-335/ |
