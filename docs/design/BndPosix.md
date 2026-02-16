# bnd-posix — POSIX System Bindings via WinMD

`bnd-posix` provides Rust bindings for POSIX file I/O and process APIs on
Linux, generated from C system headers through the
**bnd-winmd → WinMD → windows-bindgen** pipeline.

This is the first *product* crate built on bnd-winmd, demonstrating that the
C-header-to-WinMD approach scales beyond test fixtures to real system APIs.

## Modules

| Module | Header(s) | Functions | Constants | Structs |
|---|---|---|---|---|
| `posix::dirent` | `dirent.h`, `bits/dirent.h` | 12 | ~11 | `dirent` |
| `posix::dl`     | `dlfcn.h`, `bits/dlfcn.h` | 4 | ~8 | — |
| `posix::errno`  | `errno.h`, `bits/errno.h`, `asm-generic/errno*.h` | 1 | ~130 | — |
| `posix::fcntl`  | `fcntl.h` | 4 | ~60 | — |
| `posix::inet`   | `netinet/in.h`, `arpa/inet.h` | 20 | ~75 | `sockaddr_in`, `sockaddr_in6`, `in_addr`, `in6_addr` (+unions) |
| `posix::mmap`   | `sys/mman.h`, `bits/mman-linux.h`, `bits/mman-map-flags-generic.h` | 13 | ~62 | — |
| `posix::netdb`  | `netdb.h`, `bits/netdb.h` | 56 | ~32 | `addrinfo`, `hostent`, `servent`, `protoent`, `netent` |
| `posix::pthread` | `pthread.h`, `bits/pthreadtypes.h`, `bits/thread-shared-types.h` | ~90 | ~30 | `pthread_mutex_t`, `pthread_cond_t`, `pthread_rwlock_t`, `pthread_attr_t`, `pthread_barrier_t` (unions) |
| `posix::sched`  | `sched.h`, `bits/sched.h`, `bits/cpu-set.h` | 10 | ~3 | `cpu_set_t`, `sched_param` |
| `posix::signal` | `signal.h`, `bits/sigaction.h`, `bits/signum-*.h`, `bits/sigcontext.h`, `bits/types/*` | 30 | ~50 | `sigaction` (union), `siginfo_t` (nested unions), `__sigset_t`, `sigcontext`, `stack_t` |
| `posix::socket` | `sys/socket.h`, `bits/socket.h`, `bits/socket_type.h`, `bits/socket-constants.h` | 20 | ~102 | `sockaddr`, `sockaddr_storage`, `msghdr`, `iovec`, `cmsghdr`, `linger` |
| `posix::stat`   | `sys/stat.h`, `bits/struct_stat.h`, `bits/types/struct_timespec.h` | 17 | 4 | `stat`, `timespec` |
| `posix::time`   | `time.h`, `bits/time.h` | ~25 | ~12 | `tm`, `itimerspec`, `__locale_struct` |
| `posix::types`  | `sys/types.h`, `bits/types.h` | — | — | `__fsid_t` + 94 shared typedefs (`uid_t`, `pid_t`, `mode_t`, …) |
| `posix::unistd` | `unistd.h` | 103 | ~23 | — |

### Usage

```rust
use bnd_posix::posix::{fcntl, stat, unistd};

// Create a file
let path = c"/tmp/example.txt";
let fd = unsafe { fcntl::creat(path.as_ptr(), 0o644) };
assert!(fd >= 0);

// Write
let data = b"hello";
unsafe { unistd::write(fd, data.as_ptr().cast(), data.len() as u64) };

// Stat
let mut st = stat::stat::default();
unsafe { stat::fstat(fd, &mut st as *mut _ as *const _) };
assert_eq!(st.st_size, 5);

// Close
unsafe { unistd::close(fd) };
```

## Architecture

The bindings are produced by a separate **generator crate** (`bnd-posix-gen`)
and checked into the `bnd-posix` source tree — there is no `build.rs`.

```
  bnd-posix-gen (cargo run -p bnd-posix-gen)
  ┌─────────────────────────────────────────────────────────┐
  │                                                         │
  │  bnd-posix.toml ──▶ bnd-winmd ──▶ .winmd               │
  │                                      │                  │
  │                          windows-bindgen --package       │
  │                                      │                  │
  │                                      ▼                  │
  │                              bnd-posix/src/              │
  │                              ├── posix/                  │
  │                              │   ├── mod.rs              │
  │                              │   ├── fcntl/mod.rs        │
  │                              │   ├── stat/mod.rs         │
  │                              │   └── unistd/mod.rs       │
  │                              └── lib.rs (hand-written)   │
  └─────────────────────────────────────────────────────────┘
```

To regenerate:

```sh
cargo run -p bnd-posix-gen
```

1. **bnd-winmd** parses `bnd-posix.toml`, invokes clang on system headers,
   extracts types/functions/constants, and writes a temporary `.winmd` file.
2. **windows-bindgen `--package`** reads the `.winmd` and generates one
   `mod.rs` per namespace under `src/posix/`, with `#[cfg(feature)]`
   gating on each sub-module. It also appends feature definitions to
   `Cargo.toml` after the `# generated features` marker.
3. The intermediate `.winmd` is deleted — `bnd-posix` is a pure Rust crate
   with no build-time code generation.

### Why namespace modules?

Multiple partitions reference overlapping system types (`off_t`, `mode_t`,
`uid_t`, etc.). A dedicated **types** partition (`posix.types`) owns these
shared typedefs. During generation, bnd-winmd deduplicates: the types
partition is processed first (first-writer-wins for typedefs), and later
partitions' duplicate copies are removed. Function signatures in other
partitions use cross-partition TypeRefs (e.g. `super::types::__uid_t`).

## Partition Config

The TOML config lives at `tests/fixtures/bnd-posix/bnd-posix.toml`
and defines fifteen partitions:

| Partition | Namespace | Headers traversed |
|---|---|---|
| Types | `posix.types` | `sys/types.h`, `bits/types.h` |
| Dirent | `posix.dirent` | `dirent.h`, `bits/dirent.h` |
| Dl | `posix.dl` | `dlfcn.h`, `bits/dlfcn.h` |
| Errno | `posix.errno` | `errno.h`, `bits/errno.h`, `linux/errno.h`, `asm/errno.h`, `asm-generic/errno.h`, `asm-generic/errno-base.h` |
| Fcntl | `posix.fcntl` | `fcntl.h` |
| Inet | `posix.inet` | `netinet/in.h`, `arpa/inet.h` |
| Mmap | `posix.mmap` | `sys/mman.h`, `bits/mman-linux.h`, `bits/mman-map-flags-generic.h` |
| Netdb | `posix.netdb` | `netdb.h`, `bits/netdb.h` |
| Pthread | `posix.pthread` | `pthread.h`, `bits/pthreadtypes.h`, `bits/thread-shared-types.h`, `bits/pthreadtypes-arch.h`, `bits/struct_mutex.h`, `bits/struct_rwlock.h`, … |
| Sched | `posix.sched` | `sched.h`, `bits/sched.h`, `bits/types/struct_sched_param.h`, `bits/cpu-set.h` |
| Signal | `posix.signal` | `signal.h`, `bits/sigaction.h`, `bits/signum-generic.h`, `bits/signum-arch.h`, `bits/sigcontext.h`, `bits/types/__sigset_t.h`, `bits/types/siginfo_t.h`, `bits/types/__sigval_t.h`, `bits/types/stack_t.h`, `bits/types/struct_sigstack.h` |
| Socket | `posix.socket` | `sys/socket.h`, `bits/socket.h`, `bits/socket_type.h`, `bits/socket-constants.h`, `bits/types/struct_iovec.h` |
| Stat | `posix.stat` | `sys/stat.h`, `bits/struct_stat.h`, `bits/types/struct_timespec.h` |
| Time | `posix.time` | `time.h`, `bits/time.h`, `bits/types/clock_t.h`, `bits/types/struct_tm.h`, `bits/types/clockid_t.h`, `bits/types/timer_t.h`, `bits/types/struct_itimerspec.h`, … |
| Unistd | `posix.unistd` | `unistd.h` |

## Challenges Solved

These are issues encountered while building real system bindings and fixed
in bnd-winmd core (see [bnd-posix.md](systesting/bnd-posix.md) for details):

1. **System typedef resolution** — `CType::Named { resolved }` carries
   clang's canonical type; no hardcoded table.
2. **Variadic function skipping** — `printf`, `open`, etc. skipped with warning.
3. **LP64 `long` → `I64`** — C `long` is 8 bytes on Linux x86-64.
4. **Array parameter decay** — `const struct timespec t[2]` → pointer
   (avoids `ELEMENT_TYPE_ARRAY` blob incompatibility with windows-bindgen).
5. **Function deduplication** — glibc `__REDIRECT` macros create duplicate
   declarations; deduplicated via `HashSet<String>`.
6. **Cross-partition overlap** — namespace modules prevent duplicate
   definitions of `off_t`, `SEEK_SET`, etc.
7. **Hex/octal constant extraction** — `parse_hex_or_suffixed_int()` handles
   `0x` hex, `0` octal, and `U`/`L`/`UL`/`ULL` suffixes. Found when adding
   Mmap partition (`PROT_READ 0x1`, `MAP_SHARED 0x01` were silently dropped).
8. **PtrConst mid-chain panic** — `PtrMut(PtrConst(Named, 1), 1)` puts
   `ELEMENT_TYPE_CMOD_REQD` mid-chain in pointer blobs, crashing
   windows-bindgen. Fix: always emit `PtrMut`; const-ness tracked by
   `ConstAttribute`. Found when adding Dirent partition.
9. **Anonymous enum → constants** — unnamed C enums (e.g. `DT_*` in
   `dirent.h`) generate invalid Rust type names. Fix: detect anonymous
   enums and emit variants as standalone constants.
10. **Opaque typedef to void** — `typedef struct __dirstream DIR` maps to
    `CType::Void` which emits `c_void` (not `Copy`/`Clone`). Fix: emit
    `isize` for void-underlying typedefs.
11. **`bits/` sub-header traversal** — socket constants (`AF_*`, `SOCK_*`,
    `SOL_*`) live in `bits/socket.h`, `bits/socket_type.h`, and
    `bits/socket-constants.h`. `struct iovec` is in
    `bits/types/struct_iovec.h`, `struct netent` in `bits/netdb.h`.
    Traverse lists must include these sub-headers or types are missing
    and windows-bindgen panics with `type not found`.
12. **Cross-partition type references** — `recv`/`send` use
    `super::unistd::ssize_t`; `addrinfo` uses `super::socket::sockaddr`.
    windows-bindgen gates these with `#[cfg(feature = "X")]` automatically.
13. **`htons`/`htonl` as real symbols** — on glibc x86-64, `htons`/`htonl`
    have real symbols in `libc.so` (weak symbols), so P/Invoke works.
14. **Function-pointer typedefs** — `__sighandler_t` is
    `void (*)(int)`, emitted as a WinMD delegate and generated as
    `Option<unsafe extern "system" fn(i32)>`. First use of delegate
    types in bnd-posix.
15. **Function/struct name collision** — `sigstack` is both a function
    and a struct. Adding `bits/types/struct_sigstack.h` to the traverse
    list emits both; same pattern as `stat`.
16. **Deep include graph** — `signal.h` pulls 10 sub-headers across
    `bits/` and `bits/types/`; each missing traverse path causes
    windows-bindgen to panic with "type not found".
17. **Typedef deduplication** — shared POSIX types (`uid_t`, `pid_t`,
    `mode_t`, etc.) appear in multiple headers. A dedicated `posix.types`
    partition owns them; the type registry uses first-writer-wins for
    typedefs, and the dedup pass removes duplicates from later partitions.

## How to Add a New Partition

### 1. Identify the API surface

Determine which header(s) define the API you want to bind and inspect the
include graph to plan the traverse list:

```sh
# List extern symbols in the header
grep -E "^extern" /usr/include/<header>.h

# Show the full include tree (bits/ sub-headers you may need)
clang -E -H /usr/include/<header>.h 2>&1 | head -80
```

Key questions:

- **Is there enough API surface?** A handful of functions and constants
  justifies its own partition. A single typedef does not.
- **Should sub-headers become separate partitions?** If an included header
  (e.g. `sched.h` included by `pthread.h`) is an independent POSIX API with
  its own non-trivial surface, split it into its own partition.

### 2. Add a `[[partition]]` to `bnd-posix.toml`

Edit `tests/fixtures/bnd-posix/bnd-posix.toml` and append a new partition
block:

```toml
# Partition N: <description>
[[partition]]
namespace = "posix.<name>"
library = "c"
headers = ["<header>.h"]
traverse = [
    "<header>.h",
    "bits/<sub-header>.h",          # constants, macros
    "bits/types/<type-header>.h",   # struct/typedef definitions
]
```

Rules:

| Field | Purpose |
|---|---|
| `namespace` | Must be `posix.<name>`. Determines the Rust module path (`posix::<name>`) |
| `library` | Always `"c"` — glibc 2.34+ consolidates all symbols into libc |
| `headers` | Top-level header(s) passed to clang for parsing |
| `traverse` | Allowlist of headers to extract symbols from. **Only** declarations in files ending with one of these suffixes are emitted. This is the most important field — missing entries cause "type not found" panics in windows-bindgen |

#### Building the traverse list

The traverse list is the hardest part. If a struct, typedef, or constant lives
in a `bits/` sub-header, that sub-header *must* appear in traverse or the
symbol will be silently omitted (or referenced but undefined, causing a
windows-bindgen panic).

Approach:

```sh
# 1. Start with just the top-level header
traverse = ["<header>.h"]

# 2. Run the generator and look for warnings/panics
cargo run -p bnd-posix-gen

# 3. If windows-bindgen panics with "type not found: posix.<name>.SomeType":
#    Find which sub-header defines SomeType:
grep -rn "SomeType" /usr/include/<header>.h /usr/include/bits/

# 4. Add that sub-header to traverse and repeat
```

Common patterns:
- Constants in `bits/<header>.h` or `bits/<header>-linux.h`
- Structs in `bits/struct_<name>.h` or `bits/types/struct_<name>.h`
- Typedefs in `bits/types/<name>.h`
- Architecture-specific definitions in `bits/<name>-arch.h`

#### Partition ordering

The **types** partition must remain first — it owns shared POSIX typedefs via
first-writer-wins deduplication. New partitions can go in any order after it,
but by convention they are appended at the end.

### 3. Run the generator

```sh
cargo run -p bnd-posix-gen
```

This produces:
- `bnd-posix/src/posix/<name>/mod.rs` — generated bindings
- Updated `bnd-posix/src/posix/mod.rs` — adds `pub mod <name>;`
- Updated `bnd-posix/Cargo.toml` — appends the feature below the
  `# generated features` marker

### 4. Add the feature to the default list

Open `bnd-posix/Cargo.toml` and add `"<name>"` to the `default` feature
array (keep it sorted alphabetically):

```toml
[features]
default = ["dirent", "dl", ..., "<name>", ..., "unistd"]
```

### 5. Add a doc comment to `lib.rs`

Add a line to the module list in `bnd-posix/src/lib.rs`:

```rust
//! - [`posix::<name>`] — <One-line description>
```

Keep the list sorted alphabetically.

### 6. Inspect the generated code

Review the generated `mod.rs` for correctness:

```sh
wc -l bnd-posix/src/posix/<name>/mod.rs
grep "pub unsafe fn" bnd-posix/src/posix/<name>/mod.rs | wc -l
grep "pub const" bnd-posix/src/posix/<name>/mod.rs | wc -l
```

Things to check:
- **Missing symbols** — compare against `grep "^extern"` output from step 1.
  Missing symbols usually mean a traverse entry is missing.
- **Variadic functions** — automatically skipped with a warning (no WinMD
  representation). Expected for functions like `open`, `fcntl`, `ioctl`,
  `printf`.
- **Inline functions** — no symbol in libc, cannot be represented as P/Invoke.
  Currently silently omitted.
- **Cross-partition references** — functions using types from other partitions
  generate `super::<other>::<Type>` references. These are auto-gated with
  `#[cfg(feature = "<other>")]`.
- **Function-pointer parameters** — emitted as `*const isize` (opaque WinMD
  convention). Callers cast `unsafe extern "C" fn(...)` to `*const isize`.
- **Opaque typedefs** — `typedef struct __foo BAR` where the struct is
  incomplete maps to `isize` (Copy/Clone, like Windows handles).

### 7. Write E2E tests

Create `bnd-posix/tests/<name>_e2e.rs`:

```rust
use bnd_posix::posix::{<name>};

#[test]
fn <name>_constants() {
    // Verify key constants have expected values
    assert_eq!(<name>::SOME_CONST, expected_value);
}

#[test]
fn <name>_basic_call() {
    // Call a simple function and verify it doesn't fail
    let ret = unsafe { <name>::some_function(...) };
    assert!(ret >= 0, "some_function failed: {ret}");
}

#[test]
fn struct_layout() {
    // Verify struct sizes match C layout
    assert_eq!(core::mem::size_of::<<name>::some_struct>(), expected_size);
}
```

Guidelines:
- Test constants against known values (from man pages or C headers).
- Call at least one function to verify the P/Invoke linkage actually works.
- Check struct sizes with `core::mem::size_of` against values obtained from
  a C `sizeof` expression.
- For output parameters, note that WinMD emits them as `*const T` even though
  they are logically `*mut T`. Pass `&mut val as *mut _ as *const _`, and
  add `#![allow(clippy::unnecessary_mut_passed)]` if clippy complains.
- For function-pointer parameters, cast with
  `some_fn as *const isize`.

Run:

```sh
cargo test -p bnd-posix
cargo clippy --all-targets
```

### 8. Update documentation

Three docs need updating:

1. **`docs/design/BnsPosix.md`** — add row to Modules table, Partition Config
   table, and Tests table.
2. **`docs/WIP.md`** — mark the partition as done in the candidate table and
   add a completed-section entry.
3. **`docs/design/systesting/bnd-posix.md`** — add a status row and a detailed
   section covering partition config, API surface, design decisions, and test
   table.

### Troubleshooting

| Symptom | Cause | Fix |
|---|---|---|
| `type not found: posix.<name>.SomeType` panic | `SomeType` is defined in a `bits/` sub-header not in traverse | Add the sub-header to the traverse list |
| Function missing from generated output | Function is variadic, inline, or in a header not in traverse | Check with `grep`; add header to traverse if needed |
| `super::<other>::<Type>` compile error | Cross-partition type needs the other feature enabled | Ensure both features are in the default list |
| Duplicate type conflict | Type already defined by an earlier partition (e.g. types) | Normal — dedup removes it. If the wrong partition owns it, reorder partitions |
| Clippy `unnecessary_mut_passed` | WinMD `*const` output-parameter convention | Add `#![allow(clippy::unnecessary_mut_passed)]` to test file |
| Struct size mismatch | Missing bitfield or flexible array member | Check C `sizeof` with a small C program; may need traverse additions |

## Tests

The crate includes integration tests across multiple test files in `tests/`
that call real libc functions through the generated bindings:

| File | Partition |
|---|---|
| `posixfile_e2e.rs` | Fcntl + Unistd (file I/O, constants, syscalls) |
| `stat_e2e.rs` | Stat (file size, mode, struct layout) |
| `mmap_e2e.rs` | Mmap (PROT_*/MAP_*/MS_* constants, mmap roundtrip, mprotect) |
| `dirent_e2e.rs` | Dirent (DT_* constants, opendir/readdir/closedir, dirfd) |
| `socket_e2e.rs` | Socket (SOCK_*/PF_*/MSG_* constants, struct layout, socket/bind/listen/send/recv) |
| `inet_e2e.rs` | Inet (IPPROTO_* constants, struct layout, htons/htonl, inet_pton/ntop) |
| `netdb_e2e.rs` | Netdb (AI_*/EAI_* constants, struct layout, getprotobyname, getaddrinfo) |
| `signal_e2e.rs` | Signal (SIG_*/SA_* constants, struct layout, sigset ops, sigaction, raise, sigprocmask, kill) |
| `dl_e2e.rs` | Dlfcn (RTLD_* constants, dlopen/dlclose, dlsym lookup, dlerror) |
| `errno_e2e.rs` | Errno (E* constants, __errno_location pointer, set/read, failed-syscall check) |
| `sched_e2e.rs` | Sched (SCHED_* constants, sched_yield, priority range, cpu_set_t/sched_param layout) |
| `time_e2e.rs` | Time (CLOCK_* constants, clock_gettime, gmtime, mktime roundtrip, struct tm layout) |
| `pthread_e2e.rs` | Pthread (PTHREAD_* constants, mutex lock/unlock, rwlock, spinlock, TLS keys, pthread_create/join, struct sizes) |
