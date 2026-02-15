# Work In Progress

## Blockers — Core Features

### 1. ~~Union support~~ ✅

**Files**: `model.rs`, `extract.rs`, `emit.rs`

Implemented. `StructDef` now has `is_union: bool`. The supplemental pass
in `collect_structs` detects `EntityKind::UnionDecl`. `emit_struct` uses
`ExplicitLayout` for unions and `SequentialLayout` for structs. Tested
with `Value` union in `simple.h` fixture — roundtrip verified
(`roundtrip_union_fields` test confirms `ExplicitLayout` flag, 3 fields,
and `ClassLayout`).

**Blocks**: nothing remaining

### 2. ~~Anonymous nested types~~ ✅

**Files**: `extract.rs`

Implemented. `extract_struct_from_entity` now detects anonymous record
fields via `Entity::is_anonymous()` on the canonical type's declaration.
Anonymous records are recursively extracted as separate `StructDef`
entries with synthetic names (`ParentName_FieldName`). The
`try_extract_anonymous_field` helper handles deeply nested anonymous
types. Tested with `NetAddr` struct containing an anonymous union field
`addr` → extracted as `NetAddr_addr` union.

**Blocks**: nothing remaining

### ~~3. Fixed-size array fields in structs~~ ✅ Not a blocker

Struct field arrays already work — `windows-bindgen` generates native
Rust arrays (e.g., `[i64; 3]`) directly from metadata table entries.
The `ELEMENT_TYPE_ARRAY` blob mismatch only affects **method signature
blobs**, not `FieldSig` blobs. Confirmed working: `stat.__glibc_reserved: [i64; 3]`.

The [bug doc](bugs/element-type-array-mismatch.md) and the parameter
decay workaround remain relevant for function parameters only.

---

## Planned — bns-posix API Families

### 4. PosixSocket partitions

Add `sys/socket.h`, `netinet/in.h`, `arpa/inet.h`, `netdb.h` to
bns-posix. Full design in
[systesting/bns-posix.md](design/systesting/bns-posix.md#posixsocket--sockets).

**Blocked by**: nothing (unions ✅, anonymous types ✅)

### ~~5. Mmap partition~~ ✅

`sys/mman.h` — `mmap`/`munmap`/`mprotect`/`msync`/`madvise` and friends.
Hex constant extraction bug discovered and fixed (`parse_hex_or_suffixed_int`
helper handles `0x`, `0` octal, and `U`/`L`/`UL`/`ULL` suffixes).
All `PROT_*`, `MAP_*`, `MS_*`, `MADV_*` constants emitted. 5 E2E tests
(`prot_constants`, `map_constants`, `msync_constants`,
`mmap_anonymous_roundtrip`, `mprotect_guard_page`).

**Blocked by**: nothing

### ~~6. Dirent partition~~ ✅

`dirent.h` — `opendir`/`readdir`/`closedir`/`dirfd`/`scandir`. `struct dirent`
has `char d_name[256]` (fixed-size array in struct). Opaque `DIR *` pointer.

Bugs discovered and fixed:
- **PtrConst mid-chain panic**: `PtrMut(PtrConst(Named, 1), 1)` from
  `const struct dirent **` put `ELEMENT_TYPE_CMOD_REQD` mid-chain in blobs,
  crashing windows-bindgen `from_blob_impl`. Fix: always emit `PtrMut`;
  const-ness tracked via `ConstAttribute` on parameters.
- **Anonymous enum names**: `enum (unnamed at dirent.h:97:1)` → invalid
  Rust type name. Fix: detect anonymous enums in `collect_enums` and
  emit their variants as standalone `ConstantDef` entries (`DT_*` constants).
- **Opaque typedef to void**: `typedef struct __dirstream DIR` maps to
  `CType::Void` which emits `c_void` (not `Copy`/`Clone`). Fix: emit
  `isize` for void-underlying typedefs.

5 E2E tests (`dt_type_constants`, `dirent_struct_size`,
`opendir_readdir_closedir_roundtrip`, `readdir_dot_entries`,
`dirfd_returns_valid_fd`).

**Blocked by**: nothing

---

## Nice-to-Have — Core Features

### 7. Bitfield attribute emission

Extraction works; emission as `NativeBitfieldAttribute` is not yet
implemented.

### 8. Flexible array member handling

`IncompleteArray` → `CType::Ptr` adds a spurious pointer-sized field,
producing incorrect struct layout. Affects `struct cmsghdr`
(`__cmsg_data[]`). Low priority — advanced socket API.

### 9. Inline function skipping

`static inline` functions in headers have no symbol in the shared
library. P/Invoke fails at runtime. Affects `htons`/`htonl` in socket
headers. Need to detect via `Entity::get_storage_class()` or similar.

---

## Not Yet Implemented (lower priority)

From [RustGenerator.md](design/RustGenerator.md):

| Feature | Complexity |
|---|---|
| Multi-header wrapper generation | Low |
| Cross-WinMD type imports (`[[type_import]]`) | Medium |
| COM interface support | Medium |
| Nested types (`NestedClass`) | Low |
