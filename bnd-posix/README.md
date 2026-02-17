# bnd-posix

Rust FFI bindings for POSIX system APIs on Linux, auto-generated from C system headers via [`bnd-winmd`](../bnd-winmd/) and [`windows-bindgen`](https://crates.io/crates/windows-bindgen).

**Do not edit `src/posix/` by hand** — run `cargo run -p bnd-posix-gen` to regenerate.

## Modules

| Module | APIs |
|---|---|
| `dirent` | `opendir`, `readdir`, `closedir`, `DT_*` constants |
| `dl` | `dlopen`, `dlclose`, `dlsym`, `dlerror`, `RTLD_*` |
| `errno` | `__errno_location`, `E*` error constants |
| `fcntl` | `creat`, `lockf`, `O_*` constants |
| `inet` | `inet_pton`, `htons`, `sockaddr_in`, `IPPROTO_*` |
| `mmap` | `mmap`, `munmap`, `mprotect`, `MAP_*`/`PROT_*` |
| `netdb` | `getaddrinfo`, `gethostbyname`, `addrinfo`, `AI_*` |
| `pthread` | `pthread_create`, `pthread_mutex_lock`, `PTHREAD_*` |
| `sched` | `sched_yield`, `sched_setscheduler`, `SCHED_*` |
| `signal` | `sigaction`, `kill`, `raise`, `SIG*`/`SA_*` |
| `socket` | `socket`, `bind`, `listen`, `accept`, `AF_*`/`SOCK_*` |
| `stat` | `stat`, `chmod`, `mkdir`, `struct stat` |
| `time` | `clock_gettime`, `nanosleep`, `gmtime`, `CLOCK_*` |
| `types` | `uid_t`, `pid_t`, `mode_t`, `off_t`, … |
| `unistd` | `read`, `write`, `close`, `fork`, … |

Each module is a Cargo feature (all enabled by default).

## Example

```rust
use bnd_posix::posix::{fcntl, unistd};

let path = c"/tmp/example.txt";
let fd = unsafe { fcntl::creat(path.as_ptr(), 0o644) };
assert!(fd >= 0);
unsafe { unistd::close(fd) };
```

All function bindings are `unsafe` — they call directly into libc.
