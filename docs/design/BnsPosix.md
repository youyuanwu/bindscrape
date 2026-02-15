# bns-posix — POSIX System Bindings via WinMD

`bns-posix` provides Rust bindings for POSIX file I/O and process APIs on
Linux, generated from C system headers through the
**bindscrape → WinMD → windows-bindgen** pipeline.

This is the first *product* crate built on bindscrape, demonstrating that the
C-header-to-WinMD approach scales beyond test fixtures to real system APIs.

## Modules

| Module | Header(s) | Functions | Constants | Structs |
|---|---|---|---|---|
| `posix::dirent` | `dirent.h`, `bits/dirent.h` | 12 | ~11 | `dirent` |
| `posix::fcntl`  | `fcntl.h` | 4 | ~60 | — |
| `posix::inet`   | `netinet/in.h`, `arpa/inet.h` | 20 | ~75 | `sockaddr_in`, `sockaddr_in6`, `in_addr`, `in6_addr` (+unions) |
| `posix::mmap`   | `sys/mman.h`, `bits/mman-linux.h`, `bits/mman-map-flags-generic.h` | 13 | ~62 | — |
| `posix::netdb`  | `netdb.h`, `bits/netdb.h` | 56 | ~32 | `addrinfo`, `hostent`, `servent`, `protoent`, `netent` |
| `posix::signal` | `signal.h`, `bits/sigaction.h`, `bits/signum-*.h`, `bits/sigcontext.h`, `bits/types/*` | 30 | ~50 | `sigaction` (union), `siginfo_t` (nested unions), `__sigset_t`, `sigcontext`, `stack_t` |
| `posix::socket` | `sys/socket.h`, `bits/socket.h`, `bits/socket_type.h`, `bits/socket-constants.h` | 20 | ~102 | `sockaddr`, `sockaddr_storage`, `msghdr`, `iovec`, `cmsghdr`, `linger` |
| `posix::stat`   | `sys/stat.h`, `bits/struct_stat.h`, `bits/types/struct_timespec.h` | 17 | 4 | `stat`, `timespec` |
| `posix::types`  | `sys/types.h`, `bits/types.h` | — | — | `__fsid_t` + 94 shared typedefs (`uid_t`, `pid_t`, `mode_t`, …) |
| `posix::unistd` | `unistd.h` | 103 | ~23 | — |

### Usage

```rust
use bns_posix::posix::{fcntl, stat, unistd};

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

The bindings are produced by a separate **generator crate** (`bns-posix-gen`)
and checked into the `bns-posix` source tree — there is no `build.rs`.

```
  bns-posix-gen (cargo run -p bns-posix-gen)
  ┌─────────────────────────────────────────────────────────┐
  │                                                         │
  │  bns-posix.toml ──▶ bindscrape ──▶ .winmd               │
  │                                      │                  │
  │                          windows-bindgen --package       │
  │                                      │                  │
  │                                      ▼                  │
  │                              bns-posix/src/              │
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
cargo run -p bns-posix-gen
```

1. **bindscrape** parses `bns-posix.toml`, invokes clang on system headers,
   extracts types/functions/constants, and writes a temporary `.winmd` file.
2. **windows-bindgen `--package`** reads the `.winmd` and generates one
   `mod.rs` per namespace under `src/posix/`, with `#[cfg(feature)]`
   gating on each sub-module. It also appends feature definitions to
   `Cargo.toml` after the `# generated features` marker.
3. The intermediate `.winmd` is deleted — `bns-posix` is a pure Rust crate
   with no build-time code generation.

### Why namespace modules?

Multiple partitions reference overlapping system types (`off_t`, `mode_t`,
`uid_t`, etc.). A dedicated **types** partition (`posix.types`) owns these
shared typedefs. During generation, bindscrape deduplicates: the types
partition is processed first (first-writer-wins for typedefs), and later
partitions' duplicate copies are removed. Function signatures in other
partitions use cross-partition TypeRefs (e.g. `super::types::__uid_t`).

## Partition Config

The TOML config lives at `tests/fixtures/bns-posix/bns-posix.toml`
and defines ten partitions:

| Partition | Namespace | Headers traversed |
|---|---|---|
| Types | `posix.types` | `sys/types.h`, `bits/types.h` |
| Dirent | `posix.dirent` | `dirent.h`, `bits/dirent.h` |
| Fcntl | `posix.fcntl` | `fcntl.h` |
| Inet | `posix.inet` | `netinet/in.h`, `arpa/inet.h` |
| Mmap | `posix.mmap` | `sys/mman.h`, `bits/mman-linux.h`, `bits/mman-map-flags-generic.h` |
| Netdb | `posix.netdb` | `netdb.h`, `bits/netdb.h` |
| Signal | `posix.signal` | `signal.h`, `bits/sigaction.h`, `bits/signum-generic.h`, `bits/signum-arch.h`, `bits/sigcontext.h`, `bits/types/__sigset_t.h`, `bits/types/siginfo_t.h`, `bits/types/__sigval_t.h`, `bits/types/stack_t.h`, `bits/types/struct_sigstack.h` |
| Socket | `posix.socket` | `sys/socket.h`, `bits/socket.h`, `bits/socket_type.h`, `bits/socket-constants.h`, `bits/types/struct_iovec.h` |
| Stat | `posix.stat` | `sys/stat.h`, `bits/struct_stat.h`, `bits/types/struct_timespec.h` |
| Unistd | `posix.unistd` | `unistd.h` |

## Challenges Solved

These are issues encountered while building real system bindings and fixed
in bindscrape core (see [bns-posix.md](systesting/bns-posix.md) for details):

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
    types in bns-posix.
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

## Extending

To add more POSIX APIs (e.g., `sys/socket.h`, `pthread.h`):

1. Add a new `[[partition]]` to `bns-posix.toml` with the desired headers.
2. Run `cargo run -p bns-posix-gen` — bindscrape extracts the new partition,
   windows-bindgen adds a new `src/posix/<name>/mod.rs` and appends
   the feature to `Cargo.toml`.
3. Add the new feature to the `default` list in `Cargo.toml`.
4. `lib.rs` already does `pub mod posix;` which picks up new sub-modules
   automatically.

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
