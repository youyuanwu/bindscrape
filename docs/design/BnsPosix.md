# bns-posix — POSIX System Bindings via WinMD

`bns-posix` provides Rust bindings for POSIX file I/O and process APIs on
Linux, generated from C system headers through the
**bindscrape → WinMD → windows-bindgen** pipeline.

This is the first *product* crate built on bindscrape, demonstrating that the
C-header-to-WinMD approach scales beyond test fixtures to real system APIs.

## Modules

| Module | Header(s) | Functions | Constants | Structs |
|---|---|---|---|---|
| `PosixFile::Fcntl`  | `fcntl.h` | 4 | ~60 | — |
| `PosixFile::Stat`   | `sys/stat.h`, `bits/struct_stat.h`, `bits/types/struct_timespec.h` | 17 | 4 | `stat`, `timespec` |
| `PosixFile::Unistd` | `unistd.h` | 103 | ~23 | — |

### Usage

```rust
use bns_posix::PosixFile::{Fcntl, Stat, Unistd};

// Create a file
let path = c"/tmp/example.txt";
let fd = unsafe { Fcntl::creat(path.as_ptr(), 0o644) };
assert!(fd >= 0);

// Write
let data = b"hello";
unsafe { Unistd::write(fd, data.as_ptr().cast(), data.len() as u64) };

// Stat
let mut st = Stat::stat::default();
unsafe { Stat::fstat(fd, &mut st as *mut _ as *const _) };
assert_eq!(st.st_size, 5);

// Close
unsafe { Unistd::close(fd) };
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
  │                              ├── PosixFile/              │
  │                              │   ├── mod.rs              │
  │                              │   ├── Fcntl/mod.rs        │
  │                              │   ├── Stat/mod.rs         │
  │                              │   └── Unistd/mod.rs       │
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
   `mod.rs` per namespace under `src/PosixFile/`, with `#[cfg(feature)]`
   gating on each sub-module. It also appends feature definitions to
   `Cargo.toml` after the `# generated features` marker.
3. The intermediate `.winmd` is deleted — `bns-posix` is a pure Rust crate
   with no build-time code generation.

### Why namespace modules?

Multiple partitions extract overlapping system types (`off_t`, `mode_t`,
`SEEK_SET`, etc.). Without `--flat`, windows-bindgen generates nested
`pub mod` modules per namespace, so each partition's types live in their
own module. Cross-partition references use `super::Stat::mode_t` etc.

## Partition Config

The TOML config lives at `tests/fixtures/bns-posix/bns-posix.toml`
and defines three partitions:

| Partition | Namespace | Headers traversed |
|---|---|---|
| Fcntl | `PosixFile.Fcntl` | `fcntl.h` |
| Unistd | `PosixFile.Unistd` | `unistd.h` |
| Stat | `PosixFile.Stat` | `sys/stat.h`, `bits/struct_stat.h`, `bits/types/struct_timespec.h` |

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

## Extending

To add more POSIX APIs (e.g., `sys/socket.h`, `pthread.h`):

1. Add a new `[[partition]]` to `bns-posix.toml` with the desired headers.
2. Run `cargo run -p bns-posix-gen` — bindscrape extracts the new partition,
   windows-bindgen adds a new `src/PosixFile/<Name>/mod.rs` and appends
   the feature to `Cargo.toml`.
3. Add the new feature to the `default` list in `Cargo.toml`.
4. `lib.rs` already does `pub mod PosixFile;` which picks up new sub-modules
   automatically.

## Tests

The crate includes 15 integration tests in `tests/posixfile_e2e.rs` that
call real libc functions through the generated bindings:

- **Constants** — `O_RDONLY`, `SEEK_*`, `R_OK`/`W_OK`/`X_OK`/`F_OK`
- **Syscalls** — `getpid()`, `getuid()`
- **File I/O** — `creat` → `write` → `read` → `close` round-trip
- **Stat** — `fstat` file size and mode checks
- **Layout** — `struct stat` (144 bytes), `struct timespec` (16 bytes)
