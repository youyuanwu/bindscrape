# System Header E2E Testing: POSIX Sockets

## Goal

Validate bindscrape against **POSIX socket headers** — `<sys/socket.h>`,
`<netinet/in.h>`, `<arpa/inet.h>`, and `<netdb.h>`. This is the first
system header target that requires **union support** (`ExplicitLayout` +
`FieldLayout`) and **anonymous nested types** — both currently unimplemented
features that sockets will force.

---

## Why Sockets

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

---

## Headers & Partitions

### Headers Involved

| Header | Key declarations |
|---|---|
| `<sys/socket.h>` | `struct sockaddr`, `socket()`, `bind()`, `listen()`, `accept()`, `connect()`, `send()`, `recv()`, `setsockopt()`, `getsockname()`, `AF_INET`, `AF_INET6`, `AF_UNIX`, `SOCK_STREAM`, `SOCK_DGRAM`, `SOL_SOCKET`, `SO_REUSEADDR` |
| `<netinet/in.h>` | `struct sockaddr_in`, `struct sockaddr_in6`, `struct in_addr`, `struct in6_addr`, `IPPROTO_TCP`, `IPPROTO_UDP`, `INADDR_ANY`, `INADDR_LOOPBACK`, `htons()`, `htonl()`, `ntohs()`, `ntohl()` |
| `<arpa/inet.h>` | `inet_pton()`, `inet_ntop()`, `inet_addr()` |
| `<netdb.h>` | `struct addrinfo`, `getaddrinfo()`, `freeaddrinfo()`, `gai_strerror()`, `AI_PASSIVE`, `AI_CANONNAME` |

### Proposed Partition Layout

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

### Alternative: 2-Partition Layout

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

---

## New Features Required

### Union Support (Not Implemented)

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

### Anonymous Nested Types (Partial)

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

### New System Typedefs

| Typedef | Canonical type | Winmd mapping |
|---|---|---|
| `socklen_t` | `unsigned int` | `U32` |
| `sa_family_t` | `unsigned short` | `U16` |
| `in_port_t` | `uint16_t` | `U16` |
| `in_addr_t` | `uint32_t` | `U32` |

---

## Expected Challenges

### 1. Union detection in `clang` crate

The `clang` crate exposes `EntityKind::UnionDecl` for top-level unions,
but it's unclear whether `sonar::find_unions()` has the same limitations
as `find_structs()` (missing unions without matching typedef). Likely
needs the same supplemental pass pattern.

For anonymous unions nested inside structs, the union appears as a child
entity with `EntityKind::UnionDecl` and `is_anonymous() == true`. Need
to walk struct children and handle this case.

### 2. `sockaddr` family polymorphism

The C pattern of casting between `sockaddr*`, `sockaddr_in*`, and
`sockaddr_in6*` doesn't translate to winmd. Each is a separate
TypeDef. Callers must use the specific struct and cast the pointer.
This is fine — it matches how `windows-bindgen` handles Windows socket
APIs.

### 3. `htons` / `htonl` — macros or inline functions

On Linux, `htons()` and friends may be `#define` macros calling
`__bswap_16` or may be `static inline` functions. If they resolve to
inline functions, they won't have symbols in `libc.so` and the P/Invoke
would fail at runtime. May need to skip these and test with
`inet_pton`/`inet_ntop` instead.

### 4. `struct addrinfo` — linked list with self-referential pointer

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

### 5. `__SOCKADDR_COMMON` macro

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

### 6. Conditional compilation / `#ifdef`

Socket headers use `#ifdef __USE_GNU`, `#ifdef __USE_MISC`, etc. to
expose additional APIs. The default clang parse may or may not define
these. The set of extracted functions may vary. Could require
`clang_args = ["-D__USE_GNU"]` in the config to get the full API.

### 7. `bits/` sub-headers

As with file I/O headers, the actual constants (`AF_INET`, `SOCK_STREAM`)
may be defined in `<bits/socket.h>` or `<asm/socket.h>`, not in
`<sys/socket.h>` directly. The traverse list may need to include these
sub-headers, or the constants won't be extracted.

---

## API Surface (Expected)

### PosixSocket (sys/socket.h)

**Structs**: `sockaddr` (16 bytes — `sa_family` + `sa_data[14]`)
**Functions**: `socket`, `bind`, `listen`, `accept`, `connect`, `send`,
`recv`, `sendto`, `recvfrom`, `setsockopt`, `getsockopt`, `getsockname`,
`getpeername`, `shutdown`, `close` (if re-exported)
**Constants**: `AF_INET`, `AF_INET6`, `AF_UNIX`, `AF_UNSPEC`,
`SOCK_STREAM`, `SOCK_DGRAM`, `SOCK_RAW`, `SOL_SOCKET`, `SO_REUSEADDR`,
`SO_REUSEPORT`, `SO_KEEPALIVE`, `SHUT_RD`, `SHUT_WR`, `SHUT_RDWR`

### PosixSocket.Inet (netinet/in.h)

**Structs**: `in_addr` (4 bytes), `in6_addr` (16 bytes, contains union),
`sockaddr_in` (16 bytes), `sockaddr_in6` (28 bytes)
**Constants**: `IPPROTO_TCP`, `IPPROTO_UDP`, `IPPROTO_IP`,
`INADDR_ANY`, `INADDR_LOOPBACK`, `INADDR_BROADCAST`,
`INET_ADDRSTRLEN`, `INET6_ADDRSTRLEN`

### PosixSocket.Arpa (arpa/inet.h)

**Functions**: `inet_pton`, `inet_ntop`, `inet_addr`, `inet_ntoa`

### PosixSocket.Netdb (netdb.h)

**Structs**: `addrinfo` (self-referential linked list)
**Functions**: `getaddrinfo`, `freeaddrinfo`, `gai_strerror`
**Constants**: `AI_PASSIVE`, `AI_CANONNAME`, `AI_NUMERICHOST`,
`AI_NUMERICSERV`, `NI_MAXHOST`, `NI_MAXSERV`

---

## Proposed E2E Tests

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

## Dependencies

- No additional packages — `sys/socket.h`, `netinet/in.h`, `arpa/inet.h`,
  `netdb.h` are part of `libc6-dev` (already present)
- Functions live in libc — `cargo:rustc-link-lib=dylib=c`

---

## Implementation Order

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

## Implementation Steps

1. ⬜ Implement union support in model + extract + emit
2. ⬜ Implement anonymous nested type synthetic naming
3. ✅ System typedefs (`socklen_t`, `sa_family_t`, `in_port_t`,
   `in_addr_t`) auto-resolved via `CType::Named { resolved }` — no changes needed
4. ⬜ Create `tests/fixtures/posixsocket/posixsocket.toml`
5. ⬜ Add roundtrip tests in `roundtrip_posixsocket.rs`
6. ⬜ Create `tests/e2e-posixsocket/` crate
7. ⬜ Handle `htons`/`htonl` (skip if inline, or provide wrapper)
8. ⬜ Handle conditional compilation flags if needed
9. ⬜ Iterate on traverse paths for `bits/` sub-headers
10. ⬜ Add `e2e-posixsocket` to workspace members
