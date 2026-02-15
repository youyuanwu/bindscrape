# bns-posix: System Header Testing

Design notes for the POSIX API families tested through the `bns-posix` crate.
Each section documents partition layout, expected challenges, API surface,
and E2E test plans for one header group.

| API family | Status | Key feature exercised |
|---|---|---|
| [PosixFile](#posixfile--file-io) | ✅ Implemented | System typedefs, variadic skipping, `struct stat` |
| [PosixSocket](#posixsocket--sockets) | ⬜ Planned | Unions, anonymous nested types, self-referential structs |

---

## PosixFile — File I/O

Validate bindscrape against **POSIX file I/O headers** — `<fcntl.h>`,
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
separates them into distinct Rust modules (`PosixFile::Fcntl::off_t`
vs `PosixFile::Unistd::off_t`), avoiding compilation errors.

---

### API Surface

#### PosixFile.Fcntl (fcntl.h + bits/fcntl-linux.h)

**Functions (4)**: `creat`, `lockf`, `posix_fadvise`, `posix_fallocate`
(skipping variadic `open`, `fcntl`, `openat`)
**Constants (60)**: `O_RDONLY`, `O_WRONLY`, `O_RDWR`, `O_CREAT`, `O_TRUNC`,
`O_APPEND`, `O_EXCL`, `O_NONBLOCK`, `AT_FDCWD`, `SEEK_SET`, `SEEK_CUR`,
`SEEK_END`, `R_OK`, `W_OK`, `X_OK`, `F_OK`, ...
**Typedefs (3)**: `mode_t`, `off_t`, `pid_t`

#### PosixFile.Unistd (unistd.h)

**Functions (103)**: `read`, `write`, `close`, `lseek`, `ftruncate`, `unlink`,
`access`, `getpid`, `dup`, `dup2`, `pipe`, `fsync`, `fork`, `execv`, ...
(variadic `execl`, `execle`, `execlp`, `syscall` automatically skipped)
**Constants (23)**: `STDIN_FILENO`, `STDOUT_FILENO`, `STDERR_FILENO`,
`SEEK_SET`, `SEEK_CUR`, `SEEK_END`, `R_OK`, `W_OK`, `X_OK`, `F_OK`, ...
**Typedefs (8)**: `gid_t`, `intptr_t`, `off_t`, `pid_t`, `socklen_t`,
`ssize_t`, `uid_t`, `useconds_t`

#### PosixFile.Stat (sys/stat.h + bits/struct_stat.h + bits/types/struct_timespec.h)

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

---

## PosixSocket — Sockets

Validate bindscrape against **POSIX socket headers** — `<sys/socket.h>`,
`<netinet/in.h>`, `<arpa/inet.h>`, and `<netdb.h>`. This is the first
system header target that requires **union support** (`ExplicitLayout` +
`FieldLayout`) and **anonymous nested types** — both currently unimplemented
features that sockets will force.

### Why Sockets

- **Unions**: `struct in6_addr` contains an anonymous union with three
  members (`__u6_addr8`, `__u6_addr16`, `__u6_addr32`). `struct sockaddr`
  variants (`sockaddr_in` vs `sockaddr_in6` vs `sockaddr_un`) are commonly
  cast between via pointer, but the `in6_addr` union is the critical
  structural test.
- **Anonymous nested types**: `in6_addr.__in6_u` is an anonymous union
  member — needs synthetic naming (`in6_addr__Anonymous_0` or similar)
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
name = "PosixSocket"
file = "posixsocket.winmd"

# Partition 1: socket types and core API
[[partition]]
namespace = "PosixSocket"
library = "c"
headers = ["/usr/include/sys/socket.h"]
traverse = ["/usr/include/sys/socket.h"]

# Partition 2: IPv4/IPv6 structs and constants
[[partition]]
namespace = "PosixSocket.Inet"
library = "c"
headers = ["/usr/include/netinet/in.h"]
traverse = ["/usr/include/netinet/in.h"]

# Partition 3: address conversion functions
[[partition]]
namespace = "PosixSocket.Arpa"
library = "c"
headers = ["/usr/include/arpa/inet.h"]
traverse = ["/usr/include/arpa/inet.h"]

# Partition 4: name resolution
[[partition]]
namespace = "PosixSocket.Netdb"
library = "c"
headers = ["/usr/include/netdb.h"]
traverse = ["/usr/include/netdb.h"]
```

#### Alternative: 2-Partition Layout

```toml
# Partition 1: sys/socket.h types + functions
[[partition]]
namespace = "PosixSocket"
library = "c"
headers = ["/usr/include/sys/socket.h"]
traverse = ["/usr/include/sys/socket.h"]

# Partition 2: inet + arpa + netdb
[[partition]]
namespace = "PosixSocket.Inet"
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

#### Union Support (Not Implemented)

This is the **primary driver** for choosing sockets. Unions require:

1. **`ExplicitLayout`** flag on the TypeDef (instead of `SequentialLayout`)
2. **`FieldLayout`** with offset 0 for every field (all fields overlap)
3. **`ClassLayout`** with the union's total size
4. Detection: `EntityKind::UnionDecl` in clang, or check
   `Type::get_canonical_type()` for record types with `is_union()` (via
   the underlying clang API — may need `clang-sys` raw FFI if
   `clang` crate doesn't expose it directly)

Implementation sketch:
```rust
// In emit.rs — new emit_union function
fn emit_union(file: &mut File, namespace: &str, union_def: &StructDef) {
    let value_type = file.TypeRef("System", "ValueType");
    let td = file.TypeDef(
        namespace, &union_def.name, value_type,
        TypeAttributes::PUBLIC | TypeAttributes::EXPLICIT_LAYOUT,
    );
    file.ClassLayout(td, union_def.align as u16, union_def.size as u32);
    for field in &union_def.fields {
        let ty = ctype_to_wintype(&field.ty, namespace, &registry);
        let f = file.Field(&field.name, &ty, FieldAttributes::PUBLIC);
        file.FieldLayout(f, 0);  // All fields at offset 0
    }
}
```

Changes needed:
- **`model.rs`**: Add `is_union: bool` to `StructDef` (or create `UnionDef`)
- **`extract.rs`**: Detect `EntityKind::UnionDecl` or check
  `Type::is_union()` — add `collect_unions()` helper, or flag on
  `StructDef`
- **`emit.rs`**: `emit_union()` with `ExplicitLayout` + `FieldLayout`
  at offset 0
- **`sonar`**: Check if `find_unions()` works or needs the same
  supplemental-pass treatment as `find_structs()`

#### Anonymous Nested Types (Partial)

`struct in6_addr` on Linux/glibc:
```c
struct in6_addr {
    union {
        uint8_t  __u6_addr8[16];
        uint16_t __u6_addr16[8];
        uint32_t __u6_addr32[4];
    } __in6_u;
};
```

This requires:
1. Detecting the anonymous union member
2. Generating a synthetic TypeDef name (e.g., `in6_addr__in6_u`)
3. Emitting the anonymous union as a separate TypeDef with
   `ExplicitLayout`
4. Referencing it as a field type in the parent struct
5. Optionally emitting `NestedClass` to associate parent and child

#### New System Typedefs

| Typedef | Canonical type | Winmd mapping |
|---|---|---|
| `socklen_t` | `unsigned int` | `U32` |
| `sa_family_t` | `unsigned short` | `U16` |
| `in_port_t` | `uint16_t` | `U16` |
| `in_addr_t` | `uint32_t` | `U32` |

---

### Challenges

#### 1. Union detection in `clang` crate

The `clang` crate exposes `EntityKind::UnionDecl` for top-level unions,
but it's unclear whether `sonar::find_unions()` has the same limitations
as `find_structs()` (missing unions without matching typedef). Likely
needs the same supplemental pass pattern.

For anonymous unions nested inside structs, the union appears as a child
entity with `EntityKind::UnionDecl` and `is_anonymous() == true`. Need
to walk struct children and handle this case.

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

#### PosixSocket (sys/socket.h)

**Structs**: `sockaddr` (16 bytes — `sa_family` + `sa_data[14]`)
**Functions**: `socket`, `bind`, `listen`, `accept`, `connect`, `send`,
`recv`, `sendto`, `recvfrom`, `setsockopt`, `getsockopt`, `getsockname`,
`getpeername`, `shutdown`, `close` (if re-exported)
**Constants**: `AF_INET`, `AF_INET6`, `AF_UNIX`, `AF_UNSPEC`,
`SOCK_STREAM`, `SOCK_DGRAM`, `SOCK_RAW`, `SOL_SOCKET`, `SO_REUSEADDR`,
`SO_REUSEPORT`, `SO_KEEPALIVE`, `SHUT_RD`, `SHUT_WR`, `SHUT_RDWR`

#### PosixSocket.Inet (netinet/in.h)

**Structs**: `in_addr` (4 bytes), `in6_addr` (16 bytes, contains union),
`sockaddr_in` (16 bytes), `sockaddr_in6` (28 bytes)
**Constants**: `IPPROTO_TCP`, `IPPROTO_UDP`, `IPPROTO_IP`,
`INADDR_ANY`, `INADDR_LOOPBACK`, `INADDR_BROADCAST`,
`INET_ADDRSTRLEN`, `INET6_ADDRSTRLEN`

#### PosixSocket.Arpa (arpa/inet.h)

**Functions**: `inet_pton`, `inet_ntop`, `inet_addr`, `inet_ntoa`

#### PosixSocket.Netdb (netdb.h)

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

1. ⬜ Implement union support in model + extract + emit
2. ⬜ Implement anonymous nested type synthetic naming
3. ✅ System typedefs (`socklen_t`, `sa_family_t`, `in_port_t`,
   `in_addr_t`) auto-resolved via `CType::Named { resolved }` — no changes needed
4. ⬜ Create `tests/fixtures/bns-posix/posixsocket.toml`
5. ⬜ Add roundtrip tests in `roundtrip_posixsocket.rs`
6. ⬜ Add socket partitions to `bns-posix` crate
7. ⬜ Handle `htons`/`htonl` (skip if inline, or provide wrapper)
8. ⬜ Handle conditional compilation flags if needed
9. ⬜ Iterate on traverse paths for `bits/` sub-headers
10. ⬜ Add socket E2E tests to `posixfile_e2e.rs` (or new test file)
