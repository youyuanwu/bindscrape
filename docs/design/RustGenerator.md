# Design: Pure-Rust C Header → WinMD Pipeline

## Context

[CSharpGenerator.md](CSharpGenerator.md) describes a pipeline that reuses
[win32metadata](https://github.com/microsoft/win32metadata)'s .NET components
(ClangSharp + Roslyn Emitter) to generate `.winmd` from C headers. That approach
requires the .NET SDK and shells out to C# tools.

This document evaluates an **all-Rust alternative** — how difficult it is, what
building blocks exist, and what options are available.

---

## Existing Rust Building Blocks

### 1. `windows-metadata` crate (winmd reader + writer)

**Crate**: [`windows-metadata`](https://crates.io/crates/windows-metadata) v0.59
(was `windows-ecma335` before rename)

**Source**: [`crates/libs/metadata/`](https://github.com/microsoft/windows-rs/tree/master/crates/libs/metadata)

This is a pure Rust, zero-dependency ECMA-335 library with **both reader and writer**
modules. It is the same library that
[`windows-bindgen`](https://github.com/microsoft/windows-rs/tree/master/crates/libs/bindgen)
uses to consume winmd files when generating Rust bindings.

#### Writer API

The writer lives in
[`src/writer/`](https://github.com/microsoft/windows-rs/tree/master/crates/libs/metadata/src/writer)
and provides a `File` struct that can be built up incrementally, then serialized to a
valid PE/COFF `.winmd` file via `into_stream() -> Vec<u8>`.

Key methods on [`writer::File`](https://github.com/microsoft/windows-rs/blob/master/crates/libs/metadata/src/writer/file/mod.rs):

```rust
File::new(name: &str) -> Self           // Creates minimal ECMA-335 skeleton
File::TypeDef(namespace, name, extends, flags) -> id::TypeDef
File::TypeRef(namespace, name) -> id::TypeRef
File::Field(name, ty, flags) -> id::Field
File::MethodDef(name, signature, flags, impl_flags) -> id::MethodDef
File::Param(name, sequence, flags) -> id::Param
File::ImplMap(method, flags, import_name, import_scope)  // P/Invoke
File::ClassLayout(parent, packing_size, class_size)
File::FieldLayout(field, offset)
File::NestedClass(inner, outer)
File::InterfaceImpl(class, interface) -> id::InterfaceImpl
File::Attribute(parent, ty, values)     // Custom attributes
File::Constant(parent, value)           // Literal enum members
File::GenericParam(name, owner, number, flags)
File::into_stream() -> Vec<u8>          // Serialize to PE bytes
```

The [`Type`](https://github.com/microsoft/windows-rs/blob/master/crates/libs/metadata/src/ty.rs)
enum represents ECMA-335 element types:

```rust
enum Type {
    Void, Bool, Char, I8, U8, I16, U16, I32, U32, I64, U64, F32, F64,
    ISize, USize, String, Object,
    Name(TypeName),             // Named type reference
    PtrMut(Box<Self>, usize),   // Mutable pointer with depth
    PtrConst(Box<Self>, usize), // Const pointer with depth
    Array(Box<Self>),           // SZARRAY
    ArrayFixed(Box<Self>, usize), // Fixed-size array
    RefMut(Box<Self>),          // ByRef
    RefConst(Box<Self>),        // ByRef + IsConst
    Generic(u16),               // Generic type parameter
    // ...
}
```

The writer handles all PE/COFF serialization internally
([`into_stream.rs`](https://github.com/microsoft/windows-rs/blob/master/crates/libs/metadata/src/writer/file/into_stream.rs)):
DOS header, PE header, CLR header, metadata streams (#~, #Strings, #GUID, #Blob),
table row encoding with correct coded index sizes. It produces the exact same
format as .NET's `ManagedPEBuilder`.

**This is the key enabler for a pure-Rust pipeline.** We don't need .NET's
`MetadataBuilder` — the Rust equivalent already exists and is maintained by
Microsoft.

#### Reader API

The reader in
[`src/reader/`](https://github.com/microsoft/windows-rs/tree/master/crates/libs/metadata/src/reader)
provides `TypeIndex` for querying types from existing `.winmd` files. Useful for
round-trip testing and for implementing `typeImports` (cross-winmd references).

### 2. `bindgen` — Rust C/C++ header parser

**Crate**: [`bindgen`](https://crates.io/crates/bindgen) v0.72
(~241M downloads, maintained by Rust project)

**Source**: [github.com/rust-lang/rust-bindgen](https://github.com/rust-lang/rust-bindgen)

`bindgen` wraps `libclang` to parse C/C++ headers and produce Rust FFI code.
It already extracts exactly what we need:

- Structs with layout (`#[repr(C)]`, field offsets, packing)
- Functions with signatures and calling conventions
- Enums with variants and underlying type
- Typedefs
- Constants (`const` values — not `#define`, same limitation as ClangSharp)
- Union types
- Nested types
- Bitfields
- Function pointers

It does **not** produce winmd — it produces `.rs` files. But its internal AST
([`ir` module](https://github.com/rust-lang/rust-bindgen/tree/main/bindgen/ir))
contains all the parsed type information before it's lowered to Rust code.

### 3. `clang` / `clang-sys` — Raw libclang bindings

**Crate**: [`clang`](https://crates.io/crates/clang) v2.0 (idiomatic wrapper) /
[`clang-sys`](https://crates.io/crates/clang-sys) (raw FFI)

Lower-level access to libclang's AST. Used internally by `bindgen`. You'd use
these directly only if `bindgen`'s IR is insufficient.

---

## Options

### Option A: Pure Rust — `bindgen` Rust output + `syn` → `windows-metadata` writer

**Approach**: Use `bindgen` as a library to parse C headers, generate its
standard Rust FFI output (the stable public contract), then parse that output
with [`syn`](https://crates.io/crates/syn) to extract full structural type
information. Write types into `windows_metadata::writer::File`.

> **Why not `bindgen` IR directly?**  bindgen's IR module
> ([`ir::context::BindgenContext`](https://github.com/rust-lang/rust-bindgen/blob/main/bindgen/ir/context.rs))
> is `pub(crate)` — it is not part of the public API and may change without
> notice. See [Q1](#q1-why-not-parsecallbacks-or-raw-ir-superseded) for the full analysis.

> **Why not `ParseCallbacks`?**  Verified against bindgen v0.72:
> [`DiscoveredItem`](https://docs.rs/bindgen/latest/bindgen/callbacks/enum.DiscoveredItem.html)
> only carries names (`original_name` / `final_name`), not field types, struct
> layout, function signatures, or calling conventions. `ParseCallbacks` is
> designed for naming/configuration control, not structural extraction.

bindgen's **generated Rust code** is its true stable contract. It contains all
the information we need in well-defined Rust syntax:

- `#[repr(C)]` struct definitions with typed fields
- `extern "C" { fn ... }` blocks with full signatures
- `#[repr(transparent)]` / `#[repr(u32)]` enums with variants
- `type Alias = Target;` typedefs
- `Option<unsafe extern "C" fn(...)>` function pointers

```
C/C++ Headers
      │
      ▼
┌─────────────┐
│   bindgen    │  libclang-based parser (Rust crate)
│   (as lib)   │  Produces: Rust FFI code (String / TokenStream)
└──────┬───────┘
       │  Rust source / TokenStream
       ▼
┌─────────────┐
│     syn      │  Parse Rust code into full AST
└──────┬───────┘
       │  syn::File (items: structs, fns, enums, typedefs)
       ▼
┌─────────────┐
│  bindscrape  │  Walks syn AST, maps to ECMA-335 types
│  (Rust)      │  Uses windows-metadata writer
└──────┬───────┘
       │  Vec<u8>
       ▼
  output.winmd
```

**Mapping from syn AST → winmd:**

| syn Item | bindgen Output Pattern | winmd Writer Call |
|---|---|---|
| `ItemStruct` with `#[repr(C)]` | `#[repr(C)] struct Foo { x: i32 }` | `file.TypeDef(ns, name, ValueType, SequentialLayout)` + `file.Field()` per field + `file.ClassLayout()` |
| `ForeignItemFn` in `extern "C"` | `extern "C" { fn Foo(x: i32) -> i32; }` | `file.TypeDef(ns, "Apis", ...)` + `file.MethodDef()` + `file.ImplMap()` |
| `ItemEnum` with `#[repr(u32)]` | `#[repr(u32)] enum E { A = 0, B = 1 }` | `file.TypeDef(ns, name, Enum, Sealed)` + `file.Field("value__", ...)` + `file.Constant()` per variant |
| `ItemType` | `type HANDLE = *mut c_void;` | `file.TypeDef()` wrapping target + `NativeTypedefAttribute` |
| `ItemStruct` with fn ptr field | `Option<unsafe extern "C" fn(...)>` | `file.TypeDef()` extending `MulticastDelegate` + `file.MethodDef("Invoke", sig)` |
| `ItemUnion` | `#[repr(C)] union U { x: i32, y: f32 }` | `file.TypeDef(ns, name, ValueType, ExplicitLayout)` + `file.FieldLayout(field, 0)` |
| `ItemConst` | `pub const X: u32 = 42;` | `file.Field()` with `Literal` flag + `file.Constant()` |
| `int_macro` callback | `#define FOO 42` (integer) | `file.Field()` + `file.Constant()` (via `ParseCallbacks`, not syn) |

**Difficulty**: ★★★☆☆ (Medium)

- `bindgen` Rust output is the stable public API — it is semver'd and tested
- `syn` is the de-facto Rust parser (2B+ downloads), handles all Rust syntax
- Bitfields are lowered to integer fields by bindgen — original bitfield
  info is lost in the Rust output (see [Blockers § Bitfield Info Loss](#4-bitfield-info-loss-resolved-by-option-b))
- Anonymous types get mangled names (e.g., `_bindgen_ty_1`) — resolvable via
  `ParseCallbacks::item_name()` or post-processing

**Pros**:
- Single toolchain (pure Rust)
- **Stable interface**: bindgen's Rust output is semver'd; syn parses any valid Rust
- `windows-metadata` writer handles all PE/COFF/ECMA-335 serialization
- Fast — no .NET startup cost, no subprocess spawning
- Cross-platform by default
- Can produce exactly what `windows-bindgen` expects (same crate ecosystem)
- `ParseCallbacks::int_macro()` still available for `#define` integer constants

**Cons**:
- Extra parse step (bindgen → Rust text → syn AST → winmd) adds indirection
- Need to replicate the C type → ECMA-335 mapping logic ourselves
- Must map Rust types back to C/ECMA-335 types (`::std::os::raw::c_int` → `I32`)
- Bitfield and calling convention info may need supplementary extraction
- No existing prior art for syn AST → winmd specifically

---

### Option B: Pure Rust — `clang` crate → `windows-metadata` writer

**Approach**: Use an idiomatic libclang wrapper to walk the C header AST
directly, extract type/function/enum information with full layout data, and
produce winmd. No intermediate Rust-code generation step.

Three sub-approaches were evaluated. **B1 (`clang` crate)** is recommended.

#### B1: `clang` crate (Recommended)

**Crate**: [`clang`](https://crates.io/crates/clang) v2.0 by KyleMayes
(same author as `clang-sys`). 4.1M downloads.
[Source](https://github.com/KyleMayes/clang-rs).

An idiomatic Rust wrapper around libclang with high-level types:

- **`Entity`** — AST node. Key methods:
  - `get_kind()` → `EntityKind` (StructDecl, FunctionDecl, EnumDecl, FieldDecl,
    TypedefDecl, UnionDecl, ParmDecl, etc.)
  - `get_name()` → `Option<String>`
  - `get_type()` → `Option<Type>` (the C type of this entity)
  - `get_children()` → `Vec<Entity>` (child nodes / fields)
  - `get_arguments()` → `Option<Vec<Entity>>` (function parameters)
  - `get_result_type()` → `Option<Type>` (return type)
  - `get_bit_field_width()` → `Option<usize>` (bitfield width in bits!)
  - `is_bit_field()` → `bool`
  - `get_enum_constant_value()` → `Option<(i64, u64)>`
  - `get_enum_underlying_type()` → `Option<Type>`
  - `get_typedef_underlying_type()` → `Option<Type>`
  - `get_offset_of_field()` → `Result<usize, OffsetofError>` (field offset in bits)
  - `get_location()` → `Option<SourceLocation>` (for partition filtering)
  - `evaluate()` → `Option<EvaluationResult>` (constant evaluation)
  - `is_anonymous()` → `bool`
  - `is_variadic()` → `bool`
  - `visit_children()` — recursive visitor with callback

- **`Type`** — C type with computed properties:
  - `get_kind()` → `TypeKind`
  - `get_sizeof()` → `Result<usize>` (size in **bytes**)
  - `get_alignof()` → `Result<usize>` (alignment in **bytes**)
  - `get_offsetof(field_name)` → `Result<usize>` (offset in **bits**)
  - `get_calling_convention()` → `Option<CallingConvention>`
  - `get_argument_types()` → `Option<Vec<Type>>` (function type params)
  - `get_result_type()` → `Option<Type>` (function return type)
  - `get_pointee_type()` → `Option<Type>` (dereference pointer)
  - `get_element_type()` → `Option<Type>` (array element type)
  - `get_size()` → `Option<usize>` (array length)
  - `get_canonical_type()` → `Type` (strip typedefs / sugar)
  - `get_fields()` → `Option<Vec<Entity>>` (record fields)
  - `get_declaration()` → `Option<Entity>` (entity that declared this type)
  - `is_const_qualified()` / `is_pod()` / `is_variadic()` → `bool`

- **`sonar` module** — convenience iterators over top-level declarations:
  - `find_structs(entities)` → `Structs` iterator
  - `find_enums(entities)` → `Enums` iterator
  - `find_functions(entities)` → `Functions` iterator
  - `find_typedefs(entities)` → `Typedefs` iterator
  - `find_unions(entities)` → `Unions` iterator
  - `find_definitions(entities)` → `Definitions` iterator (preprocessor `#define`)
  - Returns `Declaration { name: String, entity: Entity, source: Option<Entity> }`

- **`CallingConvention`** enum: `C`, `Cdecl`, `StdCall`, `FastCall`, `Win64`,
  `X86_64SysV`, etc.

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
│  bindscrape  │  Maps Entity → ECMA-335, writes winmd
│  (model +    │  No intermediate code generation
│   emit)      │
└──────┬───────┘
       │  Vec<u8>
       ▼
  output.winmd
```

**Why B1 is the best Option B approach:**

- **Exact data we need**: `Type::get_sizeof()`, `get_alignof()`, `get_offsetof()`,
  `Entity::get_bit_field_width()`, `get_calling_convention()` — all directly
  available without any reverse-mapping or supplementary passes
- **Sonar module**: `find_structs()`, `find_enums()`, `find_functions()` etc.
  give us pre-filtered iterators — no manual cursor-kind matching needed
- **Same underlying API as ClangSharp**: both wrap the same libclang C API, so
  the mapping from C AST → winmd is conceptually identical to what ClangSharp does
- **Pipeline is only 3 layers**: C headers → libclang Entity/Type → model → winmd
  (vs Option A's 6 layers)
- **Bitfields are not a problem**: `Entity::get_bit_field_width()` gives us
  the original bitfield width, `is_bit_field()` identifies bitfield fields.
  No information loss.
- **No reverse type mapping**: we get C types directly (int, unsigned long, etc.)
  instead of needing to reverse `::std::os::raw::c_int` back to `I32`
- **Partition filtering**: `Entity::get_location()` gives source file info for
  traverse-list filtering, just like ClangSharp's `--traverse`
- **Preprocessor definitions**: `sonar::find_definitions()` extracts simple
  `#define` values — plus `Entity::evaluate()` for integer constant evaluation

**Staleness concern**: Last real release was May 2022 (v2.0), max feature flag
is `clang_10_0`. Last commit was Apr 2024 (minor fix). However:
- The libclang C APIs for struct/enum/function parsing (everything we need)
  are stable since clang 3.x — the crate covers them fully
- Missing clang_11+ features are mostly C++20/Objective-C additions irrelevant
  to C header parsing
- The crate wraps `clang-sys`, so we can drop to raw FFI for any edge case
- If needed, forking and adding clang_18_0 support is mechanical (add feature
  gates for newer API functions)

**Difficulty**: ★★★☆☆ (Medium) — significantly less than originally estimated

Example — extracting a struct:
```rust
use clang::{Clang, Index, EntityKind};
use clang::sonar::find_structs;

let clang = Clang::new().unwrap();
let index = Index::new(&clang, false, false);
let tu = index.parser("mylib.h").parse().unwrap();

for decl in find_structs(tu.get_entity().get_children()) {
    let ty = decl.entity.get_type().unwrap();
    let size = ty.get_sizeof().unwrap();
    println!("struct {} (size: {} bytes)", decl.name, size);

    for field in decl.entity.get_children() {
        if field.get_kind() == EntityKind::FieldDecl {
            let name = field.get_name().unwrap();
            let field_ty = field.get_type().unwrap();
            let offset = field.get_offset_of_field().unwrap(); // bits

            if field.is_bit_field() {
                let width = field.get_bit_field_width().unwrap();
                println!("  bitfield {}: {} bits at offset {}", name, width, offset);
            } else {
                println!("  field {}: {} at offset {} bits", name,
                         field_ty.get_display_name(), offset);
            }
        }
    }
}
```

#### B2: `clang-sys` directly (Raw FFI)

**Crate**: [`clang-sys`](https://crates.io/crates/clang-sys) v1.8.1.
184M downloads. Supports clang_3_5 through clang_18_0.

Raw `unsafe` FFI bindings to libclang. All `CX*` functions: `clang_visitChildren()`,
`clang_getCursorKind()`, `clang_Type_getSizeOf()`, etc.

**Pros**: Maximum control, latest clang version support, exactly what bindgen
uses internally.
**Cons**: Extremely verbose, all `unsafe`, essentially reimplementing the `clang`
crate wrapper. ~3x the boilerplate of B1 for the same functionality.

**Verdict**: Only use if B1 proves insufficient (unlikely for C headers). Note
that B1 uses clang-sys underneath, so we can always add raw FFI calls selectively.

#### B3: `clang-ast` (dtolnay — JSON AST)

**Crate**: [`clang-ast`](https://crates.io/crates/clang-ast) v0.1.35 by dtolnay.
521K downloads. [Source](https://github.com/dtolnay/clang-ast).

Serde-based deserialization of `clang -Xclang -ast-dump=json` output. Elegant
generic `Node<T>` design where you define only the AST node kinds you care about.

**Critical limitation**: The JSON AST dump contains only the **syntactic** AST
tree — it does NOT include computed layout information (`sizeof`, `alignof`,
`offsetof`, bitfield resolved offsets). These are properties that libclang
**computes** at query time, not data stored in the AST. To get layout data,
you'd need a separate `clang -fdump-record-layouts-simple` pass and parse its
ad-hoc text output — defeating the elegance of the approach.

**Verdict**: Not viable for winmd generation. winmd requires exact struct sizes,
field offsets, and alignment values that are absent from the raw AST dump.

#### Option B Summary

| Sub-Option | API Quality | Layout Data | Bitfields | Maintenance | Verdict |
|---|---|---|---|---|---|
| **B1: `clang` crate** | Excellent (idiomatic) | `get_sizeof()`, `get_alignof()`, `get_offsetof()` | `get_bit_field_width()` | Stable but last release 2022 | **Recommended** |
| **B2: `clang-sys`** | Raw unsafe FFI | Same (manual) | Same (manual) | Active, clang_18_0 | Fallback |
| **B3: `clang-ast`** | Elegant serde | **Missing** | **Missing** | Active (dtolnay) | Not viable |

---

## Chosen: Option B (`clang` crate → `windows-metadata` writer)

### Why

1. **Shortest pipeline**: C headers → libclang → Entity/Type → model → winmd
   (3–4 code layers). Option A had 6 layers (C → libclang → bindgen IR → Rust
   text → syn AST → model → winmd) with a lossy reverse-mapping step.
2. **Direct access to all data**: `Type::get_sizeof()`, `get_alignof()`,
   `get_offsetof()`, `Entity::get_bit_field_width()`,
   `Type::get_calling_convention()` — no information loss, no reverse mapping.
3. **Mirrors ClangSharp exactly**: ClangSharp is the .NET wrapper for the same
   libclang C API. The `clang` crate wraps the same API in Rust. The conceptual
   mapping from C AST nodes → ECMA-335 metadata is identical.
4. **Bitfields are solved**: Option A lost bitfield info when bindgen lowered
   them to integer fields. Option B gives us `get_bit_field_width()` directly.
5. **No reverse type mapping**: Option A required mapping `::std::os::raw::c_int`
   back to `I32`. Option B reads C `int` directly from libclang.
6. **Sonar module**: `find_structs()`, `find_enums()`, `find_functions()` provide
   exactly the iterators we need for top-level declaration traversal.
7. **Drop-down escape hatch**: The `clang` crate wraps `clang-sys`. If any
   libclang feature is missing from the idiomatic wrapper, we can call
   `clang-sys` FFI directly for that one case.

### Why Not Option A

Option A (`bindgen` → Rust → `syn` → winmd) was originally chosen for using
`bindgen`'s battle-tested edge-case handling. However, three discoveries weakened
it:

1. **`ParseCallbacks` is names-only** — no structural data, so the "just use
   callbacks" shortcut was out
2. **Reverse type mapping** — every `c_int`, `c_long`, `c_void` in the Rust
   output must be mapped back to ECMA-335 types. This is a finite set but adds
   a fragile translation layer.
3. **Bitfields require supplementary clang-sys** — bindgen lowers bitfields to
   opaque integer fields, losing the original widths. Recovering them requires
   a clang-sys pass anyway, which means Option A was really "Option A + half of
   Option B".

Since Option B gives us everything directly and the pipeline is simpler, it's
the clear choice.

### Risk: Edge Cases Without `bindgen`

The main risk is that `bindgen` handles many libclang edge cases (anonymous
types, flexible array members, forward declarations, recursive types, etc.)
that we'd need to handle ourselves. Mitigations:

1. **Scope is narrower** — we target C headers (not C++), no templates, no
   methods, no inheritance. Most of bindgen's complexity is C++ support.
2. **`sonar` module handles common cases** — `find_structs()` / `find_functions()`
   already filter and resolve declarations properly
3. **We're producing winmd, not Rust code** — many edge cases in bindgen exist
   because Rust has ownership/lifetime semantics that C doesn't. winmd is a
   simpler target (it's metadata, not executable code).
4. **Incremental**: start with simple headers, add edge-case handling as found

### Alternative: Option A as Fallback

If the `clang` crate proves too thin for some specific edge case, we can:
- Drop to `clang-sys` raw FFI for that specific case (the `clang` crate exposes
  its inner `clang-sys` types)
- Or fall back to Option A for just that subset (the winmd writer side is
  identical either way)

---

## Implementation Status

> **Status: v1 implemented and tested.** All core modules are built, the CLI
> runs end-to-end, and 8 round-trip integration tests pass. The tool
> successfully parses C headers via libclang, extracts structs/enums/functions/
> typedefs/constants, and emits a valid `.winmd` file that can be read back
> with the `windows-metadata` reader.

### What Is Implemented

| Feature | Status | Notes |
|---|---|---|
| CLI (`clap`) + TOML config parsing | ✅ Done | `main.rs` (86 LOC), `config.rs` (103 LOC) |
| Intermediate model types | ✅ Done | `model.rs` (171 LOC) — `StructDef`, `EnumDef`, `FunctionDef`, `TypedefDef`, `ConstantDef`, `CType`, `TypeRegistry` |
| Clang extraction (`clang` crate + sonar) | ✅ Done | `extract.rs` (512 LOC) — structs, enums, functions, typedefs, `#define` constants |
| Partition filtering by source location | ✅ Done | `should_emit_by_location()` checks `Entity::get_location()` against traverse file list |
| Type mapping (clang `TypeKind` → `CType`) | ✅ Done | Handles Void, Bool, char types, int/uint (all widths), float/double, Pointer, ConstantArray, IncompleteArray, Elaborated, Typedef, Record, Enum, FunctionPrototype |
| Well-known C typedef resolution | ✅ Done | `int8_t`..`uint64_t`, `size_t`, `ssize_t`, `intptr_t`, `uintptr_t`, `ptrdiff_t` → primitives |
| WinMD emission | ✅ Done | `emit.rs` (378 LOC) — enums (System.Enum + value__ + Constant), structs (ValueType + SequentialLayout + ClassLayout), typedefs (ValueType wrapper or delegate), functions (P/Invoke + ImplMap), constants (static literal Field + Constant) |
| Function pointer → delegate | ✅ Done | Detects `Ptr(FnPtr{...})` and bare `FnPtr{...}`, emits TypeDef extending MulticastDelegate with Invoke method |
| `#define` integer constants | ✅ Done | `sonar::find_definitions()` with `detailed_preprocessing_record` enabled on the parser |
| Cross-partition type references | ✅ Done | `TypeRegistry` maps type name → namespace; `ctype_to_wintype()` emits `TypeRef` for named types |
| Structured logging (`tracing`) | ✅ Done | `RUST_LOG=bindscrape=debug` shows per-declaration extraction and emission |
| Warn-and-skip error handling | ✅ Done | Non-fatal failures log `tracing::warn!` and skip the declaration |
| Round-trip integration tests | ✅ Done | 8 tests in `tests/roundtrip.rs` (155 LOC) using `simple.h` fixture |

### What Is NOT Yet Implemented

| Feature | Status | Complexity |
|---|---|---|
| Union support (`ExplicitLayout` + `FieldLayout`) | ⬜ Not started | Low — same as structs but with `ExplicitLayout` flag and offset 0 for all fields |
| Bitfield attribute emission (`NativeBitfieldAttribute`) | ⬜ Not started | Medium — extraction works (`is_bit_field()` / `get_bit_field_width()`), emission TODO in `emit_struct` |
| Multi-header wrapper generation | ⬜ Not started | Low — generate a temp file with `#include` for each header |
| Cross-WinMD type imports (`[[type_import]]`) | ⬜ Not started | Medium — config parsing done, emission deferred (see [Cross-WinMD](#cross-winmd-type-references-typeimports)) |
| COM interface support | ⬜ Not started | Medium — needs `ELEMENT_TYPE_CLASS` fix in `windows-metadata` |
| Nested types | ⬜ Not started | Low — `NestedClass` writer API exists |
| Anonymous type synthetic naming | ⬜ Partial | Typedef-named anonymous structs work; nested anonymous unions/structs need synthetic names |

### Actual File Structure

```
bindscrape/
├── Cargo.toml
├── src/
│   ├── lib.rs               # Module declarations (9 LOC)
│   ├── main.rs              # CLI entry point: args, tracing, orchestration (86 LOC)
│   ├── config.rs            # TOML config deserialization (103 LOC)
│   ├── model.rs             # Intermediate types: StructDef, CType, etc. (171 LOC)
│   ├── extract.rs           # clang Entity/Type → model (512 LOC)
│   └── emit.rs              # model → windows-metadata writer calls (378 LOC)
└── tests/
    ├── roundtrip.rs          # 8 integration tests (155 LOC)
    └── fixtures/
        ├── simple.h          # Smoke test C header
        └── simple.toml       # Config for smoke test
```

**Total**: ~1,414 LOC (source + tests). The `constants.rs` module from the
original design was merged into `extract.rs` since `sonar::find_definitions()`
handled constant extraction cleanly within the partition extraction loop.

### Implementation Discoveries

Several issues were found and resolved during implementation:

1. **`File` method borrow conflicts** — All `writer::File` methods take
   `&mut self`. Calling `file.TypeRef()` inside `file.TypeDef()` arguments
   caused double mutable borrow errors. **Fix**: compute TypeRef into a local
   variable before passing to TypeDef. This is a pervasive pattern in `emit.rs`.

2. **`detailed_preprocessing_record` required for `#define`s** — libclang does
   not expose macro definitions in the AST unless the parser is created with
   `detailed_preprocessing_record(true)`. Without it,
   `sonar::find_definitions()` returns nothing.

3. **Function pointer typedefs are `Ptr(FnPtr{...})`** — C `typedef int
   (*Name)(...)` maps to `CType::Ptr { pointee: FnPtr { ... } }`, not a bare
   `FnPtr`. The delegate detection in `emit_typedef` was updated to unwrap
   the pointer layer.

4. **Config path relativity** — Header paths in TOML must be relative to the
   TOML file's directory (since `base_dir` = parent of the config file).

5. **`sonar::find_definitions` returns `Definition` not `Declaration`** — The
   sonar module uses a different type (`Definition { name, value, entity }` with
   `DefinitionValue::Integer(bool, u64) | Real(f64)`) for `#define` constants,
   not the same `Declaration` struct used for types. The `bool` in `Integer`
   indicates negation.

6. **Clang singleton constraint** — `clang::Clang::new()` can only be called
   once per process. Integration tests use `LazyLock<Vec<u8>>` to produce the
   winmd bytes once and share across all test functions.

### Test Coverage

All 8 round-trip tests parse `simple.h` → emit winmd → read back with
`windows-metadata::reader::Index` → assert:

| Test | Validates |
|---|---|
| `roundtrip_typedefs_present` | All 5 types exist: Color, Rect, Widget, CompareFunc, Apis |
| `roundtrip_enum_variants` | Color extends System.Enum, has `value__` + COLOR_RED/GREEN/BLUE |
| `roundtrip_struct_fields` | Rect has 4 fields: x, y, width, height |
| `roundtrip_functions` | Apis has 3 methods: create_widget, destroy_widget, widget_count |
| `roundtrip_function_params` | create_widget has ≥3 parameters |
| `roundtrip_constants` | Apis has MAX_WIDGETS (256), DEFAULT_WIDTH, DEFAULT_HEIGHT fields |
| `roundtrip_delegate` | CompareFunc extends MulticastDelegate, has Invoke method |
| `roundtrip_pinvoke` | create_widget has ImplMap pointing to "simple" module |

---

## Partition & Namespace Handling in Rust

### What Partitions Do in win32metadata

In the C# pipeline
([Background.md § Partitions](Background.md)),
each **partition** defines:

1. **A set of headers to include** (`main.cpp` — `#include` list for dependency
   resolution)
2. **A subset of headers to traverse** (`--traverse` in `settings.rsp` — only
   types declared in these files get emitted)
3. **A target namespace** (`--namespace` in `settings.rsp` — the ECMA-335
   namespace written into every TypeDef emitted from this partition, e.g.
   `Windows.Win32.System.Registry`)

Additionally,
[`requiredNamespacesForNames.rsp`](https://github.com/microsoft/win32metadata/blob/main/generation/WinSDK/requiredNamespacesForNames.rsp)
provides per-API namespace overrides for cases where a single header's types must
be split across multiple namespaces.

### C Has No Namespaces — We Assign Them

C has no namespace concept. The namespace in a winmd TypeDef is purely an
ECMA-335 metadata value that our **emitter** controls. Neither ClangSharp, nor
bindgen, nor libclang will produce a namespace from C code — it is always
provided externally via configuration.

In win32metadata, ClangSharp's `--namespace` flag just tells it what C# namespace
to wrap the generated classes in. Our Rust emitter does the equivalent by passing
the namespace string to every `writer::File::TypeDef(namespace, name, ...)` call.

### How bindgen Handles Selective Traversal (Partitions)

**`allowlist_file(regex)`** is bindgen's direct equivalent of ClangSharp's
`--traverse`. It restricts which items get emitted based on the file they are
declared in. Headers included for dependency resolution are parsed but their
types are not emitted unless they appear in an allowlisted file.

```rust
// Equivalent of ClangSharp settings.rsp:
//   --traverse
//   <IncludeRoot>/um/winreg.h
//   <IncludeRoot>/um/statehelpers.h
let bindings = bindgen::Builder::default()
    // Include all needed headers (like main.cpp)
    .header("wrapper.h")  // #includes winreg.h, statehelpers.h, and their deps
    // Only emit types from these specific files (like --traverse)
    .allowlist_file(".*/winreg\\.h")
    .allowlist_file(".*/statehelpers\\.h")
    // Don't transitively pull in types from non-allowlisted files
    .allowlist_recursively(false)
    .generate()
    .unwrap();
```

Other useful methods:

| bindgen Method | ClangSharp Equivalent | Purpose |
|---|---|---|
| `allowlist_file(regex)` | `--traverse <path>` | Only emit types from matching files |
| `allowlist_type(regex)` | `--include <type>` | Only emit specific types |
| `allowlist_function(regex)` | (no direct equiv) | Only emit specific functions |
| `blocklist_file(regex)` | `--exclude <path>` | Skip everything in matching files |
| `blocklist_type(regex)` | `--exclude <type>` | Skip specific types |
| `allowlist_recursively(false)` | (default behavior) | Don't pull transitive deps automatically |

**Key difference**: bindgen uses regex patterns against full file paths, while
ClangSharp uses literal paths. In practice this is fine — use `.*/<filename>`
patterns.

### How `clang` Crate Handles It (Option B)

With the `clang` crate, filter declarations by checking their source location:

```rust
use clang::{Entity, source::SourceLocation};

fn should_emit(entity: &Entity, traverse_files: &[PathBuf]) -> bool {
    let Some(location) = entity.get_location() else { return false };
    let file_location = location.get_file_location();
    let Some(file) = file_location.file else { return false };
    let file_path = file.get_path();

    traverse_files.iter().any(|tf| file_path.ends_with(tf))
}
```

Or use the `sonar` module's pre-filtered iterators combined with a location check:

```rust
use clang::sonar::{find_structs, find_functions, find_enums};

let entities = tu.get_entity().get_children();

// sonar gives us well-formed declarations; we filter by source file
for decl in find_structs(&entities) {
    if should_emit(&decl.entity, &partition.traverse) {
        emit_struct(&mut file, &partition.namespace, &decl);
    }
}
```

This is the most direct analogue to what ClangSharp does internally — it parses
all included headers into one translation unit but only emits declarations from
the traverse list.

### Proposed Config for bindscrape

The partition/namespace mapping lives in `bindscrape.toml`:

```toml
[output]
name = "MyLib"               # Winmd assembly name
file = "MyLib.winmd"

# Each [[partition]] maps a set of headers → one namespace
[[partition]]
namespace = "MyLib.Graphics"
library = "mylib.so"         # DLL/so for P/Invoke ImplMap entries
headers = ["include/graphics.h"]
traverse = ["include/graphics.h"]  # which files to actually emit from

[[partition]]
namespace = "MyLib.Audio"
library = "mylib.so"
headers = [
    "include/common.h",      # included for dependency resolution only
    "include/audio.h",
]
traverse = ["include/audio.h"]     # only emit types from audio.h

[[partition]]
namespace = "MyLib.Input"
library = "mylib_input.so"
headers = ["include/input.h"]
traverse = ["include/input.h"]

# Optional: per-API namespace overrides
# (equivalent to requiredNamespacesForNames.rsp)
[namespace_overrides]
"SomeInputStruct" = "MyLib.Graphics"   # move from default to this namespace
"SharedEnum" = "MyLib.Common"
```

### Implementation Flow

```
For each partition in config:
  1. Parse translation unit with clang crate
     - Index::new() → index.parser(wrapper_header).parse()
     - wrapper_header #includes all partition.headers
  2. Filter declarations by source location
     - Entity::get_location() → check against partition.traverse paths
     - Use sonar: find_structs(), find_enums(), find_functions(), etc.
  3. For each in-scope declaration:
     a. Check namespace_overrides map → use override if present
     b. Otherwise use partition.namespace
     c. Call file.TypeDef(namespace, name, ...)
```

### Cross-Partition Type References

When a type in partition A references a struct defined in partition B, the winmd
needs a **TypeRef** (not TypeDef) pointing to the other namespace. This mirrors
how win32metadata handles cross-namespace references.

In the emitter:
1. Build a global map of `{type_name → namespace}` across all partitions
2. When emitting a field/param type that refers to another type, look up which
   namespace it lives in
3. If it's in the current partition → reference the TypeDef directly
4. If it's in a different partition → emit `file.TypeRef(other_namespace, name)`

Since all partitions go into a single `.winmd` file, both TypeDefs and TypeRefs
resolve within the same assembly. The `windows-metadata` writer handles this
correctly — TypeRef rows are just index entries into the TypeRef table, and the
reader resolves them to TypeDef rows at load time.

### Summary

| Concern | C# Pipeline (ClangSharp) | Rust Pipeline (`clang` crate) |
|---|---|---|
| **Selective traversal** | `--traverse <path>` | `Entity::get_location()` filter |
| **Namespace assignment** | `--namespace <ns>` | Emitter code: `file.TypeDef(ns, ...)` |
| **Per-API overrides** | `requiredNamespacesForNames.rsp` | `[namespace_overrides]` in TOML |
| **Cross-ns references** | C# `using` + `NamesToCorrectNamespacesMover` | Global type→namespace map + TypeRef |
| **Config format** | `.rsp` response files + `main.cpp` | `bindscrape.toml` |

**Bottom line**: The `clang` crate gives direct source-location filtering
(like ClangSharp's `--traverse`) and full type data. Namespace assignment is
purely our emitter's responsibility.

---

## Cross-WinMD Type References (typeImports)

### The Problem

A C header being scraped often uses types that are already defined in another
winmd. For example, a custom library API returns `HRESULT`, which is defined as a
NativeTypedef struct in `Windows.Win32.Foundation` inside `Windows.Win32.winmd`.
We don't want to re-define HRESULT in our output winmd — we want to **reference**
the definition in `Windows.Win32.winmd`.

### How win32metadata Handles It

win32metadata has a `--typeImport` CLI option that maps type names to external
assemblies. The format is:

```
--typeImport "HRESULT=<Windows.Win32, Version=0.1.0.0, Culture=neutral, PublicKeyToken=null>Windows.Win32.Foundation.HRESULT"
```

This encodes: "When you encounter `HRESULT`, don't create a TypeDef — instead
create a **TypeRef** whose ResolutionScope points to an **AssemblyRef** for
`Windows.Win32`."

For interfaces, a suffix is used:
```
--typeImport "IPropertyValue(interface)=<Windows.Foundation.FoundationContract, Version=4.0.0.0, Culture=neutral, PublicKeyToken=null>Windows.Foundation.IPropertyValue"
```

#### What Happens in the Emitter

From
[`ClangSharpSourceWinmdGenerator.cs`](https://github.com/microsoft/win32metadata/blob/main/sources/ClangSharpSourceToWinmd/ClangSharpSourceWinmdGenerator.cs):

1. When the emitter encounters a type it can't resolve locally, it calls
   `ConvertTypeToImportedType(fullName, out typeRef)`
2. This parses the import string to extract: assembly name, version, culture,
   public key token, and fully-qualified type name
3. It creates an **AssemblyRef** row (one per referenced assembly,
   deduplicated via `assemblyNamesToRefHandles` dictionary):
   ```csharp
   assemblyRef = metadataBuilder.AddAssemblyReference(
       name, version, culture, publicKeyToken, flags, hashValue);
   ```
4. It creates a **TypeRef** row whose `ResolutionScope` is that AssemblyRef:
   ```csharp
   typeRef = metadataBuilder.AddTypeReference(
       assemblyRef,      // ResolutionScope = the AssemblyRef
       nsHandle,         // namespace, e.g. "Windows.Win32.Foundation"
       nameHandle);      // name, e.g. "HRESULT"
   ```
5. The C# compilation's CS0246 error ("type not found") is suppressed for any
   type name present in the `typeImports` dictionary

Any consumer of the output winmd that encounters this TypeRef follows the
AssemblyRef, loads the referenced winmd, and resolves the type there.

### How HRESULT Specifically Works

`HRESULT` is a **TypeDef** in `Windows.Win32.winmd`:
- Namespace: `Windows.Win32.Foundation`
- Extends: `System.ValueType`
- One field: `int` value
- Custom attribute: `NativeTypedefAttribute`
- Defined in
  [`autoTypes.json`](https://github.com/microsoft/win32metadata/blob/main/generation/WinSDK/autoTypes.json)

When a **different** winmd needs to reference it, it emits:
1. An AssemblyRef row → `Windows.Win32` (assembly name)
2. A TypeRef row → namespace `Windows.Win32.Foundation`, name `HRESULT`,
   ResolutionScope = that AssemblyRef

### What `windows-metadata` Writer Supports Today

The Rust writer **does** have both the AssemblyRef and TypeRef tables with
proper serialization. From
[`rec.rs`](https://github.com/microsoft/windows-rs/blob/master/crates/libs/metadata/src/writer/file/rec.rs):

```rust
pub struct AssemblyRef {
    pub MajorVersion: u16,
    pub MinorVersion: u16,
    pub BuildNumber: u16,
    pub RevisionNumber: u16,
    pub Flags: AssemblyFlags,
    pub PublicKeyOrToken: id::BlobId,
    pub Name: id::StringId,
    pub Culture: u32,
    pub HashValue: u32,
}

pub struct TypeRef {
    pub ResolutionScope: ResolutionScope,  // can point to AssemblyRef
    pub TypeName: id::StringId,
    pub TypeNamespace: id::StringId,
}
```

And `ResolutionScope` is a coded index that supports `AssemblyRef`:
```rust
// From codes.rs — ResolutionScope coded index (2-bit tag)
// Tag 0: Module, Tag 1: ModuleRef, Tag 2: AssemblyRef, Tag 3: TypeRef
```

**However**, the current `File` API has a limitation:

```rust
// File::AssemblyRef is PRIVATE (fn, not pub fn)
fn AssemblyRef(&mut self, namespace: &str) -> id::AssemblyRef {
    // Extracts root namespace (splits on first '.')
    // Creates a synthetic AssemblyRef with that root namespace as the name
    // e.g., "Windows.Win32.Foundation" → AssemblyRef named "Windows"
}

// File::TypeRef is PUBLIC and auto-calls AssemblyRef
pub fn TypeRef(&mut self, namespace: &str, name: &str) -> id::TypeRef {
    let scope = ResolutionScope::AssemblyRef(self.AssemblyRef(namespace));
    // Creates TypeRef with ResolutionScope = AssemblyRef
}
```

The private `AssemblyRef()` method uses a **root-namespace heuristic**: it splits
on the first `.` and uses that as the assembly name. So
`file.TypeRef("Windows.Win32.Foundation", "HRESULT")` creates:
- AssemblyRef named `"Windows"` (not `"Windows.Win32"`)
- TypeRef with namespace `"Windows.Win32.Foundation"`, name `"HRESULT"`

This heuristic works for WinRT types (where assembly names match root namespaces)
but **does not** match the actual assembly name `"Windows.Win32"` for the Win32
winmd. It might still work if the consumer resolves by namespace probe rather
than strict assembly name matching.

### Options for Cross-WinMD in Rust

**Option 1: Use the existing heuristic (may just work)**

Call `file.TypeRef("Windows.Win32.Foundation", "HRESULT")` and rely on the
consumer (e.g., `windows-bindgen`) resolving it by namespace matching. Since
`windows-bindgen` loads all winmd files into a single `TypeIndex` and resolves
TypeRefs by namespace+name lookup (not by AssemblyRef name), the synthetic
AssemblyRef name may not matter.

```rust
// This creates: TypeRef(ns="Windows.Win32.Foundation", name="HRESULT")
//   with AssemblyRef(name="Windows") — synthetic
let hresult_ref = file.TypeRef("Windows.Win32.Foundation", "HRESULT");
```

**Option 2: Push directly to records (requires forking the crate)**

Access `file.records.AssemblyRef` directly with the correct assembly name and
version. This is the most correct ECMA-335 approach but requires forking
`windows-metadata` since `records` is not public.

```rust
// Would need: pub fn AssemblyRef(&mut self, name, version, ...) -> id::AssemblyRef
let asm_ref = file.AssemblyRef_custom("Windows.Win32", 0, 1, 0, 0, None);
let hresult_ref = file.TypeRef_with_scope(
    ResolutionScope::AssemblyRef(asm_ref),
    "Windows.Win32.Foundation",
    "HRESULT",
);
```

**Option 3: Submit a PR to `windows-metadata`**

Add a public `AssemblyRef` method or make the existing one more flexible:
```rust
pub fn AssemblyRef(
    &mut self,
    name: &str,
    version: (u16, u16, u16, u16),
    public_key_token: Option<&[u8]>,
) -> id::AssemblyRef
```

This is the cleanest solution and benefits the crate generally.

**Option 4: Define imported types locally (simplest)**

For many use cases, just re-define `HRESULT` as a TypeDef in the output winmd:
```rust
let hresult = file.TypeDef(
    "MyLib.Foundation", "HRESULT",
    TypeDefOrRef::TypeRef(file.TypeRef("System", "ValueType")),
    TypeAttributes::PUBLIC | TypeAttributes::SEQUENTIAL_LAYOUT,
);
file.Field("Value", &Type::I32, FieldAttributes::PUBLIC);
```

This avoids cross-winmd references entirely. The consumer sees a local HRESULT
type. Downside: it's a separate type from `Windows.Win32.Foundation.HRESULT`, so
language projections won't unify them.

### Proposed Config for bindscrape

```toml
# In bindscrape.toml — type imports from external winmd files
[[type_import]]
# Types to import from Windows.Win32.winmd rather than re-defining
assembly = "Windows.Win32"
version = "0.1.0.0"
types = [
    { name = "HRESULT", namespace = "Windows.Win32.Foundation" },
    { name = "NTSTATUS", namespace = "Windows.Win32.Foundation" },
    { name = "BOOL", namespace = "Windows.Win32.Foundation" },
    { name = "PWSTR", namespace = "Windows.Win32.Foundation" },
    { name = "PSTR", namespace = "Windows.Win32.Foundation" },
]

# Or for WinRT types:
[[type_import]]
assembly = "Windows.Foundation.FoundationContract"
version = "4.0.0.0"
types = [
    { name = "IPropertyValue", namespace = "Windows.Foundation", interface = true },
]
```

### Recommendation

For v1: **Option 4** (define types locally). Cross-winmd references add
complexity and most custom winmd consumers don't need type unification with
Windows.Win32.winmd.

For v2: **Option 3** (submit PR to `windows-metadata`) to enable proper
AssemblyRef with custom assembly name/version. Then implement the
`[[type_import]]` config. This is straightforward — the underlying ECMA-335
table support already exists in the writer; only the public API surface is
missing.

---

## Difficulty Comparison: Rust vs C# Pipeline

| Aspect | C# Pipeline (CSharpGenerator) | Rust Pipeline (Option B) |
|---|---|---|
| **C header parsing** | ClangSharp (.NET tool, proven, 5+ years) | `clang` crate (same libclang underneath) |
| **Intermediate format** | C# source files | In-memory model types |
| **Winmd writing** | Fork of `ClangSharpSourceWinmdGenerator` (~1,800 LOC C#), uses .NET `MetadataBuilder` | New code using `windows-metadata::writer::File`, ~800-1,200 LOC Rust |
| **PE serialization** | `ManagedPEBuilder` (.NET) | `windows-metadata::writer::File::into_stream()` (Rust, already done) |
| **Total new code** | ~200 LOC orchestration + fork ~1,800 LOC emitter | ~1,000 LOC mapping + ~200 LOC orchestration |
| **Dependencies** | .NET SDK 8.0+, ClangSharp NuGet, Roslyn, System.Reflection.Metadata | `clang`, `windows-metadata`, `toml` (all cargo crates) |
| **Toolchains needed** | .NET + optional Rust (for CLI) | Rust only |
| **Runtime perf** | .NET JIT startup + Roslyn compilation overhead | Native, near-instant |
| **`#define` constants** | Same limitation (need ConstantsScraper or manual) | `sonar::find_definitions()` + `evaluate()` for simple defines |
| **Bitfield support** | Full (`ClangSharp` exposes widths) | Full (`Entity::get_bit_field_width()`) |
| **Maintenance** | Must track win32metadata Emitter changes | Must track `clang` crate + `clang-sys` updates |
| **Proven for winmd** | Yes — same code produces Windows.Win32.winmd | No — new mapping, needs validation |

**Summary**: The Rust pipeline requires more new code (no existing emitter to fork)
but is a single toolchain, faster, and maps the same libclang API that ClangSharp
uses. The conceptual mapping from C AST → ECMA-335 is identical in both pipelines.

---

## Implementation Sketch (Option B)

### Crate Structure

```
bindscrape/
├── Cargo.toml
├── src/
│   ├── lib.rs               # Module declarations
│   ├── main.rs              # CLI: parse args, read TOML, orchestrate (86 LOC)
│   ├── config.rs            # bindscrape.toml deserialization (103 LOC)
│   ├── model.rs             # Intermediate types: StructDef, CType, etc. (171 LOC)
│   ├── extract.rs           # clang Entity/Type → intermediate model (512 LOC)
│   └── emit.rs              # model → windows-metadata writer calls (378 LOC)
└── tests/
    ├── roundtrip.rs          # 8 round-trip integration tests (155 LOC)
    └── fixtures/
        ├── simple.h          # Smoke test header (enums, structs, functions, etc.)
        └── simple.toml       # Config for smoke test
```

> **Note**: The `constants.rs` module from the original design was merged into
> `extract.rs`. The `sonar::find_definitions()` call fits naturally in the
> partition extraction loop alongside structs/enums/functions.

### Core Dependencies

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

> **Actual dependencies** match this spec exactly. `serde` was added for TOML
> config deserialization. `windows-metadata` is not pinned with `=` in practice\n> since v0.59 is the only version the code has been tested against.

Note: `clang` depends on `clang-sys` transitively. For newer libclang features,
add `clang-sys` as a direct dependency and call raw FFI where needed.

`windows-metadata` is pinned to an exact version (`=0.59`) because the crate
has no semver stability guarantees — the writer API can change between minor
releases.

### Pseudocode for the Emitter

```rust
use clang::{Clang, Index, Entity, Type as CType, EntityKind, TypeKind};
use clang::sonar::{find_structs, find_enums, find_functions, find_typedefs, find_unions};
use windows_metadata::writer;

fn emit_winmd(config: &Config) -> Vec<u8> {
    let clang = Clang::new().unwrap();
    let index = Index::new(&clang, false, false);
    let mut file = writer::File::new(&config.output.name);

    for partition in &config.partitions {
        // Parse the translation unit
        let tu = index.parser(&partition.wrapper_header())
            .arguments(&partition.clang_args())
            .parse()
            .unwrap();

        let entities = tu.get_entity().get_children();

        // Filter to only declarations from traverse files
        let in_scope = |e: &Entity| should_emit(e, &partition.traverse);

        // Emit enums
        for decl in find_enums(&entities) {
            if !in_scope(&decl.entity) { continue; }
            let underlying = decl.entity.get_enum_underlying_type().unwrap();
            let ecma_ty = map_clang_type(&underlying);
            let typedef = file.TypeDef(
                &partition.namespace, &decl.name,
                file.TypeRef("System", "Enum"),
                TypeAttributes::PUBLIC | TypeAttributes::SEALED,
            );
            file.Field("value__", &ecma_ty, FieldAttributes::PUBLIC | FieldAttributes::RTSPECIALNAME);
            for child in decl.entity.get_children() {
                if child.get_kind() == EntityKind::EnumConstantDecl {
                    let (signed, unsigned) = child.get_enum_constant_value().unwrap();
                    let field = file.Field(
                        &child.get_name().unwrap(), &ecma_ty,
                        FieldAttributes::PUBLIC | FieldAttributes::STATIC | FieldAttributes::LITERAL,
                    );
                    file.Constant(HasConstant::Field(field), &signed);
                }
            }
        }

        // Emit structs
        for decl in find_structs(&entities) {
            if !in_scope(&decl.entity) { continue; }
            let ty = decl.entity.get_type().unwrap();
            let size = ty.get_sizeof().unwrap_or(0);
            let align = ty.get_alignof().unwrap_or(0);

            let typedef = file.TypeDef(
                &partition.namespace, &decl.name,
                file.TypeRef("System", "ValueType"),
                TypeAttributes::PUBLIC | TypeAttributes::SEQUENTIAL_LAYOUT,
            );
            file.ClassLayout(typedef, align as u16, size as u32);

            for field in decl.entity.get_children() {
                if field.get_kind() == EntityKind::FieldDecl {
                    let field_ty = map_clang_type(&field.get_type().unwrap());
                    file.Field(
                        &field.get_name().unwrap_or_default(),
                        &field_ty,
                        FieldAttributes::PUBLIC,
                    );
                }
            }
        }

        // Emit functions (P/Invoke)
        let apis = file.TypeDef(
            &partition.namespace, "Apis",
            file.TypeRef("System", "Object"),
            TypeAttributes::PUBLIC | TypeAttributes::ABSTRACT | TypeAttributes::SEALED,
        );
        for decl in find_functions(&entities) {
            if !in_scope(&decl.entity) { continue; }
            let fn_type = decl.entity.get_type().unwrap();
            let ret_type = fn_type.get_result_type().unwrap();
            let param_types = fn_type.get_argument_types().unwrap_or_default();
            let args = decl.entity.get_arguments().unwrap_or_default();

            let sig = build_signature(&ret_type, &param_types);
            let method = file.MethodDef(
                &decl.name, &sig,
                MethodAttributes::PUBLIC | MethodAttributes::STATIC | MethodAttributes::PINVOKEIMPL,
                MethodImplAttributes::PRESERVE_SIG,
            );
            file.ImplMap(method, PInvokeAttributes::CONV_PLATFORM, &decl.name, &partition.library);

            for (i, arg) in args.iter().enumerate() {
                file.Param(
                    &arg.get_name().unwrap_or_default(),
                    (i + 1) as u16,
                    ParamAttributes(0),
                );
            }
        }
    }

    file.into_stream()
}
```

### Type Mapping (clang TypeKind → ECMA-335)

```rust
use clang::TypeKind;
use windows_metadata::Type as WinType;

fn map_clang_type(ty: &clang::Type) -> WinType {
    match ty.get_kind() {
        TypeKind::Void => WinType::Void,
        TypeKind::Bool => WinType::Bool,
        TypeKind::CharS | TypeKind::SChar => WinType::I8,
        TypeKind::CharU | TypeKind::UChar => WinType::U8,
        TypeKind::Short => WinType::I16,
        TypeKind::UShort => WinType::U16,
        TypeKind::Int => WinType::I32,
        TypeKind::UInt => WinType::U32,
        TypeKind::Long => WinType::I32,      // Windows ABI: C long = 32-bit
        TypeKind::ULong => WinType::U32,     // Windows ABI: C ulong = 32-bit
        TypeKind::LongLong => WinType::I64,
        TypeKind::ULongLong => WinType::U64,
        TypeKind::Float => WinType::F32,
        TypeKind::Double => WinType::F64,
        TypeKind::Pointer => {
            let pointee = ty.get_pointee_type().unwrap();
            if pointee.is_const_qualified() {
                WinType::PtrConst(Box::new(map_clang_type(&pointee)), 1)
            } else {
                WinType::PtrMut(Box::new(map_clang_type(&pointee)), 1)
            }
        }
        TypeKind::ConstantArray => {
            let elem = ty.get_element_type().unwrap();
            let len = ty.get_size().unwrap();
            WinType::ArrayFixed(Box::new(map_clang_type(&elem)), len)
        }
        TypeKind::Elaborated => {
            // Strip elaborated type sugar (e.g., "struct Foo" → "Foo")
            let inner = ty.get_elaborated_type().unwrap();
            map_clang_type(&inner)
        }
        TypeKind::Typedef => {
            // Resolve typedef to canonical type, unless we want to preserve it
            let decl = ty.get_declaration().unwrap();
            let name = decl.get_name().unwrap();
            // Named types: look up in our type registry
            WinType::named(&namespace_for(&name), &name)
        }
        TypeKind::Record => {
            let decl = ty.get_declaration().unwrap();
            let name = decl.get_name().unwrap();
            WinType::named(&namespace_for(&name), &name)
        }
        TypeKind::Enum => {
            let decl = ty.get_declaration().unwrap();
            let name = decl.get_name().unwrap();
            WinType::named(&namespace_for(&name), &name)
        }
        TypeKind::FunctionPrototype => {
            // Emit as delegate TypeDef and reference it
            todo!("Function pointer → delegate")
        }
        _ => panic!("Unsupported clang TypeKind: {:?}", ty.get_kind()),
    }
}
```

**Key advantage**: No reverse mapping needed. `TypeKind::Int` maps directly to
`I32`, `TypeKind::ULong` maps directly to `U32`. In Option A, we'd need to
pattern-match `::std::os::raw::c_int` strings to recover the same information.

---

## What `windows-metadata::writer` Does NOT Handle

The writer API is intentionally low-level. These things must be handled by
bindscrape:

1. **No semantic validation** — you can write invalid ECMA-335 tables if you
   encode things wrong. The writer trusts you.
2. **No automatic parent-child relationships** — TypeDef → Field/MethodDef
   associations are implicit via row ordering (each TypeDef's `FieldList`
   and `MethodList` point to the first row belonging to it). You must add
   fields/methods in the right order.
3. **No C type → ECMA-335 type mapping** — this is entirely our responsibility
4. **Limited `#define` extraction** — libclang's AST does not represent
   `#define` macros, but the `clang` crate's `sonar::find_definitions()` + 
   `Entity::evaluate()` can extract simple integer-valued defines. Complex or
   string macros require a preprocessor dump fallback (`cc -E -dM`).
5. **Value type vs class distinction** — the writer currently hardcodes
   `ELEMENT_TYPE_VALUETYPE` for all `Type::Name` references
   ([noted as a TODO](https://github.com/microsoft/windows-rs/blob/master/crates/libs/metadata/src/writer/file/mod.rs)).
   For COM interfaces (which are classes), this may need a patch or workaround.

---

## `windows-metadata::writer` vs .NET `MetadataBuilder`

| Feature | `windows-metadata` (Rust) | `MetadataBuilder` (.NET) |
|---|---|---|
| TypeDef | ✅ `File::TypeDef()` | ✅ `AddTypeDefinition()` |
| TypeRef | ✅ `File::TypeRef()` | ✅ `AddTypeReference()` |
| MethodDef | ✅ `File::MethodDef()` | ✅ `AddMethodDefinition()` |
| Field | ✅ `File::Field()` | ✅ `AddFieldDefinition()` |
| Param | ✅ `File::Param()` | ✅ `AddParameter()` |
| ImplMap (P/Invoke) | ✅ `File::ImplMap()` | ✅ `AddMethodImport()` |
| ClassLayout | ✅ `File::ClassLayout()` | ✅ `AddTypeLayout()` |
| FieldLayout | ✅ `File::FieldLayout()` | ✅ `AddFieldLayout()` |
| InterfaceImpl | ✅ `File::InterfaceImpl()` | ✅ `AddInterfaceImplementation()` |
| CustomAttribute | ✅ `File::Attribute()` | ✅ `AddCustomAttribute()` |
| Constant | ✅ `File::Constant()` | ✅ `AddConstant()` |
| GenericParam | ✅ `File::GenericParam()` | ✅ `AddGenericParameter()` |
| NestedClass | ✅ `File::NestedClass()` | ✅ `AddNestedType()` |
| TypeSpec | ✅ `File::TypeSpec()` | ✅ `GetOrAddTypeSpec()` |
| MemberRef | ✅ `File::MemberRef()` | ✅ `AddMemberReference()` |
| PE serialization | ✅ `into_stream()` (built-in) | ✅ `ManagedPEBuilder` |
| Signature encoding | ✅ Built-in (Type → blob) | ✅ `BlobEncoder` |

The APIs are functionally equivalent. Every ECMA-335 table that the C# Emitter
writes, the Rust writer can also write.

---

## `#define` Constants

libclang's AST does not represent `#define` macros, which is the same limitation
ClangSharp has (win32metadata uses ConstantsScraper, a regex-based tool, to fill
this gap).

However, the `clang` crate provides partial support:

1. **`sonar::find_definitions()`** — iterates over preprocessor `#define`
   directives as `Declaration` values. Each `Declaration` has a `.entity` with
   `EntityKind::MacroDefinition`.

2. **`Entity::evaluate()`** — evaluates constant expressions at the clang level.
   For simple `#define FOO 42` or `#define BAR (1 << 3)`, `evaluate()` may return
   `Some(EvaluationResult::SignedInteger(42))`. This works for integer-valued
   macros but not for string or complex expression macros.

3. **Preprocessor dump fallback** — run `cc -E -dM header.h` (subprocess) to
   dump all macro definitions, then parse with regex. This handles string
   constants and complex macros that `evaluate()` cannot resolve.

4. **TOML config override** — for small APIs, define constants manually in the
   project's TOML config file.

Recommended v1 approach: use `sonar::find_definitions()` + `evaluate()` for
simple integer defines (covers the majority of cases). Add the preprocessor dump
fallback if more constants are needed.

```rust
use clang::sonar;

let definitions = sonar::find_definitions(tu.get_entity().get_children());
for def in definitions {
    if let Some(result) = def.entity.evaluate() {
        match result {
            EvaluationResult::SignedInteger(val) => {
                // Emit as winmd Constant: file.Field(...) + file.Constant(val)
            }
            EvaluationResult::UnsignedInteger(val) => { /* ... */ }
            _ => {} // Skip non-integer defines
        }
    }
}
```

---

## Compatibility with `windows-bindgen`

[`windows-bindgen`](https://github.com/microsoft/windows-rs/tree/master/crates/libs/bindgen)
reads winmd files to generate Rust code. For bindscrape's output to be consumed
by `windows-bindgen`, it needs to match these conventions:

1. **TypeDef row ordering** — `FieldList` / `MethodList` must correctly delimit
   which fields/methods belong to each type
2. **P/Invoke** — functions must have ImplMap entries pointing to ModuleRef
   (the DLL/so name)
3. **Enums** — must extend `System.Enum`, have `value__` field, literal fields
   with `Constant` values
4. **Structs** — must extend `System.ValueType`, have `SequentialLayout` or
   `ExplicitLayout` flag, `ClassLayout` for size/packing
5. **Delegates** — must extend `System.MulticastDelegate`, have `Invoke` method
6. **Custom attributes** — `NativeTypedefAttribute` for typedef wrappers,
   `NativeBitfieldAttribute` for bitfield info, `SupportedArchitectureAttribute`
   for arch-specific types, `GuidAttribute` for COM interface GUIDs

Since `windows-bindgen` and our writer share the same `windows-metadata` crate,
the low-level encoding is guaranteed compatible. The semantic conventions above
are what we need to get right.

---

## Error Handling Strategy

**Crate**: [`anyhow`](https://crates.io/crates/anyhow) for top-level
`Result<T, anyhow::Error>` propagation. No custom error enum for v1.

**Policy: warn-and-skip for non-fatal failures.**

If a single declaration fails to extract or emit (e.g., an unsupported TypeKind,
a missing name, a libclang error), log a warning via `tracing::warn!` and skip
that declaration. Do not abort the entire run. This matches how ClangSharp
behaves — it logs warnings for unsupported constructs and continues.

Fatal errors (can't open header, can't create Index, can't write output file)
propagate up as `anyhow::Result` and abort.

```rust
for decl in find_structs(&entities) {
    if !in_scope(&decl.entity) { continue; }
    match emit_struct(&mut file, &partition.namespace, &decl) {
        Ok(_) => tracing::debug!(name = %decl.name, "emitted struct"),
        Err(e) => tracing::warn!(name = %decl.name, err = %e, "skipping struct"),
    }
}
```

---

## Logging / Diagnostics

**Crate**: [`tracing`](https://crates.io/crates/tracing) +
[`tracing-subscriber`](https://crates.io/crates/tracing-subscriber) with
`env-filter`.

Useful for debugging "why wasn't type X emitted?" and tracking what the tool
does at each step.

**Log levels**:

| Level | What gets logged |
|---|---|
| `error` | Fatal: can't parse TU, can't write output |
| `warn` | Skipped declaration (unsupported TypeKind, anonymous with no name, etc.) |
| `info` | Per-partition summary: "Partition MyLib.Audio: 12 structs, 5 enums, 23 functions" |
| `debug` | Each emitted declaration: "emitted struct Foo (size=32, fields=4)" |
| `trace` | Field-level detail, type mapping decisions, clang cursor traversal |

**Usage**: `RUST_LOG=bindscrape=debug cargo run -- bindscrape.toml`

```rust
fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = load_config(&args.config)?;
    tracing::info!(partitions = config.partitions.len(), "loaded config");
    // ...
}
```

---

## Anonymous / Unnamed Types

C headers commonly contain anonymous structs, unions, and enums:

```c
typedef struct { int x; int y; } Point;           // unnamed struct with typedef
struct Widget {
    union { int a; float b; };                     // anonymous union member
    struct { int r, g, b; } color;                 // named field, anonymous struct type
};
enum { FLAG_A = 1, FLAG_B = 2 };                   // anonymous enum
```

The `clang` crate reports these with `Entity::is_anonymous()` → `true` and
`get_name()` → `None`.

### Handling rules

| Case | Detection | Action |
|---|---|---|
| `typedef struct { ... } Name;` | `EntityKind::TypedefDecl` with anonymous child | Use the typedef name (`Name`) as the TypeDef name |
| Anonymous union/struct inside a parent struct | `is_anonymous() == true`, parent is `StructDecl` | Generate synthetic name: `ParentName__Anonymous_N` (N = 0, 1, ...) |
| Anonymous enum | `is_anonymous() == true`, `EntityKind::EnumDecl` | Emit each variant as a standalone constant (no TypeDef for the enum) |
| Named field with anonymous type | Field has a name but its type's decl is anonymous | Use `FieldName_Type` or flatten fields into parent |

For v1, the primary path is **typedef-named structs** (by far the most common in
C APIs). Anonymous unions inside structs are deferred to v2 unless encountered
early.

```rust
fn resolve_name(entity: &Entity) -> Option<String> {
    // Try the entity's own name first
    if let Some(name) = entity.get_name() {
        return Some(name);
    }
    // For anonymous types, check if there's a parent typedef
    // (sonar resolves this via Declaration.source)
    None
}
```

---

## Validation Target

To validate the implementation end-to-end, use a **concrete smoke-test header**
as the day-1 target. Create `tests/fixtures/simple.h`:

```c
#pragma once

// Enum
typedef enum {
    COLOR_RED   = 0,
    COLOR_GREEN = 1,
    COLOR_BLUE  = 2,
} Color;

// Struct with basic fields
typedef struct {
    int x;
    int y;
    unsigned int width;
    unsigned int height;
} Rect;

// Struct with pointer and array
typedef struct {
    const char* name;
    int values[4];
    Color color;
} Widget;

// Function pointer (delegate)
typedef int (*CompareFunc)(const void* a, const void* b);

// Functions
int create_widget(const char* name, Rect bounds, Widget* out);
void destroy_widget(Widget* w);
int widget_count(void);

// #define constants
#define MAX_WIDGETS 256
#define DEFAULT_WIDTH 800
#define DEFAULT_HEIGHT 600
```

**Validation criteria** (round-trip test):
1. Parse `simple.h` → emit `simple.winmd`
2. Read back with `windows-metadata` reader
3. Assert:
   - 3 TypeDefs (`Color`, `Rect`, `Widget`) + 1 delegate (`CompareFunc`) + 1
     `Apis` class with 3 methods
   - `Color` extends `System.Enum`, has `value__` + 3 literal fields
   - `Rect` has `ClassLayout` with `size == 16`, 4 fields
   - `Widget.values` is `ArrayFixed(I32, 4)`, `Widget.color` references
     `Color` TypeDef
   - `create_widget` has ImplMap, 3 params, returns `I32`
   - Constants: `MAX_WIDGETS = 256`, `DEFAULT_WIDTH = 800`, `DEFAULT_HEIGHT = 600`

This header exercises enums, structs, pointers, arrays, named type references,
function pointers, P/Invoke functions, and `#define` constants — covering the
full v1 scope.

---

## Estimated Effort

### Original Estimate

| Component | Est. LOC | Effort |
|---|---|---|
| CLI + TOML config | ~200 | Easy |
| `clang` crate setup (Index, TranslationUnit, args) | ~100 | Easy — straightforward API |
| Partition filtering (`Entity::get_location()`) | ~100 | Easy — path matching on source location |
| `sonar` + Entity traversal → intermediate model | ~400 | Medium — mapping EntityKind/TypeKind to model |
| Intermediate model types (`model.rs`) | ~150 | Easy — data definitions |
| TypeKind → ECMA-335 type mapping | ~200 | Easy — direct enum match, no reverse mapping |
| Struct/Enum/Function → TypeDef writing | ~500 | Medium — most of the new logic |
| Bitfield handling (`NativeBitfieldAttribute`) | ~100 | Easy — `is_bit_field()` + `get_bit_field_width()` |
| Custom attributes (NativeTypedefs, etc.) | ~100 | Easy — mechanical |
| `#define` constant extraction | ~100 | Easy — `sonar::find_definitions()` + `evaluate()` |
| Error handling + tracing setup | ~100 | Easy — anyhow + tracing boilerplate |
| Anonymous type resolution | ~100 | Easy — typedef names, synthetic names |
| Tests (round-trip, integration) | ~350 | Medium |
| **Total** | **~2,500** | **5-6 weeks for one developer** |

### Actual (v1 Implementation)

| Component | Actual LOC | File |
|---|---|---|
| CLI + TOML config | 189 | `main.rs` (86) + `config.rs` (103) |
| Intermediate model types | 171 | `model.rs` |
| Extraction (clang → model, type mapping, filtering) | 512 | `extract.rs` |
| Emission (model → winmd) | 378 | `emit.rs` |
| Module declarations | 9 | `lib.rs` |
| Integration tests | 155 | `tests/roundtrip.rs` |
| **Total** | **1,414** | |

The actual LOC is ~43% less than the original ~2,500 estimate. Key reasons:
- The `constants.rs` module was unnecessary — constant extraction folded into
  `extract.rs` naturally.
- `sonar` module iterators eliminated most manual cursor traversal boilerplate.
- The `windows-metadata` writer API is more concise than anticipated (one
  method call per table row).
- Bitfield attribute emission and custom attribute boilerplate are deferred to v2.

For comparison, the C# pipeline (CSharpGenerator.md) requires forking ~1,800 LOC
of existing code plus ~200 LOC of new orchestration, but also needs a .NET
project, two toolchains, and subprocess management.

---

## Blockers & Mitigations

### 1. ~~`ParseCallbacks` Does Not Provide Structural Data~~ (NO LONGER RELEVANT)

**Status**: N/A — Option B does not use bindgen or ParseCallbacks.

Option B reads structural data directly from libclang via the `clang` crate's
`Entity` and `Type` types.

### 2. `ELEMENT_TYPE_VALUETYPE` Hardcoded in Writer

**Severity**: Medium (blocks COM interface support)
**Status**: Deferred to v2

The `windows-metadata` writer encodes all `Type::Name` references as
`ELEMENT_TYPE_VALUETYPE`. COM interfaces require `ELEMENT_TYPE_CLASS`.
The writer source has a
[TODO comment](https://github.com/microsoft/windows-rs/blob/master/crates/libs/metadata/src/writer/file/mod.rs)
acknowledging this.

**Mitigation**: v1 scope excludes COM interfaces (per Q4 — Minimum Viable Feature Set).
For v2: submit a PR to `windows-metadata` adding `Type::Class(TypeName)`, or
manually encode the signature blob bytes.

### 3. `AssemblyRef` Is Private

**Severity**: Low (blocks cross-winmd type references only)
**Status**: Mitigated by v1 strategy

The writer's `File::AssemblyRef()` method is private and uses a root-namespace
heuristic (splits on first `.`). Cannot create AssemblyRef with exact assembly
name like `"Windows.Win32"`.

**Mitigation**: v1 defines imported types locally (Option 4 in
[Cross-WinMD Type References](#options-for-cross-winmd-in-rust)). For v2: submit
PR to `windows-metadata` to expose a public `AssemblyRef()` method.

### 4. ~~Bitfield Info Loss~~ (RESOLVED by Option B)

**Status**: Resolved — `Entity::get_bit_field_width()` and `is_bit_field()`
provide the original bitfield widths directly from libclang.

In Option A, bindgen lowered bitfields to opaque integer fields, losing the
original widths. Option B reads them directly:
```rust
if field.is_bit_field() {
    let width = field.get_bit_field_width().unwrap(); // e.g., 5
    let offset = field.get_offset_of_field().unwrap(); // bit offset
    // Emit NativeBitfieldAttribute with name, offset, width
}
```

### 5. C `long` Is 64-bit on Linux Host

**Severity**: Low (easy to get wrong, easy to fix)
**Status**: Open — handle in type mapping

Since winmd targets the Windows ABI, C `long` must always map to 32-bit `I32`
regardless of the host platform. libclang running on Linux will report `long` as
`TypeKind::Long` — but `Type::get_sizeof()` will return 8 (Linux ABI).

**Mitigation**: In the type mapping, use `TypeKind::Long` → `I32` and
`TypeKind::ULong` → `U32` unconditionally. Do NOT use `get_sizeof()` for
primitive type sizing — use the `TypeKind` enum directly. `get_sizeof()` is
only needed for struct total size / struct ClassLayout.

### 6. TypeDef Row Ordering

**Severity**: Low (causes subtle corruption if wrong)
**Status**: Open — handle in emitter

ECMA-335 uses implicit `FieldList` / `MethodList` associations: each TypeDef's
field/method range is determined by the row indices of the _next_ TypeDef. This
means fields and methods must be added in strict sequential order immediately
after their owning TypeDef.

**Mitigation**: The emitter must follow a strict pattern:
```rust
let td = file.TypeDef(...);
file.Field(...);  // belongs to td
file.Field(...);  // belongs to td
// Next TypeDef starts a new range
let td2 = file.TypeDef(...);
file.MethodDef(...);  // belongs to td2
```
Add round-trip validation tests that read back the winmd with the reader and
verify each TypeDef's fields/methods.

### 7. `clang` Crate Max Feature Is `clang_10_0`

**Severity**: Low (for C header parsing)
**Status**: Monitoring

The `clang` crate's highest feature flag is `clang_10_0`. System libclang
may be 15+ (Ubuntu 24.04 ships clang 18). The crate works with newer libclang
versions — it just can't call APIs added after clang 10.

**Mitigation**: For C headers (no C++20 or Objective-C features needed), all
required APIs (`get_sizeof`, `get_alignof`, `get_offsetof`,
`get_bit_field_width`, `get_calling_convention`, etc.) have been stable since
clang 3.x. If a newer API is needed, add `clang-sys` as a direct dependency
and call the specific function via raw FFI.

---

## Open Questions

### Q1: ~~Why Not `ParseCallbacks` or Raw IR?~~ (SUPERSEDED)

**No longer relevant** — Option B bypasses bindgen entirely. We read
structural data directly from libclang via the `clang` crate.

For historical context: `ParseCallbacks::DiscoveredItem` only carries names
(verified against bindgen v0.72), and bindgen's IR is `pub(crate)`. These
limitations were a key reason for choosing Option B over Option A.

### Q2: ~~Is `syn` Parsing Lossy?~~ (SUPERSEDED)

**No longer relevant** — Option B does not generate or parse Rust code. All
type information comes directly from `clang::Entity` and `clang::Type`.

Information availability in Option B:

| Information | Available via `clang` crate? | How |
|---|---|---|
| Struct fields + types | ✅ Yes | `Entity::get_children()` → field entities → `get_type()` |
| Function signatures | ✅ Yes | `Entity::get_arguments()` + `get_result_type()` |
| Enum variants + values | ✅ Yes | `Entity::get_enum_constant_value()` → `(i64, u64)` |
| Typedefs | ✅ Yes | `Entity::get_typedef_underlying_type()` |
| Struct size / alignment | ✅ Yes | `Type::get_sizeof()` / `Type::get_alignof()` |
| Calling convention | ✅ Yes | `Type::get_calling_convention()` → `CallingConvention::StdCall` etc. |
| Bitfield layout | ✅ Yes | `Entity::is_bit_field()` + `get_bit_field_width()` + `get_offset_of_field()` |
| Original C type names | ✅ Yes | `Type::get_display_name()` / `TypeKind` enum |
| `#define` integer values | ✅ Partial | `sonar::find_definitions()` + `Entity::evaluate()` |
| Packing (`#pragma pack`) | ⚠️ Indirect | `get_sizeof()` < sum of fields → packed; or parse pragmas manually |

No gaps. Every cell that was ❌ under Option A is now ✅ under Option B.

### Q3: `windows-metadata` Value Type vs Class

The writer currently hardcodes `ELEMENT_TYPE_VALUETYPE` for all named types.
COM interfaces should be `ELEMENT_TYPE_CLASS`. Options:
- Submit a PR to `windows-metadata` adding `Type::Class(TypeName)`
- Manually encode the signature blob bytes using the writer's blob API
- Fork the writer and add the distinction

### Q4: Minimum Viable Feature Set

For v1, focus on:
- ✅ Structs (sequential layout with `ClassLayout` for size/packing) — **IMPLEMENTED**
- ✅ Enums (with constants from `get_enum_constant_value()`) — **IMPLEMENTED**
- ✅ Functions (P/Invoke with DllImport/ModuleRef) — **IMPLEMENTED**
- ✅ Function pointers (delegates) — **IMPLEMENTED**
- ✅ Typedefs — **IMPLEMENTED**
- ✅ `#define` integer constants (via `sonar::find_definitions()`) — **IMPLEMENTED**
- ⬜ Bitfields (`NativeBitfieldAttribute` emission — extraction works, emit is TODO)
- ⬜ Unions (explicit layout — easy with `FieldLayout`)
- ⬜ COM interfaces (if needed for the target libraries)
- ⬜ Nested types

> **Update (2026-02-14):** v1 implementation is complete. Multi-partition and
> cross-partition support working. 29 tests passing (13 roundtrip, 8 e2e-multi,
> 6 e2e-test, 2 doc-tests). Multi-header wrapper generation added. Bitfield
> extraction works at the clang level but `NativeBitfieldAttribute` emission
> is marked TODO.

---

## Reference Links

| Resource | URL |
|---|---|
| `windows-metadata` crate (reader + writer) | https://crates.io/crates/windows-metadata |
| `windows-metadata` source | https://github.com/microsoft/windows-rs/tree/master/crates/libs/metadata |
| Writer module source | https://github.com/microsoft/windows-rs/tree/master/crates/libs/metadata/src/writer |
| Writer `File` API | https://github.com/microsoft/windows-rs/blob/master/crates/libs/metadata/src/writer/file/mod.rs |
| Writer PE serialization | https://github.com/microsoft/windows-rs/blob/master/crates/libs/metadata/src/writer/file/into_stream.rs |
| Writer record types | https://github.com/microsoft/windows-rs/blob/master/crates/libs/metadata/src/writer/file/rec.rs |
| Writer coded indexes | https://github.com/microsoft/windows-rs/blob/master/crates/libs/metadata/src/writer/codes.rs |
| Type enum | https://github.com/microsoft/windows-rs/blob/master/crates/libs/metadata/src/ty.rs |
| Reader module | https://github.com/microsoft/windows-rs/tree/master/crates/libs/metadata/src/reader |
| `windows-bindgen` (winmd → Rust) | https://github.com/microsoft/windows-rs/tree/master/crates/libs/bindgen |
| `clang` crate (idiomatic wrapper) | https://crates.io/crates/clang |
| `clang` crate docs | https://docs.rs/clang/latest/clang/ |
| `clang::Entity` API | https://docs.rs/clang/latest/clang/struct.Entity.html |
| `clang::Type` API | https://docs.rs/clang/latest/clang/struct.Type.html |
| `clang::sonar` module | https://docs.rs/clang/latest/clang/sonar/index.html |
| `clang` crate source (GitHub) | https://github.com/KyleMayes/clang-rs |
| `clang-sys` crate (raw FFI) | https://crates.io/crates/clang-sys |
| `clang-sys` source (GitHub) | https://github.com/KyleMayes/clang-sys |
| `clang-ast` crate (JSON AST) | https://crates.io/crates/clang-ast |
| `windows-rs` repo | https://github.com/microsoft/windows-rs |
| ECMA-335 spec (Type encoding) | https://www.ecma-international.org/publications-and-standards/standards/ecma-335/ |
