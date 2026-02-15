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

### ~~4. Socket partitions~~ ✅

Added as 3 partitions under the existing `posix` assembly: Socket
(`sys/socket.h`), Inet (`netinet/in.h` + `arpa/inet.h`), Netdb (`netdb.h`).
Required iterative traverse path discovery — `struct iovec` in
`bits/types/struct_iovec.h`, `struct netent` in `bits/netdb.h`, constants
spread across `bits/socket.h`, `bits/socket_type.h`, and
`bits/socket-constants.h`. All `htons`/`htonl` are real symbols in glibc.
E2E tests cover constants, struct layouts, socket syscalls, byte order
functions, address conversion, and name resolution.\n\n**Blocked by**: nothing

### ~~5. Mmap partition~~ ✅

`sys/mman.h` — `mmap`/`munmap`/`mprotect`/`msync`/`madvise` and friends.
Hex constant extraction bug discovered and fixed (`parse_hex_or_suffixed_int`
helper handles `0x`, `0` octal, and `U`/`L`/`UL`/`ULL` suffixes).
All `PROT_*`, `MAP_*`, `MS_*`, `MADV_*` constants emitted. E2E tests
cover `prot_constants`, `map_constants`, `msync_constants`,
`mmap_anonymous_roundtrip`, `mprotect_guard_page`.

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

E2E tests cover `dt_type_constants`, `dirent_struct_size`,
`opendir_readdir_closedir_roundtrip`, `readdir_dot_entries`,
`dirfd_returns_valid_fd`.

**Blocked by**: nothing

### ~~7. Signal partition~~ ✅

`signal.h` — `kill`/`raise`/`signal`/`sigaction`/`sigprocmask` and friends.
Union-in-struct (`sigaction.__sigaction_handler` with `sa_handler` vs
`sa_sigaction`), function-pointer typedef (`__sighandler_t` → WinMD delegate →
`Option<unsafe extern "system" fn(i32)>`), deeply nested anonymous types
(`siginfo_t` with 8 nested unions/structs), x86 register state structs
(`sigcontext`, `_fpstate`).

Challenges:
- **Deep include graph**: 10 sub-headers across `bits/` and `bits/types/`;
  each missing traverse path causes windows-bindgen panic.
- **Function/struct name collision**: `sigstack` is both a function and a
  struct — required adding `bits/types/struct_sigstack.h` to traverse.
- **Cross-partition reference**: `sigtimedwait` uses `stat::timespec`,
  auto-gated by `#[cfg(feature = "stat")]`.

E2E tests cover constants (SIG_*, SA_*), struct layouts (sigaction,
__sigset_t, siginfo_t, stack_t), sigset operations (sigemptyset/sigfillset/
sigaddset/sigdelset/sigismember), signal delivery (raise + handler),
sigaction install, sigprocmask block/pending, and kill(self, 0).

**Blocked by**: nothing

### ~~8. Types partition~~ ✅

`sys/types.h` — shared POSIX typedefs (`uid_t`, `pid_t`, `mode_t`, `off_t`,
`gid_t`, `ssize_t`, `ino_t`, `dev_t`, `nlink_t`, `blksize_t`, `blkcnt_t`, …).
Centralises ~95 typedefs and 1 struct (`__fsid_t`) into a dedicated
`posix.types` partition so other partitions reference them via cross-partition
`TypeRef` instead of duplicating definitions.

Challenges:
- **`__fsid_t` not found**: `sys/types.h` has `typedef __fsid_t fsid_t` but
  the struct is defined in `bits/types.h` via macro. Fix: add `bits/types.h`
  to the traverse list (pulls in ~60 internal `__` typedefs, harmless).
- **First-writer-wins registry**: `build_type_registry` uses first-writer-wins
  for typedefs — the types partition comes first in the TOML, so it registers
  `uid_t` etc. before other partitions see them. The dedup pass
  (`partition.typedefs.retain(…)`) then strips duplicates from later partitions.
- **Cross-partition `#[cfg]` gates**: windows-bindgen auto-generates
  `#[cfg(feature = "types")]` on references in other modules (39 in unistd,
  32 in signal, 20 in stat, 16 in socket, etc.).

No E2E tests — typedef-only partition with no callable functions.

**Blocked by**: nothing

### Candidate API families

| Header | Partition | Why it's interesting |
|---|---|---|
| `poll.h` | `posix.poll` | Tiny clean API — `struct pollfd` with bitfield-like `short` fields, `POLLIN`/`POLLOUT` constants |
| `sys/resource.h` | `posix.resource` | `struct rlimit` with `rlim_t` typedef, `RLIMIT_*` constants |
| `dlfcn.h` | `posix.dl` | `void*` returns (`dlopen`, `dlsym`), `RTLD_*` constants, linked to `libdl` not `libc` (tests non-`c` library field) |
| `sys/utsname.h` | `posix.utsname` | `struct utsname` with fixed-size `char[]` array fields — stress-tests array-in-struct emission |
| `termios.h` | `posix.termios` | Large struct with array fields (`c_cc[NCCS]`), many `B*`/`TC*` constants |
| `pthread.h` | `posix.pthread` | Opaque types (`pthread_t`, `pthread_mutex_t`), function-pointer params (`pthread_create`'s start routine) |
| `time.h` | `posix.time` | `struct tm` (many fields), `clock_gettime` with `clockid_t`, `CLOCK_*` constants |

Priority: **poll.h** (quick win, complements socket testing), **dlfcn.h** (non-libc linkage),
**time.h** (classic struct, trivially testable).

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

| Feature | Complexity | Status |
|---|---|---|
| Multi-header wrapper generation | Low | ⬜ |
| Cross-WinMD type imports (`[[type_import]]`) | Medium | ⬜ |
| COM interface support | Medium | ⬜ |
| Inline function skipping | Low | ⬜ |
