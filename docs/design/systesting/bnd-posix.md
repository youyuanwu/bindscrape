# bnd-posix: System Header Testing

Design notes for the POSIX API families tested through the `bnd-posix` crate.
Each section documents partition layout, expected challenges, API surface,
and E2E test plans for one header group.

| API family | Status | Key feature exercised |
|---|---|---|
| [posix](#posix--file-io) | ✅ Implemented | System typedefs, variadic skipping, `struct stat` |
| [Mmap](#mmap) | ✅ Implemented | Hex constant extraction, `void *` return |
| [Dirent](#dirent) | ✅ Implemented | Anonymous enums, opaque typedefs, PtrConst fix |
| [Sockets](#sockets) | ✅ Implemented | 3 partitions under posix (socket, inet, netdb) |
| [Signal](#signal) | ✅ Implemented | Union-in-struct, function-pointer delegate, deep include graph |
| [Types](#types) | ✅ Implemented | Shared POSIX typedefs, first-writer-wins dedup |
| [Dlfcn](#dlfcn) | ✅ Implemented | `void*` returns, `RTLD_*` constants, non-libc linkage (glibc 2.34+ uses libc) |
| [Errno](#errno) | ✅ Implemented | `*mut i32` return type, ~130 kernel `E*` constants, deep kernel header traverse |
| [Sched](#sched) | ✅ Implemented | Scheduling API, `cpu_set_t` struct, `SCHED_*` constants |
| [Time](#time) | ✅ Implemented | `struct tm`, `clock_gettime`, `CLOCK_*` constants, POSIX timers |
| [Pthread](#pthread) | ✅ Implemented | Opaque union types, function-pointer params, ~90 functions, cross-partition `__sigset_t` |

---

## posix — File I/O

Validate bnd-winmd against **POSIX file I/O headers** — `<fcntl.h>`,
`<unistd.h>`, and `<sys/stat.h>`. This exercises many system typedefs
(`mode_t`, `uid_t`, `pid_t`, `time_t`, etc.), variadic functions (`open`),
large/complex structs (`struct stat`), and a dense `#define` constant
space (`O_RDONLY`, `S_IRUSR`, etc.).

### Why File I/O

- **Always available** — no additional `-dev` package needed
- **Many new system typedefs**: `mode_t`, `uid_t`, `gid_t`, `pid_t`,
  `time_t`, `dev_t`, `ino_t`, `nlink_t`, `blksize_t`, `blkcnt_t` — all
  auto-resolved via clang's canonical types (stored in `CType::Named { resolved }`)
- **Variadic function**: `open(const char *path, int flags, ...)` —
  automatically skipped by `collect_functions()` via `Entity::is_variadic()`
- **Large struct**: `struct stat` has 13+ fields with mixed typedef types
- **Dense `#define` constants**: `O_RDONLY`, `O_WRONLY`, `O_CREAT`,
  `O_TRUNC`, `S_IRUSR`, `S_IWUSR`, `S_IRGRP`, etc. — tests the constant
  extraction at scale
- **Straightforward E2E testing**: `creat`/`write`/`read`/`close`/`stat`
  on a temp file is deterministic and safe

---

### Headers & Partitions

#### Arch-Specific Header Paths (Critical)

On Debian/Ubuntu x86-64, clang resolves system headers through
`/usr/include/x86_64-linux-gnu` **before** `/usr/include`:

```
#include <...> search starts here:
 /usr/lib/llvm-18/lib/clang/18/include
 /usr/local/include
 /usr/include/x86_64-linux-gnu        ← arch-specific, searched first
 /usr/include                          ← generic
End of search list.
```

This means:
- `<sys/stat.h>` resolves to `/usr/include/x86_64-linux-gnu/sys/stat.h`
- `<sys/types.h>` resolves to `/usr/include/x86_64-linux-gnu/sys/types.h`
- `<fcntl.h>` resolves to `/usr/include/fcntl.h` (generic, no arch override)
- `<unistd.h>` resolves to `/usr/include/unistd.h` (generic)

The traverse paths must match what clang resolves, otherwise `should_emit`
location checks will fail. Using `include_paths` with the arch-specific
directory first ensures `resolve_header()` produces matching paths.

#### Where Declarations Actually Live (Verified on Ubuntu 24.04)

| Declaration | Clang-resolved location | Notes |
|---|---|---|
| `open()` | `/usr/include/fcntl.h:209` | Variadic: `int (const char *, int, ...)` |
| `creat()` | `/usr/include/fcntl.h:255` | Non-variadic: `int (const char *, mode_t)` |
| `O_RDONLY` | `/usr/include/x86_64-linux-gnu/bits/fcntl-linux.h` | Sub-header, NOT in `fcntl.h` |
| `read()`, `write()`, `close()` | `/usr/include/unistd.h` | Standard locations |
| `lseek()` | `/usr/include/unistd.h:339` | Returns `__off_t`, not `off_t` |
| `getpid()` | `/usr/include/unistd.h:650` | Returns `__pid_t` |
| `SEEK_SET` | `/usr/include/stdio.h:110` | NOT in `unistd.h` |
| `struct stat` | `/usr/include/x86_64-linux-gnu/bits/struct_stat.h` | Sub-header, NOT in `sys/stat.h` |
| `stat()`, `fstat()`, `chmod()` | `/usr/include/x86_64-linux-gnu/sys/stat.h` | Functions in main header |
| `S_IRUSR` | `/usr/include/x86_64-linux-gnu/sys/stat.h:168` | `#define S_IRUSR __S_IREAD` |
| `mode_t`, `uid_t`, etc. | `/usr/include/x86_64-linux-gnu/sys/types.h` | Arch-specific types.h |
| `time_t` | `/usr/include/x86_64-linux-gnu/bits/types/time_t.h` | Separate sub-header |

#### Sub-Header Problem

Several key declarations live in `bits/` sub-headers, not in the
top-level header that users `#include`:

- **`struct stat`** → `bits/struct_stat.h`
- **`O_RDONLY`, `O_CREAT`** → `bits/fcntl-linux.h`
- **`SEEK_SET`** → not in `unistd.h` at all; lives in `stdio.h`
  and `linux/fs.h`

The traverse list must include these sub-headers to capture the
declarations, or we accept they won't be extracted. For constants,
sonar's `find_definitions` operates on macro definitions which ARE
visible even from sub-headers (they get `#include`-expanded into the
translation unit). The traverse filter only applies to `Entity` location
checks, not to macro enumeration — **needs verification**.

#### Partition Layout

```toml
include_paths = [
    "/usr/include/x86_64-linux-gnu",
    "/usr/include",
]

[output]
name = "posix"
file = "bnd-posix.winmd"

# Partition 1: fcntl — creat + O_* flags
# open/openat/fcntl are variadic and will be auto-skipped
[[partition]]
namespace = "posix.fcntl"
library = "c"
headers = ["fcntl.h"]
traverse = ["fcntl.h", "bits/fcntl-linux.h"]

# Partition 2: unistd — read/write/close/lseek
[[partition]]
namespace = "posix.unistd"
library = "c"
headers = ["unistd.h"]
traverse = ["unistd.h"]

# Partition 3: sys/stat — struct stat + stat/fstat/chmod + S_* constants
[[partition]]
namespace = "posix.stat"
library = "c"
headers = ["sys/stat.h"]
traverse = [
    "sys/stat.h",
    "bits/struct_stat.h",              # struct stat definition
    "bits/types/struct_timespec.h",    # struct timespec for st_atim etc.
]
```

Key points:
- **No `posix.types` partition** — system typedefs are auto-resolved
  by clang canonical types stored in `CType::Named { resolved }`. A
  separate `sys/types.h` partition is unnecessary and extracts ~33 noisy
  typedefs including `__fsid_t` (anonymous struct) that windows-bindgen
  cannot handle.
- **`include_paths`** resolves relative header names — arch-specific dir
  first so `sys/stat.h` → `/usr/include/x86_64-linux-gnu/sys/stat.h`
  (matches what clang resolves)
- **`library = "c"`** — functions live in libc (`libc.so.6`)
- **Sub-header traverse entries** — `bits/fcntl-linux.h` for O_* constants,
  `bits/struct_stat.h` for `struct stat`,
  `bits/types/struct_timespec.h` for `struct timespec`
- **`SEEK_SET`** happens to be extracted from `fcntl.h`'s includes,
  and also appears in `unistd.h`
- **No extra packages** — all headers are part of `libc6-dev`
- **Namespace modules** (no `--flat` in windows-bindgen) — prevents
  cross-partition duplicate definitions from conflicting

---

### System Typedefs (Auto-Resolved)

These typedefs appear in `struct stat` fields and function signatures.
They are **automatically resolved** by clang's `get_canonical_type()` —
no hardcoded table needed. At extraction time, `CType::Named { resolved }`
stores the canonical primitive, and at emit time the resolved type is
used as fallback when the name isn't in the `TypeRegistry`.

Note: clang/glibc function signatures use `__`-prefixed internal names
(`__mode_t`, `__off_t`, `__pid_t`). Both variants are handled by the
same mechanism — clang resolves the canonical type regardless of name.

| Typedef | Internal name | Canonical type | Auto-resolved to |
|---|---|---|---|
| `mode_t` | `__mode_t` | `unsigned int` | `U32` |
| `uid_t` | `__uid_t` | `unsigned int` | `U32` |
| `gid_t` | `__gid_t` | `unsigned int` | `U32` |
| `pid_t` | `__pid_t` | `int` | `I32` |
| `time_t` | `__time_t` | `long` | `I64` |
| `dev_t` | `__dev_t` | `unsigned long` | `U64` |
| `ino_t` | `__ino_t` | `unsigned long` | `U64` |
| `nlink_t` | `__nlink_t` | `unsigned long` | `U64` |
| `blksize_t` | `__blksize_t` | `long` | `I64` |
| `blkcnt_t` | `__blkcnt_t` | `long` | `I64` |
| `clockid_t` | `__clockid_t` | `int` | `I32` |

Note: These sizes are **Linux x86-64** (LP64 ABI). `unsigned long` is
8 bytes here. The type mapping matches the host platform.

---

### Challenges

#### 1. Variadic functions (`open`) — ✅ Resolved

`open(const char *pathname, int flags, ...)` is variadic.
`collect_functions()` now checks `Entity::is_variadic()` and skips
with a warning. The E2E tests use `creat()` (non-variadic) instead.
`fcntl()` and `openat()` are also variadic and automatically skipped.

#### 2. `struct stat` in sub-header

`struct stat` is defined in `bits/struct_stat.h`, not `sys/stat.h`.
The traverse list must include the sub-header. Clang reports the entity
location as the sub-header path, so `should_emit` needs to match
against the resolved sub-header path.

The struct has ~13 fields with glibc-internal reserved fields
(`__pad0`, `__glibc_reserved`). Field names will include these
internals. `struct stat` also uses `struct timespec` for `st_atim`
etc., which needs handling.

#### 3. Constants in sub-headers

`O_RDONLY` and friends are `#define`d in `bits/fcntl-linux.h`.
`sonar::find_definitions` operates on macro definitions in the
translation unit — need to verify whether the traverse/location
filter applies to macro definitions or only to entity declarations.
If macros bypass the location filter (since they're preprocessor
directives), the sub-header traverse entry may be unnecessary for
constants.

#### 4. `SEEK_SET` not in `unistd.h`

`SEEK_SET`, `SEEK_CUR`, `SEEK_END` are defined in `<stdio.h>` and
`<linux/fs.h>`, not in `<unistd.h>`. If the E2E tests need these
constants, they must come from a `stdio.h` partition or be defined
manually. Alternative: the E2E test Rust code can define these values
directly as Rust constants.

#### 5. `S_ISREG` / `S_ISDIR` — function-like macros

These are `#define S_ISREG(m) (((m) & S_IFMT) == S_IFREG)` — not
extractable as constants. `sonar::find_definitions` will see them but
`evaluate()` will fail (they take an argument). Skipped automatically.

#### 6. Inline functions in headers

`<unistd.h>` may contain `static inline` functions (glibc versions
vary). These would be extracted as regular functions but have no symbol
in `libc.so`. The P/Invoke would fail at runtime. Need to detect and
skip via `Entity::get_storage_class()` or similar.

#### 7. `__` prefixed internal typedefs — ✅ Resolved

glibc function signatures use `__mode_t`, `__off_t`, `__pid_t`, etc.
(verified: `lseek` returns `__off_t`, `getpid` returns `__pid_t`).
These are automatically handled by clang canonical type resolution —
`CType::Named { name: "__mode_t", resolved: Some(U32) }`. No hardcoded
table needed. The user-facing names (`mode_t`, `uid_t` etc.) may appear
as extracted typedefs from the `sys/types.h` partition if they pass
the sonar/collect filters.

#### 8. `struct timespec` nested in `struct stat` — ✅ Resolved

`struct stat` fields `st_atim`, `st_mtim`, `st_ctim` are of type
`struct timespec` (defined in `bits/types/struct_timespec.h`).
Adding this sub-header to the Stat partition's traverse list extracts
the struct and allows windows-bindgen to resolve the field types.

#### 9. Array parameter decay — ✅ Resolved

Functions like `futimens(int fd, const struct timespec t[2])` have
C array parameters. In C semantics, array parameters always decay
to pointers. However, the winmd ELEMENT_TYPE_ARRAY encoding breaks
windows-bindgen's blob reader (it doesn't consume all ArrayShape fields,
leaving stray bytes). Fixed by decaying `CType::Array` → `CType::Ptr`
in `extract_function()` for parameters.

#### 10. Duplicate function declarations (`__REDIRECT`) — ✅ Resolved

glibc uses `__REDIRECT` macros to alias function names (e.g. `lockf`
redirected to `lockf64`). This produces multiple clang declarations of
the same function name. Fixed by deduplicating in `collect_functions()`
with a `HashSet<String>` on the function name.

#### 11. Cross-partition type duplicates — ✅ Resolved

Typedefs like `off_t`, `mode_t`, constants like `SEEK_SET`, `R_OK`
appear in multiple partitions. Using namespace modules (no `--flat`)
separates them into distinct Rust modules (`posix::fcntl::off_t`
vs `posix::unistd::off_t`), avoiding compilation errors.

---

### API Surface

#### posix.fcntl (fcntl.h + bits/fcntl-linux.h)

**Functions (4)**: `creat`, `lockf`, `posix_fadvise`, `posix_fallocate`
(skipping variadic `open`, `fcntl`, `openat`)
**Constants (60)**: `O_RDONLY`, `O_WRONLY`, `O_RDWR`, `O_CREAT`, `O_TRUNC`,
`O_APPEND`, `O_EXCL`, `O_NONBLOCK`, `AT_FDCWD`, `SEEK_SET`, `SEEK_CUR`,
`SEEK_END`, `R_OK`, `W_OK`, `X_OK`, `F_OK`, ...
**Typedefs (3)**: `mode_t`, `off_t`, `pid_t`

#### posix.unistd (unistd.h)

**Functions (103)**: `read`, `write`, `close`, `lseek`, `ftruncate`, `unlink`,
`access`, `getpid`, `dup`, `dup2`, `pipe`, `fsync`, `fork`, `execv`, ...
(variadic `execl`, `execle`, `execlp`, `syscall` automatically skipped)
**Constants (23)**: `STDIN_FILENO`, `STDOUT_FILENO`, `STDERR_FILENO`,
`SEEK_SET`, `SEEK_CUR`, `SEEK_END`, `R_OK`, `W_OK`, `X_OK`, `F_OK`, ...
**Typedefs (8)**: `gid_t`, `intptr_t`, `off_t`, `pid_t`, `socklen_t`,
`ssize_t`, `uid_t`, `useconds_t`

#### posix.stat (sys/stat.h + bits/struct_stat.h + bits/types/struct_timespec.h)

**Structs (2)**: `stat` (15 fields, 144 bytes on x86-64), `timespec` (2 fields, 16 bytes)
**Functions (17)**: `stat`, `fstat`, `lstat`, `fstatat`, `chmod`, `lchmod`,
`fchmod`, `fchmodat`, `mkdir`, `mkdirat`, `mkfifo`, `mkfifoat`,
`mknod`, `mknodat`, `umask`, `utimensat`, `futimens`
**Constants (4)**: `S_BLKSIZE`, `_BITS_STRUCT_STAT_H`, `_STRUCT_TIMESPEC`,
`_SYS_STAT_H`
**Typedefs (7)**: `dev_t`, `gid_t`, `ino_t`, `mode_t`, `nlink_t`, `off_t`, `uid_t`

Note: `S_IRUSR`, `S_IWUSR`, etc. are `#define S_IRUSR __S_IREAD` —
macro-to-macro definitions that `sonar::find_definitions` cannot evaluate.
These are NOT extracted as constants.

---

### E2E Tests

Test against real filesystem operations using temp files.

| Test | What it does |
|---|---|
| `creat_and_close` | `creat(tmppath, 0o644)` returns valid fd, `close(fd)` returns 0 |
| `write_then_read` | Write "hello" to tmpfile, lseek to start, read back, assert equal |
| `stat_file_size` | Write 13 bytes, `fstat(fd)` → `st_size == 13` |
| `stat_is_regular_file` | `fstat(fd)` → `st_mode & S_IFREG != 0` |
| `unlink_file` | `unlink(tmppath)` returns 0 |
| `lseek_returns_offset` | `lseek(fd, 10, SEEK_SET)` returns 10 (define SEEK_SET=0 locally) |
| `access_existing_file` | `access(tmppath, F_OK)` returns 0 |
| `access_nonexistent_file` | `access("/nonexistent", F_OK)` returns -1 |
| `getpid_returns_positive` | `getpid()` > 0 |
| `o_rdonly_is_zero` | `O_RDONLY == 0` |
| `s_irusr_is_0o400` | `S_IRUSR == 0o400` |
| `stat_struct_size` | `size_of::<stat>() > 0` |

---

### Dependencies

- No additional packages — `sys/types.h`, `fcntl.h`, `unistd.h`,
  `sys/stat.h` are part of `libc6-dev` (already present if
  `libclang-dev` is installed)
- libc is implicitly linked — `cargo:rustc-link-lib=dylib=c` may not
  even be necessary, but explicit is safer

---

### Implementation Steps

1. ✅ System typedefs auto-resolved via `CType::Named { resolved }` —
   no hardcoded table needed
2. ✅ Variadic functions warn-and-skip via `Entity::is_variadic()`
3. ✅ C `long` → `I64` for Linux LP64 ABI
4. ✅ Array parameter decay → pointer in `extract_function()`
5. ✅ Function deduplication via `HashSet` in `collect_functions()`
6. ✅ Created `bnd-posix-gen/bnd-posix.toml`
   (5 partitions: Dirent, Fcntl, Mmap, Stat, Unistd)
7. ✅ Added roundtrip tests in `roundtrip_posixfile.rs`
8. ✅ Created `bnd-posix/` crate with feature-gated namespace modules
   (package mode via `bnd-posix-gen`, no `build.rs`)
9. ✅ Added `struct_timespec.h` to Stat traverse list
10. ✅ Created E2E tests (`posixfile_e2e.rs`) — all passing
11. ✅ Added `bnd-posix` and `bnd-posix-gen` to workspace members
12. ✅ Separated generator into `bnd-posix-gen` crate using
   `windows-bindgen --package` mode

---

## Mmap — ✅ Implemented

`sys/mman.h` — memory mapping APIs. Added as Partition 4.

### Partition Config

```toml
[[partition]]
namespace = "posix.mmap"
library = "c"
headers = ["sys/mman.h"]
traverse = ["sys/mman.h", "bits/mman-linux.h", "bits/mman-map-flags-generic.h"]
```

### Challenges Solved

**Hex constant extraction**: sonar's `find_definitions` uses
`u64::from_str()` which only parses decimal. Constants like
`PROT_READ 0x1`, `MAP_SHARED 0x01` were silently dropped. Fixed
by adding a supplemental pass in `collect_constants` that iterates
`MacroDefinition` entities, tokenizes them, and calls
`parse_hex_or_suffixed_int()` — handles `0x` hex, `0` octal, and
trailing `U`/`L`/`UL`/`ULL` suffixes.

### API Surface

**Functions (13)**: `mmap`, `munmap`, `mprotect`, `msync`, `madvise`,
`mlock`, `munlock`, `mlockall`, `munlockall`, `mincore`,
`posix_madvise`, `shm_open`, `shm_unlink`
**Constants (~30)**: `PROT_READ`, `PROT_WRITE`, `PROT_EXEC`, `PROT_NONE`,
`MAP_SHARED`, `MAP_PRIVATE`, `MAP_ANONYMOUS`, `MAP_FIXED`,
`MS_ASYNC`, `MS_SYNC`, `MS_INVALIDATE`, `MADV_*`, etc.

### E2E Tests

| Test | What it does |
|---|---|
| `prot_constants` | `PROT_READ=1`, `PROT_WRITE=2`, `PROT_EXEC=4`, `PROT_NONE=0` |
| `map_constants` | `MAP_SHARED=1`, `MAP_PRIVATE=2`, `MAP_ANONYMOUS=32`, `MAP_FIXED=16` |
| `msync_constants` | `MS_ASYNC=1`, `MS_SYNC=4`, `MS_INVALIDATE=2` |
| `mmap_anonymous_roundtrip` | `mmap(MAP_ANONYMOUS)` → write → read → `munmap` |
| `mprotect_guard_page` | `mmap` → `mprotect(PROT_READ)` → `mprotect(PROT_READ\|PROT_WRITE)` → `munmap` |

---

## Dirent — ✅ Implemented

`dirent.h` — directory entry APIs. Added as Partition 5.

### Partition Config

```toml
[[partition]]
namespace = "posix.dirent"
library = "c"
headers = ["dirent.h"]
traverse = ["dirent.h", "bits/dirent.h"]
```

### Challenges Solved

1. **PtrConst mid-chain panic**: `const struct dirent **` produces
   `PtrMut(PtrConst(Named("dirent"), 1), 1)` which puts
   `ELEMENT_TYPE_CMOD_REQD` mid-chain in the blob, crashing
   windows-bindgen's `from_blob_impl`. Fix: always emit `PtrMut`;
   mutability preserved via `ParamAttributes::Out` on mutable pointer
   parameters.

2. **Anonymous enum names**: `enum (unnamed at dirent.h:97:1)` — the
   unnamed enum containing `DT_UNKNOWN`, `DT_FIFO`, etc. generates
   invalid Rust type names. Fix: detect anonymous enums in
   `collect_enums` and emit their variants as standalone `ConstantDef`
   entries.

3. **Opaque typedef to void**: `typedef struct __dirstream DIR` — the
   underlying `struct __dirstream` is incomplete, so clang resolves it
   to `CType::Void`. Emitting `c_void` produces a struct that doesn't
   implement `Copy`/`Clone`/`Default`. Fix: emit `isize` for
   void-underlying typedefs.

### API Surface

**Functions (11)**: `opendir`, `closedir`, `readdir`, `readdir_r`,
`rewinddir`, `seekdir`, `telldir`, `dirfd`, `fdopendir`, `scandir`,
`alphasort`, `getdirentries`
**Structs (1)**: `dirent` (280 bytes — `d_ino`, `d_off`, `d_reclen`,
`d_type`, `d_name[256]`)
**Types**: `DIR` (opaque handle, emitted as `isize`)
**Constants (~11)**: `DT_UNKNOWN=0`, `DT_FIFO=1`, `DT_CHR=2`,
`DT_DIR=4`, `DT_BLK=6`, `DT_REG=8`, `DT_LNK=10`, `DT_SOCK=12`,
`DT_WHT=14`

### E2E Tests

| Test | What it does |
|---|---|
| `dt_type_constants` | `DT_UNKNOWN=0`, `DT_DIR=4`, `DT_REG=8`, etc. |
| `dirent_struct_size` | `size_of::<dirent>() == 280` |
| `opendir_readdir_closedir_roundtrip` | Open `/tmp`, read entry, verify `d_ino != 0`, close |
| `readdir_dot_entries` | Read `/tmp`, find `.` and `..` entries with `d_type == DT_DIR` |
| `dirfd_returns_valid_fd` | `dirfd(opendir("/tmp"))` returns valid fd ≥ 0 |

---

## Sockets

Validate bnd-winmd against **POSIX socket headers** — `<sys/socket.h>`,
`<netinet/in.h>`, `<arpa/inet.h>`, and `<netdb.h>`. This is the next
system header target. Union support and anonymous nested type naming —
previously blockers — are now implemented and tested.

### Why Sockets

- **Unions**: `struct in6_addr` contains an anonymous union with three
  members (`__u6_addr8`, `__u6_addr16`, `__u6_addr32`). (✅ union support
  and anonymous type naming now implemented)
- **Anonymous nested types**: `in6_addr.__in6_u` is an anonymous union
  member — extracted with synthetic name `in6_addr_FieldName`
  (✅ `try_extract_anonymous_field()` implemented)
- **New system typedefs**: `socklen_t`, `sa_family_t`, `in_port_t`,
  `in_addr_t` — auto-resolved via clang canonical types (no table needed)
- **Packed / specific-layout structs**: `sockaddr_in` has a very specific
  layout (16 bytes, `sin_family` at offset 0, `sin_port` at offset 2,
  `sin_addr` at offset 4, `sin_zero` padding)
- **No additional packages needed** — socket headers are part of base
  `libc6-dev`
- **Testable E2E**: `socket`/`bind`/`inet_pton`/`getsockname`/`close`
  are safe, deterministic operations that don't require network access

### Headers & Partitions

#### Headers Involved

| Header | Key declarations |
|---|---|
| `<sys/socket.h>` | `struct sockaddr`, `socket()`, `bind()`, `listen()`, `accept()`, `connect()`, `send()`, `recv()`, `setsockopt()`, `getsockname()`, `AF_INET`, `AF_INET6`, `AF_UNIX`, `SOCK_STREAM`, `SOCK_DGRAM`, `SOL_SOCKET`, `SO_REUSEADDR` |
| `<netinet/in.h>` | `struct sockaddr_in`, `struct sockaddr_in6`, `struct in_addr`, `struct in6_addr`, `IPPROTO_TCP`, `IPPROTO_UDP`, `INADDR_ANY`, `INADDR_LOOPBACK`, `htons()`, `htonl()`, `ntohs()`, `ntohl()` |
| `<arpa/inet.h>` | `inet_pton()`, `inet_ntop()`, `inet_addr()` |
| `<netdb.h>` | `struct addrinfo`, `getaddrinfo()`, `freeaddrinfo()`, `gai_strerror()`, `AI_PASSIVE`, `AI_CANONNAME` |

#### Partition Layout (4-partition)

```toml
[output]
name = "posix"
file = "posixsocket.winmd"

# Partition 1: socket types and core API
[[partition]]
namespace = "posix.socket"
library = "c"
headers = ["/usr/include/sys/socket.h"]
traverse = ["/usr/include/sys/socket.h"]

# Partition 2: IPv4/IPv6 structs and constants
[[partition]]
namespace = "posix.inet"
library = "c"
headers = ["/usr/include/netinet/in.h"]
traverse = ["/usr/include/netinet/in.h"]

# Partition 3: address conversion functions
[[partition]]
namespace = "posix.arpa"
library = "c"
headers = ["/usr/include/arpa/inet.h"]
traverse = ["/usr/include/arpa/inet.h"]

# Partition 4: name resolution
[[partition]]
namespace = "posix.netdb"
library = "c"
headers = ["/usr/include/netdb.h"]
traverse = ["/usr/include/netdb.h"]
```

#### Alternative: 2-Partition Layout

```toml
# Partition 1: sys/socket.h types + functions
[[partition]]
namespace = "posix.socket"
library = "c"
headers = ["/usr/include/sys/socket.h"]
traverse = ["/usr/include/sys/socket.h"]

# Partition 2: inet + arpa + netdb
[[partition]]
namespace = "posix.inet"
library = "c"
headers = [
    "/usr/include/netinet/in.h",
    "/usr/include/arpa/inet.h",
    "/usr/include/netdb.h",
]
traverse = [
    "/usr/include/netinet/in.h",
    "/usr/include/arpa/inet.h",
    "/usr/include/netdb.h",
]
```

### New Features Required

#### Union Support (✅ Implemented)

Union support is complete. `StructDef.is_union` flag drives
`ExplicitLayout` + `FieldLayout(offset=0)` emission. The supplemental
pass in `collect_structs` detects `EntityKind::UnionDecl`. Tested with
`Value` union in `simple.h` and `NetAddr_addr` anonymous union.

#### Anonymous Nested Types (✅ Implemented)

`try_extract_anonymous_field()` detects anonymous record fields via
`Entity::is_anonymous()` on the canonical type's declaration. Recursive
extraction with synthetic names (`ParentName_FieldName`). Tested with
`NetAddr` struct containing anonymous union field `addr` → extracted as
`NetAddr_addr`.

For `struct in6_addr`, the anonymous union member `__in6_u` would be
extracted as `in6_addr___in6_u` (or similar synthetic name).

#### New System Typedefs

| Typedef | Canonical type | Winmd mapping |
|---|---|---|
| `socklen_t` | `unsigned int` | `U32` |
| `sa_family_t` | `unsigned short` | `U16` |
| `in_port_t` | `uint16_t` | `U16` |
| `in_addr_t` | `uint32_t` | `U32` |

---

### Challenges

#### 1. Union detection in `clang` crate — ✅ Resolved

The supplemental pass in `collect_structs` detects
`EntityKind::UnionDecl` directly. `sonar` has no `find_unions()`, so the
supplemental pass handles all unions. For anonymous unions nested inside
structs, `try_extract_anonymous_field()` walks struct children and
detects `is_anonymous() == true`.

#### 2. `sockaddr` family polymorphism

The C pattern of casting between `sockaddr*`, `sockaddr_in*`, and
`sockaddr_in6*` doesn't translate to winmd. Each is a separate
TypeDef. Callers must use the specific struct and cast the pointer.
This is fine — it matches how `windows-bindgen` handles Windows socket
APIs.

#### 3. `htons` / `htonl` — macros or inline functions

On Linux, `htons()` and friends may be `#define` macros calling
`__bswap_16` or may be `static inline` functions. If they resolve to
inline functions, they won't have symbols in `libc.so` and the P/Invoke
would fail at runtime. May need to skip these and test with
`inet_pton`/`inet_ntop` instead.

#### 4. `struct addrinfo` — linked list with self-referential pointer

```c
struct addrinfo {
    int              ai_flags;
    int              ai_family;
    int              ai_socktype;
    int              ai_protocol;
    socklen_t        ai_addrlen;
    struct sockaddr *ai_addr;
    char            *ai_canonname;
    struct addrinfo *ai_next;  // self-referential pointer
};
```

The `ai_next` field is a pointer to the same struct type. This should
work — it's just `CType::Ptr { pointee: Named("addrinfo") }` and the
TypeRef resolves to the same TypeDef. But worth explicit testing.

#### 5. `__SOCKADDR_COMMON` macro

glibc defines `struct sockaddr` using a macro:
```c
#define __SOCKADDR_COMMON(sa_prefix) sa_family_t sa_prefix##family
struct sockaddr {
    __SOCKADDR_COMMON(sa_);  // expands to: sa_family_t sa_family;
    char sa_data[14];
};
```

libclang resolves macros before the AST is visible, so this should be
transparent. But if the macro introduces unexpected field names, the
tests will catch it.

#### 6. Conditional compilation / `#ifdef`

Socket headers use `#ifdef __USE_GNU`, `#ifdef __USE_MISC`, etc. to
expose additional APIs. The default clang parse may or may not define
these. The set of extracted functions may vary. Could require
`clang_args = ["-D__USE_GNU"]` in the config to get the full API.

#### 7. `bits/` sub-headers

As with file I/O headers, the actual constants (`AF_INET`, `SOCK_STREAM`)
may be defined in `<bits/socket.h>` or `<asm/socket.h>`, not in
`<sys/socket.h>` directly. The traverse list may need to include these
sub-headers, or the constants won't be extracted.

---

### API Surface

#### posix.socket (sys/socket.h)

**Structs**: `sockaddr` (16 bytes — `sa_family` + `sa_data[14]`)
**Functions**: `socket`, `bind`, `listen`, `accept`, `connect`, `send`,
`recv`, `sendto`, `recvfrom`, `setsockopt`, `getsockopt`, `getsockname`,
`getpeername`, `shutdown`, `close` (if re-exported)
**Constants**: `AF_INET`, `AF_INET6`, `AF_UNIX`, `AF_UNSPEC`,
`SOCK_STREAM`, `SOCK_DGRAM`, `SOCK_RAW`, `SOL_SOCKET`, `SO_REUSEADDR`,
`SO_REUSEPORT`, `SO_KEEPALIVE`, `SHUT_RD`, `SHUT_WR`, `SHUT_RDWR`

#### posix.inet (netinet/in.h)

**Structs**: `in_addr` (4 bytes), `in6_addr` (16 bytes, contains union),
`sockaddr_in` (16 bytes), `sockaddr_in6` (28 bytes)
**Constants**: `IPPROTO_TCP`, `IPPROTO_UDP`, `IPPROTO_IP`,
`INADDR_ANY`, `INADDR_LOOPBACK`, `INADDR_BROADCAST`,
`INET_ADDRSTRLEN`, `INET6_ADDRSTRLEN`

#### posix.arpa (arpa/inet.h)

**Functions**: `inet_pton`, `inet_ntop`, `inet_addr`, `inet_ntoa`

#### posix.netdb (netdb.h)

**Structs**: `addrinfo` (self-referential linked list)
**Functions**: `getaddrinfo`, `freeaddrinfo`, `gai_strerror`
**Constants**: `AI_PASSIVE`, `AI_CANONNAME`, `AI_NUMERICHOST`,
`AI_NUMERICSERV`, `NI_MAXHOST`, `NI_MAXSERV`

---

### E2E Tests

Test using loopback operations — no network access needed.

| Test | What it does |
|---|---|
| `socket_create_tcp` | `socket(AF_INET, SOCK_STREAM, 0)` returns valid fd ≥ 0 |
| `socket_create_udp` | `socket(AF_INET, SOCK_DGRAM, 0)` returns valid fd ≥ 0 |
| `socket_close` | `close(socket_fd)` returns 0 |
| `bind_loopback` | Bind to `127.0.0.1:0`, `getsockname` returns assigned port |
| `inet_pton_ipv4` | `inet_pton(AF_INET, "127.0.0.1", &addr)` returns 1 |
| `inet_pton_ipv6` | `inet_pton(AF_INET6, "::1", &addr6)` returns 1 |
| `inet_ntop_roundtrip` | `pton` then `ntop` → same string |
| `sockaddr_in_size` | `size_of::<sockaddr_in>() == 16` |
| `sockaddr_in6_size` | `size_of::<sockaddr_in6>() == 28` |
| `in6_addr_size` | `size_of::<in6_addr>() == 16` |
| `af_inet_value` | `AF_INET == 2` |
| `sock_stream_value` | `SOCK_STREAM == 1` |
| `setsockopt_reuseaddr` | Set `SO_REUSEADDR` on socket, verify `getsockopt` reads it back |
| `addrinfo_getaddrinfo` | `getaddrinfo("localhost", NULL, ...)` succeeds, `freeaddrinfo` doesn't crash |

---

### Implementation Order

Sockets should be implemented **after file I/O** because:

1. File I/O extends the system typedef table (prerequisite — `socklen_t`
   etc. follow the same pattern)
2. File I/O tests the pipeline without new emit features (lower risk)
3. Sockets require **union support** — a new emit feature that should be
   implemented and tested in isolation before integrating with a complex
   header target
4. Sockets require **anonymous type naming** — another new feature

Suggested sequence:
1. Implement file I/O E2E (extends typedefs, handles variadic decision)
2. Implement union support as a standalone feature (unit test with fixture)
3. Implement anonymous nested type naming (unit test with fixture)
4. Then tackle sockets E2E (exercises both new features against real headers)

---

### Implementation Steps

1. ✅ Implement union support in model + extract + emit
2. ✅ Implement anonymous nested type synthetic naming
3. ✅ System typedefs (`socklen_t`, `sa_family_t`, `in_port_t`,
   `in_addr_t`) auto-resolved via `CType::Named { resolved }` — no changes needed
4. ✅ Added 3 partitions (Socket, Inet, Netdb) to `bnd-posix.toml` under
   `posix` namespace (not separate assembly as originally
   planned — simpler to keep in one assembly)
5. ✅ Iteratively discovered traverse paths: `bits/socket.h`,
   `bits/socket_type.h`, `bits/socket-constants.h`,
   `bits/types/struct_iovec.h`, `bits/netdb.h`
6. ✅ Added Socket, Inet, Netdb features to `bnd-posix/Cargo.toml`
7. ✅ `htons`/`htonl` are real weak symbols in glibc — P/Invoke works
8. ✅ No conditional compilation flags needed — default clang parse picks up
   all required APIs
9. ✅ Cross-partition refs work via `#[cfg(feature = "X")]` gating
   (e.g. `recv` → `super::unistd::ssize_t`, `addrinfo` → `super::socket::sockaddr`)
10. ✅ 37 socket E2E tests added to `posixfile_e2e.rs`

---

## Signal

### Why Signal

- **Union-in-struct**: `struct sigaction` contains a union
  `__sigaction_handler` with `sa_handler` (function pointer) vs
  `sa_sigaction` (3-arg handler pointer) — first real use of unions
  inside structs in bnd-posix
- **Function-pointer typedef**: `__sighandler_t` = `void (*)(int)` —
  emitted as a WinMD delegate, generated as
  `Option<unsafe extern "system" fn(i32)>`. First delegate type in bnd-posix.
- **Deeply nested anonymous types**: `siginfo_t` contains
  `_sifields` union with 7 variants, some containing further nested
  structs and unions (e.g. `_sigfault._bounds._addr_bnd`)
- **Deep include graph**: `signal.h` pulls 10 sub-headers across
  `bits/` and `bits/types/`
- **Platform-specific register state**: `sigcontext` with x86-64
  registers, `_fpstate`/`_fpxreg`/`_xmmreg`/`_xstate` FPU structs
- **Function/struct name collision**: `sigstack` is both a function
  and a struct (same pattern as `stat`)

### Partition Config

Single partition under `posix.signal`:
- **Header**: `signal.h`
- **Traverse paths**: `signal.h`, `bits/sigaction.h`,
  `bits/signum-generic.h`, `bits/signum-arch.h`, `bits/sigcontext.h`,
  `bits/types/__sigset_t.h`, `bits/types/siginfo_t.h`,
  `bits/types/__sigval_t.h`, `bits/types/stack_t.h`,
  `bits/types/struct_sigstack.h`

### Challenges

#### 1. Deep include graph — ✅ Resolved

`signal.h` includes 10 sub-headers. Each missing path caused
windows-bindgen to panic with "type not found". Iteratively discovered:
`__sigset_t` → `siginfo_t`/`__sigval_t` → `stack_t` → `sigcontext` →
`struct_sigstack.h`.

#### 2. Function-pointer delegate — ✅ Resolved

`__sighandler_t` is `void (*)(int)`, emitted as a WinMD delegate.
windows-bindgen generates `Option<unsafe extern "system" fn(i32)>`.
Works correctly for both `signal()` function parameter/return and
`sigaction.__sigaction_handler.sa_handler` field.

#### 3. Function/struct name collision — ✅ Resolved

`sigstack` is both a deprecated function and a struct. Adding
`bits/types/struct_sigstack.h` to traverse emits both alongside
each other. Same pattern as `stat` function vs `struct stat`.

#### 4. Cross-partition reference — ✅ Resolved

`sigtimedwait` takes `const struct timespec *` which lives in the
stat partition. windows-bindgen auto-gates with
`#[cfg(feature = "stat")]`.

### API Surface

**30 functions**: `kill`, `raise`, `signal`, `sigaction`, `sigprocmask`,
`sigemptyset`, `sigfillset`, `sigaddset`, `sigdelset`, `sigismember`,
`sigpending`, `sigsuspend`, `sigaltstack`, `sigwait`, `sigwaitinfo`,
`sigtimedwait`, `sigqueue`, `sigreturn`, `psignal`, `psiginfo`,
`killpg`, `sigblock`, `sigsetmask`, `siggetmask`, `siginterrupt`,
`sigstack`, `ssignal`, `gsignal`, `__sysv_signal`,
`__libc_current_sigrtmin`, `__libc_current_sigrtmax`

**~50 constants**: `SIGHUP`..`SIGUSR2` (standard signals),
`SA_NOCLDSTOP`, `SA_NOCLDWAIT`, `SA_SIGINFO`, `SA_ONSTACK`,
`SA_RESTART`, `SA_NODEFER`, `SA_RESETHAND`, `SIG_BLOCK`,
`SIG_UNBLOCK`, `SIG_SETMASK`, `__SIGRTMIN`, `__SIGRTMAX`

**23 structs**: `sigaction` (with union), `siginfo_t` (with 8 nested
anonymous types), `__sigset_t`, `sigcontext`, `sigval`, `__sigval_t`,
`stack_t`, `sigstack`, `_fpstate`, `_fpreg`, `_fpxreg`, `_xmmreg`,
`_fpx_sw_bytes`, `_xsave_hdr`, `_ymmh_state`, `_xstate`

**Types**: `__sighandler_t` = `Option<unsafe extern "system" fn(i32)>`,
`pid_t` = `i32`, `uid_t` = `u32`, `sig_t` = `__sighandler_t`

### E2E Tests (`signal_e2e.rs`)

| Test | What it validates |
|---|---|
| `sig_constants` | SIGHUP through SIGTSTP values match POSIX |
| `sa_flag_constants` | SA_NOCLDSTOP, SA_SIGINFO, SA_RESTART, etc. |
| `sig_block_constants` | SIG_BLOCK=0, SIG_UNBLOCK=1, SIG_SETMASK=2 |
| `sigaction_struct_size` | 152 bytes on x86-64 |
| `sigset_struct_size` | 128 bytes (1024 bits) |
| `siginfo_struct_size` | 128 bytes on x86-64 |
| `stack_t_struct_size` | 24 bytes on x86-64 |
| `sighandler_type_is_option_fn_pointer` | pointer-sized Option\<fn\> |
| `sigemptyset_and_sigaddset` | sigset bit manipulation via libc |
| `sigfillset_and_sigdelset` | sigset fill + delete via libc |
| `raise_and_signal_handler` | Install handler, raise SIGUSR1, verify called |
| `sigaction_install_handler` | sigaction with SA_RESTART, raise SIGUSR2 |
| `sigprocmask_block_and_pending` | SIG_BLOCK, sigpending, SIG_SETMASK restore |
| `kill_self_with_zero` | kill(getpid(), 0) process existence check |

### Implementation Steps

1. ✅ Added partition 9 to `bnd-posix.toml` with `posix.signal` namespace
2. ✅ Iteratively discovered 10 traverse paths through `bits/` and `bits/types/`
3. ✅ Generation succeeded — 30 functions, 23 structs, ~50 constants, 4 types
4. ✅ Compilation clean — all struct layouts match libc
5. ✅ Added `signal` feature to `bnd-posix/Cargo.toml` default list
6. ✅ Cross-partition ref: `sigtimedwait` → `stat::timespec` (auto `#[cfg]`)
7. ✅ E2E tests covering constants, struct layouts, sigset ops, signal
   delivery, sigaction, sigprocmask, and kill

---

## Types

Centralise shared POSIX typedefs (`uid_t`, `pid_t`, `mode_t`, `off_t`,
`gid_t`, `ssize_t`, `ino_t`, `dev_t`, `nlink_t`, `blksize_t`, `blkcnt_t`, …)
into a dedicated `posix.types` partition so other partitions reference them
via cross-partition `TypeRef` instead of duplicating definitions.

### Why a Types Partition

- **Deduplication**: Without it, `uid_t`/`pid_t`/`mode_t` etc. appear in
  every partition that includes a header transitively defining them.
- **Single source of truth**: Matches the C model where `<sys/types.h>` is
  the canonical home for these types.
- **Cross-partition hygiene**: Other partitions now carry `#[cfg(feature = "types")]`
  gates on shared-type references, making dependency direction explicit.

### Header Layout

| Header | Resolved path | Role |
|---|---|---|
| `sys/types.h` | `/usr/include/x86_64-linux-gnu/sys/types.h` | Public typedefs |
| `bits/types.h` | `/usr/include/x86_64-linux-gnu/bits/types.h` | Internal `__` types, `__fsid_t` struct |

### Generated Content

- **95 typedefs**: Public (`uid_t`, `pid_t`, …) and internal (`__uid_t`,
  `__pid_t`, …) type aliases.
- **1 struct**: `__fsid_t` (2-element `i32` array, used by `fsid_t` typedef).
- **0 functions**: `sys/types.h` declares no functions.
- **3 constants**: `__S_IREAD`, `__S_IWRITE`, `__S_IEXEC` (permission masks).

### Typedef Deduplication Mechanism

1. **First-writer-wins registry**: `build_type_registry` iterates partitions
   in TOML order. For typedefs, if the name is already registered, the
   later partition is skipped. Since types is partition 1, it registers
   `uid_t` etc. before any other partition processes them.
2. **Dedup retain pass**: After the registry is built, each partition runs
   `partition.typedefs.retain(|td| canonical_ns == partition.namespace)`,
   stripping any typedef whose canonical home is another partition.
3. **Cross-partition TypeRefs**: windows-bindgen emits `super::types::__uid_t`
   references with `#[cfg(feature = "types")]` gates automatically.

### Challenges

| Problem | Root cause | Fix |
|---|---|---|
| `type not found: posix.types.__fsid_t` | `__fsid_t` struct lives in `bits/types.h`, not `sys/types.h` | Added `bits/types.h` to traverse list |
| Last-writer-wins fragility | Original registry used HashMap insert (last writer wins) | Redesigned to first-writer-wins for typedefs |
| ~60 internal `__` typedefs pulled in | `bits/types.h` defines many `__*` types | Harmless — internal types, no API pollution |

### E2E Tests

No E2E tests for this partition — it contains only typedefs and one struct
with no callable functions. Correctness is verified indirectly through the
existing E2E tests in other partitions that consume these types.

### Implementation Steps

1. ✅ Added partition 1 (types) to `bnd-posix.toml` with `posix.types` namespace
2. ✅ Added `sys/types.h` header and `bits/types.h` traverse path
3. ✅ Implemented first-writer-wins typedef dedup in `build_type_registry`
4. ✅ Added dedup retain pass in `generate_from_config`
5. ✅ Generation succeeded — 95 typedefs, 1 struct, 0 functions, 3 constants
6. ✅ Compilation clean — all cross-partition `#[cfg]` gates resolve
7. ✅ Added `types` feature to `bnd-posix/Cargo.toml` default list
8. ✅ All existing tests pass (no E2E changes needed)

---

## Sched

`sched.h` — scheduling API. Separated from `pthread.h` include chain as an
independent POSIX API with its own functions, constants, and types.

### Partition Config

```toml
[[partition]]
namespace = "posix.sched"
library = "c"
headers = ["sched.h"]
traverse = [
    "sched.h",
    "bits/sched.h",
    "bits/types/struct_sched_param.h",
    "bits/cpu-set.h",
]
```

### API Surface

**Functions (10)**: `sched_yield`, `sched_setparam`, `sched_getparam`,
`sched_setscheduler`, `sched_getscheduler`, `sched_get_priority_max`,
`sched_get_priority_min`, `sched_rr_get_interval`, `__sched_cpualloc`,
`__sched_cpucount`, `__sched_cpufree`

**Constants (3)**: `SCHED_OTHER=0`, `SCHED_FIFO=1`, `SCHED_RR=2`

**Structs (2)**: `cpu_set_t` (128 bytes), `sched_param` (4 bytes)

Note: `clone()` is variadic and auto-skipped. `CLONE_*` constants are
`#ifdef __USE_GNU` guarded and extracted.

### E2E Tests (`sched_e2e.rs`)

| Test | What it validates |
|---|---|
| `sched_constants` | SCHED_OTHER/FIFO/RR values |
| `sched_yield_succeeds` | sched_yield returns 0 |
| `sched_get_priority_range` | FIFO priority min < max |
| `sched_getscheduler_self` | Current process is SCHED_OTHER |
| `cpu_set_t_size` | cpu_set_t is 128 bytes |
| `sched_param_size` | sched_param is 4 bytes |

---

## Time

`time.h` — time manipulation, formatting, and POSIX timer APIs. Separated
from `pthread.h` include chain as a substantial independent API (~25
functions, 12 clock constants, and key struct types).

### Partition Config

```toml
[[partition]]
namespace = "posix.time"
library = "c"
headers = ["time.h"]
traverse = [
    "time.h",
    "bits/time.h",
    "bits/types/clock_t.h",
    "bits/types/struct_tm.h",
    "bits/types/clockid_t.h",
    "bits/types/timer_t.h",
    "bits/types/struct_itimerspec.h",
    "bits/types/locale_t.h",
    "bits/types/__locale_t.h",
]
```

### API Surface

**Functions (~25)**: `time`, `clock`, `clock_gettime`, `clock_getres`,
`clock_settime`, `clock_nanosleep`, `nanosleep`, `gmtime`, `gmtime_r`,
`localtime`, `localtime_r`, `mktime`, `timegm`, `difftime`, `strftime`,
`asctime`, `ctime`, `tzset`, `timer_create`, `timer_delete`,
`timer_settime`, `timer_gettime`, `timer_getoverrun`, `timespec_get`

**Constants (12)**: `CLOCK_REALTIME=0`, `CLOCK_MONOTONIC=1`,
`CLOCK_PROCESS_CPUTIME_ID=2`, `CLOCK_THREAD_CPUTIME_ID=3`,
`CLOCK_MONOTONIC_RAW=4`, `CLOCK_REALTIME_COARSE=5`,
`CLOCK_MONOTONIC_COARSE=6`, `CLOCK_BOOTTIME=7`,
`CLOCK_REALTIME_ALARM=8`, `CLOCK_BOOTTIME_ALARM=9`,
`CLOCK_TAI=11`, `TIMER_ABSTIME=1`

**Types**: `tm` (struct, ~56 bytes), `itimerspec` (struct),
`clock_t` (typedef), `clockid_t` (typedef), `timer_t` (typedef),
`locale_t` (typedef), `__locale_struct` (struct)

### E2E Tests (`time_e2e.rs`)

| Test | What it validates |
|---|---|
| `clock_constants` | All CLOCK_* and TIMER_ABSTIME values |
| `time_returns_epoch` | time() returns recent epoch timestamp |
| `clock_gettime_monotonic` | clock_gettime(CLOCK_MONOTONIC) succeeds with elapsed seconds |
| `gmtime_epoch_zero` | gmtime(0) returns 1970-01-01 00:00:00 UTC |
| `mktime_roundtrip` | timegm(gmtime_r(t)) == t |
| `difftime_works` | difftime(100, 50) == 50.0 |
| `struct_tm_layout` | struct tm has reasonable size and zeroed defaults |
| `tzset_runs` | tzset() does not crash |

---

## Pthread

`pthread.h` — POSIX threads API. The largest partition with ~90 functions,
~30 constants, and complex union-based synchronisation types. This was the
primary motivation for exploring this header family — `pthread_create` takes
a function-pointer parameter (testing the delegate-as-param codegen path).

### Partition Config

```toml
[[partition]]
namespace = "posix.pthread"
library = "c"
headers = ["pthread.h"]
traverse = [
    "pthread.h",
    "bits/pthreadtypes.h",
    "bits/thread-shared-types.h",
    "bits/pthreadtypes-arch.h",
    "bits/atomic_wide_counter.h",
    "bits/struct_mutex.h",
    "bits/struct_rwlock.h",
    "bits/types/__sigset_t.h",
    "bits/types/struct___jmp_buf_tag.h",
    "bits/pthread_stack_min-dynamic.h",
    "bits/pthread_stack_min.h",
]
```

### Design Decisions

- **sched.h and time.h separated**: Both are substantial independent APIs
  included by pthread.h. Given their own partitions (13 and 14) rather than
  bundled into the pthread partition.
- **`bits/pthreadtypes.h` in traverse**: Defines all union types
  (`pthread_mutex_t`, `pthread_cond_t`, etc.). Missing this caused
  "type not found: posix.pthread.pthread_mutex_t" panic on first attempt.
- **Function pointers as `*const isize`**: `pthread_create`'s
  `__start_routine`, `pthread_atfork`'s callbacks, `pthread_once`'s
  `__init_routine`, and `pthread_key_create`'s destructor are all emitted
  as `*const isize` (opaque function pointer in WinMD convention).
- **Cross-partition `__sigset_t`**: Both signal and pthread traverse
  `bits/types/__sigset_t.h`. The type ends up defined in both modules;
  signal functions reference `pthread::__sigset_t`. Signal tests updated
  to use `pthread::__sigset_t` for variables passed to signal functions.

### API Surface

**Functions (~90)**: `pthread_create`, `pthread_join`, `pthread_detach`,
`pthread_self`, `pthread_equal`, `pthread_exit`, `pthread_cancel`,
`pthread_mutex_init`/`lock`/`unlock`/`trylock`/`destroy`,
`pthread_cond_init`/`signal`/`broadcast`/`wait`/`timedwait`/`destroy`,
`pthread_rwlock_init`/`rdlock`/`wrlock`/`unlock`/`destroy`,
`pthread_spin_init`/`lock`/`trylock`/`unlock`/`destroy`,
`pthread_barrier_init`/`wait`/`destroy`,
`pthread_key_create`/`delete`/`getspecific`/`setspecific`,
`pthread_attr_*` (init, destroy, get/set detachstate, stacksize, etc.),
`pthread_once`, `pthread_atfork`, …

**Constants (~30)**: `PTHREAD_CREATE_JOINABLE=0`, `PTHREAD_CREATE_DETACHED=1`,
`PTHREAD_MUTEX_NORMAL=0`, `PTHREAD_MUTEX_RECURSIVE=1`,
`PTHREAD_MUTEX_ERRORCHECK=2`, `PTHREAD_CANCEL_ENABLE=0`,
`PTHREAD_CANCEL_DISABLE=1`, `PTHREAD_ONCE_INIT=0`,
`PTHREAD_BARRIER_SERIAL_THREAD=-1`, `PTHREAD_SCOPE_SYSTEM=0`, …

**Types**: `pthread_t` (u64), `pthread_key_t` (u32), `pthread_once_t` (i32),
`pthread_spinlock_t` (i32), `pthread_mutex_t` (union, 40 bytes),
`pthread_cond_t` (union, 48 bytes), `pthread_rwlock_t` (union, 56 bytes),
`pthread_attr_t` (union, 56 bytes), `pthread_barrier_t` (union, 32 bytes),
`pthread_mutexattr_t`, `pthread_condattr_t`, `pthread_rwlockattr_t`,
`pthread_barrierattr_t`

### E2E Tests (`pthread_e2e.rs`)

| Test | What it validates |
|---|---|
| `pthread_constants` | PTHREAD_CREATE_*, MUTEX_*, CANCEL_*, SCOPE_*, ONCE_INIT, BARRIER_SERIAL_THREAD |
| `pthread_self_returns_nonzero` | pthread_self() != 0 |
| `pthread_equal_self` | Thread is equal to itself |
| `mutex_init_lock_unlock_destroy` | Full mutex lifecycle |
| `mutex_trylock` | trylock succeeds on unlocked, returns EBUSY on locked |
| `rwlock_read_write` | rdlock + wrlock roundtrip |
| `pthread_key_create_delete` | TLS key create/set/get/delete with value 42 |
| `pthread_create_join` | Create thread with function pointer, join, verify return value |
| `pthread_attr_init_destroy` | Attr init, default detach state is JOINABLE |
| `spinlock_lock_unlock` | Spinlock init/lock/unlock/destroy |
| `struct_sizes` | mutex_t=40, cond_t=48, rwlock_t=56, attr_t=56, barrier_t=32 |

---

## Stdio

`stdio.h` — standard buffered I/O. The most widely used C header, providing
file stream operations (`fopen`/`fclose`/`fread`/`fwrite`), character I/O
(`fgetc`/`fputc`/`fgets`/`fputs`), positioning (`fseek`/`ftell`/`fgetpos`),
and POSIX extensions (`fdopen`, `fileno`, `popen`/`pclose`, `getline`).

### Partition Config

```toml
[[partition]]
namespace = "posix.stdio"
library = "c"
headers = ["stdio.h"]
traverse = [
    "stdio.h",
    "bits/stdio_lim.h",
    "bits/types/__fpos_t.h",
    "bits/types/__mbstate_t.h",
    "bits/types/struct_FILE.h",
    "bits/types/cookie_io_functions_t.h",
]
```

### Design Decisions

1. **`_IO_FILE` struct traversal** — `struct _IO_FILE` is defined in
   `bits/types/struct_FILE.h` with ~30 internal fields. Several fields
   reference glibc-private incomplete types (`_IO_marker`, `_IO_codecvt`,
   `_IO_wide_data`) which map to `*mut c_void` via the incomplete-record
   fallback. `_IO_lock_t` is forward-declared but gets an opaque `isize`
   typedef. Traversing `struct_FILE.h` is required because windows-bindgen
   panics with "type not found" when functions reference a type that has
   no definition in the winmd. The emitted struct has the correct 216-byte
   layout, validated by the `io_file_struct_size` E2E test.

2. **Variadic functions skipped** — `printf`, `scanf`, `dprintf`,
   `fprintf`, `sprintf`, `snprintf`, `fscanf`, `sscanf` and variants
   are variadic (auto-skipped). The usable I/O surface is the
   non-variadic functions: `fread`, `fwrite`, `fgets`, `fputs`, `fgetc`,
   `fputc`, `puts`, `getline`, etc. The `v*` variants (`vfprintf`,
   `vsnprintf`, `vscanf`, etc.) take `va_list` which maps to
   `*mut c_void` — present in bindings but not directly callable
   from safe Rust.

3. **`__va_list_tag` compiler built-in** — on x86-64, `va_list` is
   `typedef __builtin_va_list`, whose canonical type is
   `__va_list_tag[1]`. The record type `__va_list_tag` has no header
   file location and leaks through when clang resolves `va_list`
   parameters on the `v*printf`/`v*scanf` functions. Fix: map
   `__va_list_tag` to `CType::Void` in extract.rs so these functions
   get `*mut c_void` parameters.

4. **`fpos_t` struct** — `fpos_t` is `typedef struct _G_fpos_t { __off_t
   __pos; __mbstate_t __state; }`. Requires traversing
   `bits/types/__fpos_t.h` and `bits/types/__mbstate_t.h` for
   `fgetpos`/`fsetpos`.

5. **glibc `__REDIRECT` duplicates** — glibc uses `__REDIRECT` macros for
   LFS (Large File Support) compatibility. Functions like `fseeko`,
   `ftello`, `fgetpos`, `fsetpos` have both 32-bit and 64-bit variants.
   The existing function dedup pass handles this.

6. **`cookie_io_functions_t`** — glibc extension struct with function
   pointer fields (read/write/seek/close callbacks). Used only by
   `fopencookie` (GNU extension, not POSIX). Requires traversing
   `bits/types/cookie_io_functions_t.h`. Callback fields are emitted as
   delegate typedefs (`cookie_read_function_t`, etc.).

7. **`stdin`/`stdout`/`stderr`** — these are `extern FILE*` global
   variables, not functions. bnd-winmd currently only extracts functions,
   constants, and types — not global variables. These will be missing
   from the bindings.

### API Surface

**Non-variadic functions (~60)**: `fopen`, `fclose`, `fflush`, `freopen`,
`fdopen`, `fmemopen`, `open_memstream`, `fopencookie`,
`setbuf`, `setvbuf`, `setbuffer`, `setlinebuf`,
`fgetc`, `fputc`, `getc`, `putc`, `getchar`, `putchar`, `ungetc`,
`fgets`, `fputs`, `puts`, `gets`, `getline`, `getdelim`,
`fread`, `fwrite`, `fread_unlocked`, `fwrite_unlocked`,
`fseek`, `ftell`, `rewind`, `fseeko`, `ftello`,
`fgetpos`, `fsetpos`, `fgetpos64`, `fsetpos64`,
`clearerr`, `feof`, `ferror`, `fileno`, `perror`,
`popen`, `pclose`, `tmpfile`, `tmpnam`, `tmpnam_r`, `tempnam`,
`flockfile`, `ftrylockfile`, `funlockfile`,
`fcloseall`,
`vfprintf`, `vprintf`, `vsprintf`, `vsnprintf`, `vasprintf`,
`vfscanf`, `vscanf`, `vsscanf`, `vdprintf`
(+`_unlocked` variants)

**Variadic (auto-skipped)**: `printf`, `fprintf`, `sprintf`, `snprintf`,
`dprintf`, `scanf`, `fscanf`, `sscanf`, `asprintf`

**Constants (21)**: `BUFSIZ=8192`, `SEEK_SET=0`, `SEEK_CUR=1`,
`SEEK_END=2`, `L_tmpnam=20`, `L_ctermid=9`, `TMP_MAX=238328`,
`FOPEN_MAX=16`, `FILENAME_MAX=4096`, `_IOFBF=0`, `_IOLBF=1`, `_IONBF=2`,
`_IO_EOF_SEEN=16`, `_IO_ERR_SEEN=32`, `_IO_USER_LOCK=32768`,
`_STDIO_H`, `_BITS_STDIO_LIM_H`, `_____fpos_t_defined`,
`____mbstate_t_defined`, `__cookie_io_functions_t_defined`,
`__struct_FILE_defined`

**Structs (8)**: `_IO_FILE` (216 bytes, 30 fields), `_G_fpos_t`,
`fpos_t`, `__fpos_t`, `__mbstate_t` (with `__mbstate_t___value` union),
`_IO_cookie_io_functions_t`, `cookie_io_functions_t`

**Typedefs (6)**: `_IO_lock_t` (opaque `isize`), `va_list`
(`*mut c_void`), `cookie_read_function_t`, `cookie_write_function_t`,
`cookie_seek_function_t`, `cookie_close_function_t`

### E2E Tests (`stdio_e2e.rs`)

| Test | What it validates |
|---|---|
| `stdio_constants` | BUFSIZ, EOF, SEEK_SET/CUR/END, L_tmpnam, TMP_MAX, FOPEN_MAX, FILENAME_MAX |
| `fopen_fclose` | fopen a temp file, verify non-null FILE*, fclose returns 0 |
| `fwrite_fread_roundtrip` | Write bytes with fwrite, rewind, read back with fread, compare |
| `fgets_fputs` | fputs a string, rewind, fgets reads it back |
| `fseek_ftell` | fseek to offset, ftell returns same offset |
| `fgetc_fputc` | Write chars with fputc, rewind, read back with fgetc |
| `fileno_returns_valid_fd` | fileno on fopen'd file returns fd >= 0 |
| `popen_pclose` | popen "echo hello", read output, pclose returns 0 |
| `feof_after_read` | Read until EOF, feof returns non-zero |
| `ferror_on_write_to_readonly` | Write to read-only stream, ferror returns non-zero |
| `fpos_t_layout` | fpos_t struct size matches C sizeof (16 bytes) |
| `io_file_struct_size` | _IO_FILE struct is 216 bytes (glibc x86-64) |
| `tmpfile_creates_anonymous` | tmpfile returns non-null, fileno >= 0, fclose succeeds |
