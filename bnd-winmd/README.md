# bnd-winmd

C header → ECMA-335 `.winmd` metadata generator.

Parses C headers via **libclang**, extracts functions, structs, enums, typedefs, constants, and callback signatures, then emits a `.winmd` file using the [`windows-metadata`](https://crates.io/crates/windows-metadata) writer. The resulting `.winmd` can be fed to [`windows-bindgen`](https://crates.io/crates/windows-bindgen) to produce Rust FFI bindings.

## Pipeline

```
C Headers ──→ libclang (extract) ──→ Intermediate Model ──→ ECMA-335 (emit) ──→ .winmd
```

| Module | Role |
|---|---|
| `config` | TOML configuration loading (partitions, headers, traverse paths) |
| `extract` | libclang AST → intermediate model (`CType`, `FunctionDef`, `StructDef`, …) |
| `model` | Type-safe intermediate representation of C declarations |
| `emit` | Model → ECMA-335 WinMD bytes via `windows-metadata` writer |

## Library usage

Generate a `.winmd` file from a config (suitable for `build.rs`):

```rust
use std::path::Path;

bnd_winmd::run(Path::new("bnd-winmd.toml"), None).unwrap();
```

Or get the raw bytes without writing to disk:

```rust
use std::path::Path;

let winmd_bytes = bnd_winmd::generate(Path::new("bnd-winmd.toml")).unwrap();
```

## CLI

```
bnd-winmd [OPTIONS] [CONFIG]

Arguments:
  [CONFIG]  Path to bnd-winmd.toml [default: bnd-winmd.toml]

Options:
  -o, --output <PATH>  Output file path (overrides config)
```

## Configuration

```toml
[output]
name = "MyLib"
file = "mylib.winmd"

# Optional: extra include search paths
# include_paths = ["/usr/include/x86_64-linux-gnu"]

[[partition]]
namespace = "MyLib"
library = "mylib"
headers = ["mylib.h"]
traverse = ["mylib.h"]
```

Each `[[partition]]` maps a set of headers to a WinMD namespace and shared library name. The `traverse` list controls which headers' declarations are extracted (included headers outside this list provide types but not function exports).

## Prerequisites

- **libclang** — `apt install libclang-dev` (or equivalent)

## Example bindings built with bnd-winmd

- [`bnd-posix`](../bnd-posix/) — 15 POSIX modules from glibc system headers
- [`bnd-openssl`](../bnd-openssl/) — 8 OpenSSL 3.x partitions across libssl + libcrypto
