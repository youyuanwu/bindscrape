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

## Planned — bnd-posix API Families

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

### ~~9. Dlfcn partition~~ ✅

`dlfcn.h` — `dlopen`/`dlclose`/`dlsym`/`dlerror` and `RTLD_*` constants.
On glibc 2.34+, `libdl.so.2` is a stub — all symbols live in `libc.so.6`,
so uses `library = "c"`. `void*` returns emitted as `*mut c_void`.
E2E tests cover constant values, dlopen/dlclose roundtrip, dlsym lookup
(found + missing), dlerror, and casting dlsym result to callable fn pointer.

**Blocked by**: nothing

### ~~10. Errno partition~~ ✅

`errno.h` — `__errno_location()` returns `*mut i32` (pointer-to-primitive
return type), plus ~130 `E*` error constants from kernel headers.
Traverse chain: `errno.h` → `bits/errno.h` → `linux/errno.h` →
`asm/errno.h` → `asm-generic/errno.h` → `asm-generic/errno-base.h`.
E2E tests cover constant values, pointer validity, set/read roundtrip,
and errno-after-failed-syscall.

**Blocked by**: nothing

### ~~11. Sched partition~~ ✅

`sched.h` — `sched_yield`/`sched_setscheduler`/`sched_getparam` and friends,
`SCHED_*` constants, `struct sched_param`, `cpu_set_t`. `clone()` is variadic
and auto-skipped. Separated from pthread.h as an independent POSIX API.
On glibc 2.34+, sched_* symbols live in libc.
E2E tests cover constant values, sched_yield, priority range queries,
getscheduler, and struct sizes.

**Blocked by**: nothing

### ~~12. Time partition~~ ✅

`time.h` — `clock_gettime`/`nanosleep`/`gmtime`/`mktime`/`timer_create` and
friends, `CLOCK_*`/`TIMER_ABSTIME` constants, `struct tm`, `struct itimerspec`,
`clockid_t`/`timer_t` types. Separated from pthread.h as an independent
POSIX API. Rich function set (~25 functions across time manipulation,
formatting, and POSIX timers).
E2E tests cover constant values, time(), clock_gettime(MONOTONIC),
gmtime epoch zero, mktime roundtrip, difftime, struct tm layout, tzset.

**Blocked by**: nothing

### ~~13. Pthread partition~~ ✅

`pthread.h` — `pthread_create`/`pthread_join`/`pthread_mutex_*`/`pthread_cond_*`/
`pthread_rwlock_*`/`pthread_spin_*`/`pthread_barrier_*`/`pthread_key_*` and
~90 functions total. `PTHREAD_*` constants (~30). Union-based opaque types
(`pthread_mutex_t`, `pthread_cond_t`, `pthread_rwlock_t`, `pthread_barrier_t`,
`pthread_attr_t`). Function pointer parameters (`pthread_create`'s
`__start_routine`, `pthread_atfork`, `pthread_once`, `pthread_key_create`'s
destructor) emitted as `*const isize` (opaque function pointer in WinMD).
On glibc 2.34+, pthread_* symbols live in libc.

Included headers `sched.h` and `time.h` were separated into their own
partitions (11 and 12) since they are substantial independent POSIX APIs.
`bits/pthreadtypes.h` needed explicit inclusion in the traverse list —
defines all pthread union types (`pthread_mutex_t`, etc.).

E2E tests cover constant values, pthread_self, pthread_equal, mutex
init/lock/unlock/destroy, trylock (EBUSY), rwlock read/write, spinlock,
TLS key create/set/get/delete, pthread_create/join with function pointer
callback, attr init/getdetachstate/destroy, and struct sizes for all
major synchronisation types.

**Blocked by**: nothing

### Candidate API families

| Header | Partition | Why it's interesting |
|---|---|---|
| `poll.h` | `posix.poll` | Tiny clean API — `struct pollfd` with bitfield-like `short` fields, `POLLIN`/`POLLOUT` constants |
| `sys/resource.h` | `posix.resource` | `struct rlimit` with `rlim_t` typedef, `RLIMIT_*` constants |
| `dlfcn.h` | `posix.dl` | ✅ Implemented — `void*` returns, `RTLD_*` constants |
| `errno.h` | `posix.errno` | ✅ Implemented — `*mut i32` return, ~130 `E*` constants |
| `sched.h` | `posix.sched` | ✅ Implemented — scheduling API, `SCHED_*` constants, `cpu_set_t` |
| `time.h` | `posix.time` | ✅ Implemented — `struct tm`, `clock_gettime`, `CLOCK_*` constants |
| `pthread.h` | `posix.pthread` | ✅ Implemented — opaque union types, function-pointer params, ~90 functions |
| `sys/utsname.h` | `posix.utsname` | `struct utsname` with fixed-size `char[]` array fields — stress-tests array-in-struct emission |
| `termios.h` | `posix.termios` | Large struct with array fields (`c_cc[NCCS]`), many `B*`/`TC*` constants |
| `sys/wait.h` | `posix.wait` | `waitpid` is plain function; `WIFEXITED` etc. are macros wrapping bit ops — tests limits of `#define` extraction |
| `sys/ioctl.h` | `posix.ioctl` | `ioctl` itself is variadic (skipped), but `IOCTL_*` constants are complex macro expressions — tests extraction edge cases |

All high-priority candidates (delegate-as-param, void* returns, *mut i32
returns, opaque union types) are now implemented.

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

`static inline` functions in headers have no exported symbol in the shared
library — WinMD P/Invoke metadata cannot represent them. Detection is
straightforward (clang `Entity::get_storage_class()` → `Static`); the fix
is warn-and-skip, same as variadic functions. Note: `htons`/`htonl` are
**not** affected — glibc exports them as real symbols despite the inline
definition in the header.

---

## Not Yet Implemented (lower priority)

From [RustGenerator.md](design/RustGenerator.md):

| Feature | Complexity | Status |
|---|---|---|
| Multi-header wrapper generation | Low | ⬜ |
| Cross-WinMD type imports (`[[type_import]]`) | Medium | ⬜ |
| COM interface support | Medium | ⬜ |
| Inline function skipping | Low | ⬜ |
