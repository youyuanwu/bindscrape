# System Header E2E Testing: POSIX File I/O

## Goal

Validate bindscrape against **POSIX file I/O headers** — `<fcntl.h>`,
`<unistd.h>`, and `<sys/stat.h>`. This exercises a fundamentally different
API surface than zlib: many new system typedefs (`mode_t`, `uid_t`, `pid_t`,
`time_t`, etc.), variadic functions (`open`), large/complex structs
(`struct stat`), and a dense `#define` constant space (`O_RDONLY`,
`S_IRUSR`, etc.).

---

## Why File I/O

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

## Headers & Partitions

### Arch-Specific Header Paths (Critical)

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

### Where Declarations Actually Live (Verified on Ubuntu 24.04)

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

### Sub-Header Problem

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

### Proposed Partition Layout

```toml
include_paths = [
    "/usr/include/x86_64-linux-gnu",
    "/usr/include",
]

[output]
name = "PosixFile"
file = "bns-posix.winmd"

# Partition 1: fcntl — creat + O_* flags
# open/openat/fcntl are variadic and will be auto-skipped
[[partition]]
namespace = "PosixFile.Fcntl"
library = "c"
headers = ["fcntl.h"]
traverse = ["fcntl.h", "bits/fcntl-linux.h"]

# Partition 2: unistd — read/write/close/lseek
[[partition]]
namespace = "PosixFile.Unistd"
library = "c"
headers = ["unistd.h"]
traverse = ["unistd.h"]

# Partition 3: sys/stat — struct stat + stat/fstat/chmod + S_* constants
[[partition]]
namespace = "PosixFile.Stat"
library = "c"
headers = ["sys/stat.h"]
traverse = [
    "sys/stat.h",
    "bits/struct_stat.h",              # struct stat definition
    "bits/types/struct_timespec.h",    # struct timespec for st_atim etc.
]
```

Key points:
- **No `PosixFile.Types` partition** — system typedefs are auto-resolved
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

## System Typedefs (Auto-Resolved)

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

## Expected Challenges

### 1. Variadic functions (`open`) — ✅ Resolved

`open(const char *pathname, int flags, ...)` is variadic.
`collect_functions()` now checks `Entity::is_variadic()` and skips
with a warning. The E2E tests use `creat()` (non-variadic) instead.
`fcntl()` and `openat()` are also variadic and automatically skipped.

### 2. `struct stat` in sub-header

`struct stat` is defined in `bits/struct_stat.h`, not `sys/stat.h`.
The traverse list must include the sub-header. Clang reports the entity
location as the sub-header path, so `should_emit` needs to match
against the resolved sub-header path.

The struct has ~13 fields with glibc-internal reserved fields
(`__pad0`, `__glibc_reserved`). Field names will include these
internals. `struct stat` also uses `struct timespec` for `st_atim`
etc., which needs handling.

### 3. Constants in sub-headers

`O_RDONLY` and friends are `#define`d in `bits/fcntl-linux.h`.
`sonar::find_definitions` operates on macro definitions in the
translation unit — need to verify whether the traverse/location
filter applies to macro definitions or only to entity declarations.
If macros bypass the location filter (since they're preprocessor
directives), the sub-header traverse entry may be unnecessary for
constants.

### 4. `SEEK_SET` not in `unistd.h`

`SEEK_SET`, `SEEK_CUR`, `SEEK_END` are defined in `<stdio.h>` and
`<linux/fs.h>`, not in `<unistd.h>`. If the E2E tests need these
constants, they must come from a `stdio.h` partition or be defined
manually. Alternative: the E2E test Rust code can define these values
directly as Rust constants.

### 5. `S_ISREG` / `S_ISDIR` — function-like macros

These are `#define S_ISREG(m) (((m) & S_IFMT) == S_IFREG)` — not
extractable as constants. `sonar::find_definitions` will see them but
`evaluate()` will fail (they take an argument). Skipped automatically.

### 6. Inline functions in headers

`<unistd.h>` may contain `static inline` functions (glibc versions
vary). These would be extracted as regular functions but have no symbol
in `libc.so`. The P/Invoke would fail at runtime. Need to detect and
skip via `Entity::get_storage_class()` or similar.

### 7. `__` prefixed internal typedefs — ✅ Resolved

glibc function signatures use `__mode_t`, `__off_t`, `__pid_t`, etc.
(verified: `lseek` returns `__off_t`, `getpid` returns `__pid_t`).
These are automatically handled by clang canonical type resolution —
`CType::Named { name: "__mode_t", resolved: Some(U32) }`. No hardcoded
table needed. The user-facing names (`mode_t`, `uid_t` etc.) may appear
as extracted typedefs from the `sys/types.h` partition if they pass
the sonar/collect filters.

### 8. `struct timespec` nested in `struct stat` — ✅ Resolved

`struct stat` fields `st_atim`, `st_mtim`, `st_ctim` are of type
`struct timespec` (defined in `bits/types/struct_timespec.h`).
Adding this sub-header to the Stat partition's traverse list extracts
the struct and allows windows-bindgen to resolve the field types.

### 9. Array parameter decay — ✅ Resolved

Functions like `futimens(int fd, const struct timespec t[2])` have
C array parameters. In C semantics, array parameters always decay
to pointers. However, the winmd ELEMENT_TYPE_ARRAY encoding breaks
windows-bindgen's blob reader (it doesn't consume all ArrayShape fields,
leaving stray bytes). Fixed by decaying `CType::Array` → `CType::Ptr`
in `extract_function()` for parameters.

### 10. Duplicate function declarations (`__REDIRECT`) — ✅ Resolved

glibc uses `__REDIRECT` macros to alias function names (e.g. `lockf`
redirected to `lockf64`). This produces multiple clang declarations of
the same function name. Fixed by deduplicating in `collect_functions()`
with a `HashSet<String>` on the function name.

### 11. Cross-partition type duplicates — ✅ Resolved

Typedefs like `off_t`, `mode_t`, constants like `SEEK_SET`, `R_OK`
appear in multiple partitions. Using namespace modules (no `--flat`)
separates them into distinct Rust modules (`PosixFile::Fcntl::off_t`
vs `PosixFile::Unistd::off_t`), avoiding compilation errors.

---

## API Surface (Expected)

### PosixFile.Fcntl (fcntl.h + bits/fcntl-linux.h)

**Functions (4)**: `creat`, `lockf`, `posix_fadvise`, `posix_fallocate`
(skipping variadic `open`, `fcntl`, `openat`)
**Constants (60)**: `O_RDONLY`, `O_WRONLY`, `O_RDWR`, `O_CREAT`, `O_TRUNC`,
`O_APPEND`, `O_EXCL`, `O_NONBLOCK`, `AT_FDCWD`, `SEEK_SET`, `SEEK_CUR`,
`SEEK_END`, `R_OK`, `W_OK`, `X_OK`, `F_OK`, ...
**Typedefs (3)**: `mode_t`, `off_t`, `pid_t`

### PosixFile.Unistd (unistd.h)

**Functions (103)**: `read`, `write`, `close`, `lseek`, `ftruncate`, `unlink`,
`access`, `getpid`, `dup`, `dup2`, `pipe`, `fsync`, `fork`, `execv`, ...
(variadic `execl`, `execle`, `execlp`, `syscall` automatically skipped)
**Constants (23)**: `STDIN_FILENO`, `STDOUT_FILENO`, `STDERR_FILENO`,
`SEEK_SET`, `SEEK_CUR`, `SEEK_END`, `R_OK`, `W_OK`, `X_OK`, `F_OK`, ...
**Typedefs (8)**: `gid_t`, `intptr_t`, `off_t`, `pid_t`, `socklen_t`,
`ssize_t`, `uid_t`, `useconds_t`

### PosixFile.Stat (sys/stat.h + bits/struct_stat.h + bits/types/struct_timespec.h)

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

## Proposed E2E Tests

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

## Dependencies

- No additional packages — `sys/types.h`, `fcntl.h`, `unistd.h`,
  `sys/stat.h` are part of `libc6-dev` (already present if
  `libclang-dev` is installed)
- libc is implicitly linked — `cargo:rustc-link-lib=dylib=c` may not
  even be necessary, but explicit is safer

---

## Implementation Steps

1. ✅ System typedefs auto-resolved via `CType::Named { resolved }` —
   no hardcoded table needed
2. ✅ Variadic functions warn-and-skip via `Entity::is_variadic()`
3. ✅ C `long` → `I64` for Linux LP64 ABI
4. ✅ Array parameter decay → pointer in `extract_function()`
5. ✅ Function deduplication via `HashSet` in `collect_functions()`
6. ✅ Created `tests/fixtures/bns-posix/bns-posix.toml`
   (3 partitions: Fcntl, Unistd, Stat — no Types partition needed)
7. ✅ Added 9 roundtrip tests in `roundtrip_posixfile.rs`
8. ✅ Created `bns-posix/` crate with feature-gated namespace modules
   (package mode via `bns-posix-gen`, no `build.rs`)
9. ✅ Added `struct_timespec.h` to Stat traverse list
10. ✅ Created 15 E2E tests (`posixfile_e2e.rs`) — all passing
11. ✅ Added `bns-posix` and `bns-posix-gen` to workspace members
12. ✅ Separated generator into `bns-posix-gen` crate using
   `windows-bindgen --package` mode
