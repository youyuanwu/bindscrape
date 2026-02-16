# OpenSSL: System Library Testing

## Goal

Validate bnd-winmd against **real OpenSSL 3.x headers** — `<openssl/evp.h>`,
`<openssl/ssl.h>`, `<openssl/bio.h>`, `<openssl/err.h>`, `<openssl/rand.h>`,
`<openssl/crypto.h>`, `<openssl/bn.h>`, `<openssl/sha.h>`. This exercises
the pipeline against a large, widely-used C library with:

- **~130 opaque typedef-to-incomplete-struct** patterns (`SSL *`, `EVP_MD_CTX *`,
  `BIO *`, `BIGNUM *`, etc.) — all pointers to forward-declared structs
  with no public field definitions
- **Two shared libraries** — `libssl.so` (518 exported symbols) and
  `libcrypto.so` (5363 exported symbols), requiring multi-library partitions
- **Deprecation-gated declarations** — `OSSL_DEPRECATEDIN_3_0` macro wraps
  legacy APIs inside `#ifndef OPENSSL_NO_DEPRECATED_3_0`
- **Callback function-pointer typedefs** — `pem_password_cb`,
  `BIO_callback_fn`, etc.
- **Macro-defined constants** — `SHA_DIGEST_LENGTH`, `EVP_MAX_MD_SIZE`,
  `SSL_*` error codes, `NID_*` identifiers
- **Non-opaque structs under `#ifdef`** — `SHA_CTX`, `SHA256_CTX`,
  `SHA512_CTX` are fully defined but deprecated behind `OPENSSL_NO_DEPRECATED_3_0`

This is the second real-world third-party library tested (after zlib),
scaling from a small single-library API to a large two-library ecosystem.

---

## Why OpenSSL

- **Massive API surface** — tests extraction at scale (~5800 exported symbols
  across two libraries, ~133 headers)
- **Opaque-pointer-dominant design** — OpenSSL 3.x made most structs opaque.
  120 `typedef struct foo_st FOO` declarations in `types.h` with no public
  fields. Tests the incomplete-struct → `isize` opaque typedef path at scale
- **Two libraries (`libssl`, `libcrypto`)** — validates multi-library
  partitioning where different partitions link different `.so` files
- **Callback function pointers** — `pem_password_cb`, `BIO_callback_fn_ex`,
  `SSL_verify_cb` exercise delegate emission
- **Widely available** — `libssl-dev` on every Linux distro; CI-friendly
- **Well-known test vectors** — SHA-256 digests, AES encryption, RAND_bytes
  have deterministic expected outputs for E2E validation
- **Deprecation macros** — `OSSL_DEPRECATEDIN_3_0` tests whether clang
  sees declarations inside `#ifndef` guards (it does — macros expand to
  `__attribute__((deprecated))`, not exclusion)

---

## Architecture

### Scope Decision: Subset vs Full

OpenSSL has 133 headers and ~5800 functions. **We will NOT attempt full
coverage.** Instead, we target a curated subset of 8 headers that
exercise the interesting patterns while remaining testable (a 9th,
`err.h`, was planned but skipped due to LHASH macro issues):

| Header group | Library | Key pattern exercised |
|---|---|---|
| `openssl/types.h` | — | 120 opaque typedefs (no functions) |
| `openssl/crypto.h` | `crypto` | Version query, memory functions |
| `openssl/err.h` | `crypto` | ~~Skipped (LHASH macro issue)~~ |
| `openssl/rand.h` | `crypto` | Random bytes, simple int-returning API |
| `openssl/bn.h` | `crypto` | Opaque BIGNUM pointers, alloc/free |
| `openssl/evp.h` | `crypto` | Digest + cipher via opaque contexts, `const EVP_MD *` returns |
| `openssl/sha.h` | `crypto` | Non-opaque structs (deprecated), one-shot hash functions |
| `openssl/bio.h` | `crypto` | I/O abstraction, memory BIO, callback typedefs |
| `openssl/ssl.h` | `ssl` | TLS context/connection lifecycle, multi-step opaque workflow |

### Two-Library Partitioning

Unlike zlib (single `libz.so`) and bnd-posix (single `libc.so`), OpenSSL
splits its API across two shared libraries:

- **`libcrypto.so`** — cryptographic primitives (EVP, BN, SHA, RAND, ERR, BIO)
- **`libssl.so`** — TLS protocol (SSL_CTX, SSL, SSL_METHOD)

Each partition's `library` field must match the correct `.so`:

```
openssl/types.h ──────────────────────────────────┐
openssl/crypto.h  ──→  openssl.crypto  (crypto)   │
openssl/err.h     ──→  (skipped — LHASH issue)     │
openssl/rand.h    ──→  openssl.rand    (crypto)   │
openssl/bn.h      ──→  openssl.bn      (crypto)   ├── openssl.winmd
openssl/evp.h     ──→  openssl.evp     (crypto)   │
openssl/sha.h     ──→  openssl.sha     (crypto)   │
openssl/bio.h     ──→  openssl.bio     (crypto)   │
openssl/ssl.h     ──→  openssl.ssl     (ssl)      │
                                                   │
                          openssl.winmd ◄──────────┘
                               │
                     windows-bindgen --sys --package
                               │
                               ▼
                     bnd-openssl/src/openssl/       ──→ links libssl + libcrypto
                     ├── mod.rs                         #[cfg(feature)] pub mod …
                     ├── types/mod.rs                   opaque typedefs
                     ├── crypto/mod.rs                  version, memory
                     ├── evp/mod.rs                     digest, cipher
                     ├── ssl/mod.rs                     TLS protocol
                     └── …
```

### Crate Layout

Follows the same **generator + product crate** pattern as bnd-posix.
A separate generator crate (`bnd-openssl-gen`) runs bnd-winmd + windows-bindgen
`--package` offline, producing a checked-in `bnd-openssl/` source tree.

```
bnd-openssl-gen/                    ← generator (cargo run -p bnd-openssl-gen)
├── Cargo.toml
├── src/
│   ├── lib.rs                      ← generate(&Path) → bnd-winmd → windows-bindgen --package
│   └── main.rs                     ← fn main() { generate(&bnd_openssl_dir) }
└── tests/
    └── up_to_date.rs               ← regen-to-tempdir and diff

bnd-openssl/                        ← product crate
├── Cargo.toml                      ← hand-written header + # generated features
├── build.rs                        ← emits cargo:rustc-link-lib=crypto/ssl
├── src/
│   ├── lib.rs                      ← hand-written: pub mod openssl;
│   └── openssl/                    ← GENERATED by windows-bindgen --package
│       ├── mod.rs                  ← #[cfg(feature="types")] pub mod types; …
│       ├── types/mod.rs            ← opaque typedefs (EVP_MD, SSL, BIO, …)
│       ├── crypto/mod.rs           ← OpenSSL_version, OPENSSL_VERSION_*
│       ├── rand/mod.rs             ← RAND_bytes, RAND_status, …
│       ├── bn/mod.rs               ← BN_new, BN_free, BN_set_word, …
│       ├── evp/mod.rs              ← EVP_MD_CTX_new, EVP_DigestInit_ex, …
│       ├── sha/mod.rs              ← SHA256, SHA_DIGEST_LENGTH, SHA_CTX, …
│       ├── bio/mod.rs              ← BIO_new, BIO_read, BIO_write, …
│       └── ssl/mod.rs              ← SSL_CTX_new, SSL_connect, TLS_*, …

tests/fixtures/openssl/
└── openssl.toml                    ← multi-partition config

bnd-winmd/tests/
└── roundtrip_openssl.rs            ← winmd roundtrip tests
```

To regenerate:

```sh
cargo run -p bnd-openssl-gen
```

---

## Config

**`tests/fixtures/openssl/openssl.toml`**:

```toml
include_paths = ["/usr/include/x86_64-linux-gnu", "/usr/include"]

[output]
name = "openssl"
file = "openssl.winmd"

# Partition 1: Opaque type forward declarations
# 120 typedef struct foo_st FOO patterns — no functions
[[partition]]
namespace = "openssl.types"
library = "crypto"
headers = ["openssl/types.h"]
traverse = ["openssl/types.h"]

# Partition 2: Library version + core utilities
[[partition]]
namespace = "openssl.crypto"
library = "crypto"
headers = ["openssl/crypto.h"]
traverse = ["openssl/crypto.h", "bits/types/struct_tm.h", "bits/types/struct_FILE.h"]

# Partition 3: Random number generation
[[partition]]
namespace = "openssl.rand"
library = "crypto"
headers = ["openssl/rand.h"]
traverse = ["openssl/rand.h"]

# Partition 4: Error queue
# NOTE: openssl/err.h uses DEFINE_LHASH_OF_INTERNAL which generates
# a struct with an inline union field (lh_ERR_STRING_DATA_dummy) that
# bnd-winmd extracts but cannot emit correctly. Skipped for now.
# [[partition]]
# namespace = "openssl.err"
# library = "crypto"
# headers = ["openssl/err.h"]
# traverse = ["openssl/err.h"]

# Partition 5: Big number arithmetic (opaque BIGNUM)
[[partition]]
namespace = "openssl.bn"
library = "crypto"
headers = ["openssl/bn.h"]
traverse = ["openssl/bn.h"]

# Partition 6: EVP high-level crypto (digest, cipher)
[[partition]]
namespace = "openssl.evp"
library = "crypto"
headers = ["openssl/evp.h"]
traverse = ["openssl/evp.h"]

# Partition 7: SHA one-shot hash + context structs
[[partition]]
namespace = "openssl.sha"
library = "crypto"
headers = ["openssl/sha.h"]
traverse = ["openssl/sha.h"]

# Partition 8: BIO I/O abstraction
[[partition]]
namespace = "openssl.bio"
library = "crypto"
headers = ["openssl/bio.h"]
traverse = ["openssl/bio.h"]

# Partition 9: TLS protocol — links libssl
[[partition]]
namespace = "openssl.ssl"
library = "ssl"
headers = ["openssl/ssl.h"]
traverse = ["openssl/ssl.h", "openssl/tls1.h"]
```

Key points:

- **`include_paths = ["/usr/include/x86_64-linux-gnu", "/usr/include"]`** —
  OpenSSL headers live under `/usr/include/openssl/`. The arch-specific
  path is needed for `opensslconf.h` and `configuration.h`, and for
  glibc sub-headers like `bits/types/struct_tm.h` referenced transitively
  by `openssl/crypto.h`
- **`library = "crypto"` vs `"ssl"`** — partitions must specify which
  `.so` their symbols live in. Most APIs are in libcrypto; only `SSL_*`,
  `TLS_*` methods are in libssl
- **Types partition first** — opaque typedefs like `EVP_MD`, `SSL_CTX`,
  `BIO` are forward-declared in `types.h`. Placing this partition first
  ensures first-writer-wins registration for the TypeRegistry, and later
  partitions reference them via cross-partition TypeRef
- **Crypto traverse includes glibc sub-headers** — `openssl/crypto.h`
  references `struct tm` (from `<time.h>`) and `FILE` (from `<stdio.h>`),
  which require `bits/types/struct_tm.h` and `bits/types/struct_FILE.h`
  in the traverse to resolve complete struct definitions
- **SSL traverse includes `tls1.h`** — `openssl/ssl.h` typedefs
  `tls_session_ticket_ext_st` which is defined in `openssl/tls1.h`;
  without this extra traverse entry, bnd-winmd fails with "type not found"
- **err partition skipped** — `DEFINE_LHASH_OF_INTERNAL(ERR_STRING_DATA)`
  generates an inline `union lh_ERR_STRING_DATA_dummy` field that
  bnd-winmd extracts as a struct field type but windows-bindgen cannot
  resolve. See **Known Limitations** below

---

## API Surface (Expected)

### openssl.types Partition (~120 opaque typedefs)

All `typedef struct foo_st FOO` patterns. No functions, no constants
(aside from macro guards). Key types:

| Typedef | Internal struct | Used by |
|---|---|---|
| `EVP_MD` | `evp_md_st` | Digest algorithms (`EVP_sha256()`) |
| `EVP_MD_CTX` | `evp_md_ctx_st` | Digest context |
| `EVP_CIPHER` | `evp_cipher_st` | Cipher algorithms (`EVP_aes_256_cbc()`) |
| `EVP_CIPHER_CTX` | `evp_cipher_ctx_st` | Cipher context |
| `EVP_PKEY` | `evp_pkey_st` | Public/private key |
| `EVP_PKEY_CTX` | `evp_pkey_ctx_st` | Key operation context |
| `SSL` | `ssl_st` | TLS connection |
| `SSL_CTX` | `ssl_ctx_st` | TLS configuration |
| `SSL_METHOD` | — | Protocol method (opaque) |
| `BIO` | `bio_st` | I/O abstraction |
| `BIO_METHOD` | — | BIO type descriptor |
| `BIGNUM` | `bignum_st` | Arbitrary-precision integer |
| `BN_CTX` | `bignum_ctx` | Temp BIGNUM pool |
| `X509` | `x509_st` | Certificate |
| `OSSL_LIB_CTX` | `ossl_lib_ctx_st` | Library context (OpenSSL 3.x) |
| `OSSL_PARAM` | `ossl_param_st` | Provider parameter |

All of these map to opaque `isize` typedefs in the WinMD (same pattern
as `DIR` in the dirent partition) because the structs are incomplete.

### openssl.crypto Partition (crypto.h)

**Functions (~15)**: `OpenSSL_version`, `OPENSSL_version_major`,
`OPENSSL_version_minor`, `OPENSSL_version_patch`,
`OPENSSL_version_pre_release`, `OPENSSL_version_build_metadata`,
`CRYPTO_malloc`, `CRYPTO_free`, `OPENSSL_hexstr2buf`, `OPENSSL_buf2hexstr`

**Constants (~5)**: `OPENSSL_VERSION=0`, `OPENSSL_VERSION_STRING=6`,
`SSLEAY_VERSION`

### ~~openssl.err Partition (err.h)~~ — Skipped

> **Skipped due to LHASH macro issue.** `DEFINE_LHASH_OF_INTERNAL(ERR_STRING_DATA)`
> generates inline union types that bnd-winmd cannot emit. See **Known Limitations**.

**Functions (~20)**: `ERR_get_error`, `ERR_peek_error`,
`ERR_error_string`, `ERR_error_string_n`, `ERR_get_error_all`,
`ERR_clear_error`, `ERR_print_errors_fp`, `ERR_reason_error_string`,
`ERR_func_error_string`, `ERR_lib_error_string`

### openssl.rand Partition (rand.h)

**Functions (~8)**: `RAND_bytes`, `RAND_priv_bytes`, `RAND_seed`,
`RAND_status`, `RAND_add`, `RAND_bytes_ex`, `RAND_priv_bytes_ex`

Small, clean API. Good first-pass validation target.

### openssl.bn Partition (bn.h)

**Functions (~80)**: `BN_new`, `BN_free`, `BN_clear_free`, `BN_num_bits`,
`BN_set_word`, `BN_get_word`, `BN_bn2hex`, `BN_hex2bn`, `BN_bn2dec`,
`BN_dec2bn`, `BN_add`, `BN_sub`, `BN_mul`, `BN_div`, `BN_mod`,
`BN_cmp`, `BN_is_zero`, `BN_is_one`, `BN_is_negative`, `BN_rand`,
`BN_generate_prime_ex`, ...

**Structs**: `BN_GENCB` (non-opaque, has callback field)

**Constants**: `BN_RAND_TOP_*`, `BN_RAND_BOTTOM_*`

### openssl.evp Partition (evp.h) — Largest partition

**Functions (~200+)**: Digest (`EVP_MD_CTX_new`, `EVP_MD_CTX_free`,
`EVP_DigestInit_ex`, `EVP_DigestUpdate`, `EVP_DigestFinal_ex`,
`EVP_sha1`, `EVP_sha256`, `EVP_sha512`), Cipher (`EVP_CIPHER_CTX_new`,
`EVP_CIPHER_CTX_free`, `EVP_EncryptInit_ex`, `EVP_EncryptUpdate`,
`EVP_EncryptFinal_ex`, `EVP_DecryptInit_ex`, `EVP_DecryptUpdate`,
`EVP_DecryptFinal_ex`, `EVP_aes_128_cbc`, `EVP_aes_256_cbc`,
`EVP_aes_256_gcm`), PKEY, KDF, MAC, ...

**Constants (~30)**: `EVP_MAX_MD_SIZE`, `EVP_MAX_KEY_LENGTH`,
`EVP_MAX_IV_LENGTH`, `EVP_MAX_BLOCK_LENGTH`

### openssl.sha Partition (sha.h)

**Structs (3, deprecated)**: `SHA_CTX` (SHAstate_st), `SHA256_CTX`
(SHA256state_st), `SHA512_CTX` (SHA512state_st) — fully defined with
public fields, but wrapped in `#ifndef OPENSSL_NO_DEPRECATED_3_0`

**Functions (~6 non-deprecated)**: `SHA1`, `SHA224`, `SHA256`, `SHA384`,
`SHA512` (one-shot convenience functions, NOT deprecated)

**Functions (~15 deprecated)**: `SHA1_Init`/`Update`/`Final`,
`SHA256_Init`/`Update`/`Final`, `SHA512_Init`/`Update`/`Final`

**Constants**: `SHA_DIGEST_LENGTH=20`, `SHA256_DIGEST_LENGTH=32`,
`SHA384_DIGEST_LENGTH=48`, `SHA512_DIGEST_LENGTH=64`

### openssl.bio Partition (bio.h)

**Functions (~60)**: `BIO_new`, `BIO_free`, `BIO_read`, `BIO_write`,
`BIO_s_mem`, `BIO_new_mem_buf`, `BIO_ctrl_pending`, `BIO_gets`,
`BIO_puts`, `BIO_push`, `BIO_pop`, `BIO_set_flags`, `BIO_clear_flags`,
`BIO_s_file`, `BIO_new_file`, `BIO_s_socket`, ...

**Callback typedefs**: `BIO_callback_fn`, `BIO_callback_fn_ex`,
`BIO_info_cb`

**Constants**: `BIO_CTRL_*`, `BIO_C_*`, `BIO_FLAGS_*`, `BIO_NOCLOSE`,
`BIO_CLOSE`

### openssl.ssl Partition (ssl.h) — Links `libssl`

**Functions (~200+)**: `SSL_CTX_new`, `SSL_CTX_free`, `SSL_new`,
`SSL_free`, `SSL_set_fd`, `SSL_connect`, `SSL_accept`, `SSL_read`,
`SSL_write`, `SSL_shutdown`, `SSL_get_error`, `TLS_client_method`,
`TLS_server_method`, `SSL_CTX_set_verify`, `SSL_CTX_use_certificate_file`,
`SSL_CTX_use_PrivateKey_file`, `SSL_CTX_load_verify_locations`, ...

**Constants (~100+)**: `SSL_ERROR_*`, `SSL_FILETYPE_PEM`,
`SSL_FILETYPE_ASN1`, `SSL_VERIFY_NONE`, `SSL_VERIFY_PEER`,
`SSL_OP_*` option flags

**Callback typedefs**: `SSL_verify_cb`, `SSL_CTX_info_callback`, ...

---

## Challenges Encountered

| # | Challenge | Outcome |
|---|---|---|
| 1 | ~130 opaque typedefs at scale | Worked — existing incomplete-struct → `isize` path handled all automatically |
| 2 | Two shared libraries (`crypto`, `ssl`) | Worked — needed `build.rs` with `cargo:rustc-link-lib` directives |
| 3 | `__owur` / deprecation attributes | No issue — clang strips attributes from AST |
| 4 | `STACK_OF(TYPE)` macro types | No issue — clang expands to real struct names, opaque ones map to `isize` |
| 5 | `const EVP_MD *` returns | Worked — `PtrConst` → `*mut isize` as expected |
| 6 | Variadic functions | Auto-skipped with warnings, same as bnd-posix |
| 7 | Deprecated structs behind `#ifdef` | Visible by default on Ubuntu (`OPENSSL_NO_DEPRECATED_3_0` not defined) |
| 8 | Macro aliases (`EVP_MD_CTX_create`) | Not extracted (expected) — underlying real functions are available |
| 9 | Callback typedefs | Extracted as WinMD delegates correctly |
| 10 | Cross-partition include coupling | Resolved via TypeRegistry cross-partition TypeRef, same as bnd-posix |
| 11 | `struct tm` / `FILE` in crypto.h | Fixed by adding `bits/types/struct_tm.h`, `struct_FILE.h` to traverse |
| 12 | `tls_session_ticket_ext_st` in ssl.h | Fixed by adding `openssl/tls1.h` to ssl traverse |
| 13 | LHASH inline unions in err.h | **Unresolved** — err partition skipped. See Known Limitations |

---

## Partition Ordering Strategy

Partitions should be ordered from most-depended-on to least-depended-on:

1. **openssl.types** — owns all opaque typedefs (130+). Must be first
   for first-writer-wins dedup.
2. **openssl.crypto** — core utilities, few cross-refs.
3. ~~**openssl.err**~~ — skipped (LHASH macro issue).
4. **openssl.rand** — simple API, references `OSSL_LIB_CTX`.
5. **openssl.bn** — references `BN_CTX`, `BN_GENCB` (may be in types).
6. **openssl.evp** — references `EVP_MD`, `EVP_CIPHER`, `BIO`, `ENGINE`.
7. **openssl.sha** — self-contained (non-deprecated structs + one-shot
   functions).
8. **openssl.bio** — references `BIO_METHOD`, callback typedefs.
9. **openssl.ssl** — highest fan-out, references EVP, BIO, X509, etc.

---

## Generator Design (`bnd-openssl-gen`)

Follows the bnd-posix-gen pattern: `generate(output_dir: &Path)` runs
bnd-winmd → windows-bindgen `--package --sys --filter openssl` → deletes
intermediate winmd. TOML generation is enabled — `windows-bindgen`
auto-appends feature definitions after the `# generated features` sentinel.

Unlike bnd-posix, the product crate needs a `build.rs` to emit
`cargo:rustc-link-lib=crypto` and `cargo:rustc-link-lib=ssl` —
`windows_link::link!` macros alone were insufficient for linking.

---

## E2E Tests

E2E tests live in `bnd-openssl/tests/`, one per partition:

### Crypto / Version

| Test | What it does |
|---|---|
| `openssl_version_major_is_3` | `crypto::OPENSSL_version_major()` returns 3 |
| `openssl_version_string_starts_with_openssl` | `crypto::OpenSSL_version(crypto::OPENSSL_VERSION)` returns non-null string starting with "OpenSSL" |
| `openssl_version_num_nonzero` | `crypto::OpenSSL_version_num()` returns non-zero |
| `openssl_version_constants` | `crypto::OPENSSL_VERSION == 0`, `crypto::OPENSSL_VERSION_STRING == 6` |
| `crypto_malloc_free` | `crypto::CRYPTO_malloc(64)` returns non-null, `crypto::CRYPTO_free()` succeeds |

### Rand

| Test | What it does |
|---|---|
| `rand_bytes_fills_buffer` | `rand::RAND_bytes(buf, 32)` returns 1 and buffer is non-zero |
| `rand_bytes_different_each_time` | Two `rand::RAND_bytes` calls produce different output |
| `rand_status_is_seeded` | `rand::RAND_status()` returns 1 (PRNG seeded) |
| `rand_priv_bytes_fills_buffer` | `rand::RAND_priv_bytes(buf, 32)` returns 1 and buffer is non-zero |

### Bn (BigNum)

| Test | What it does |
|---|---|
| `bn_new_free` | `bn::BN_new()` returns non-null, `bn::BN_free()` succeeds |
| `bn_set_word_get_word` | `bn::BN_set_word(bn, 42)` then `bn::BN_get_word(bn)` returns 42 |
| `bn_num_bits` | `bn::BN_set_word(bn, 255)` → `bn::BN_num_bits(bn)` returns 8 |
| `bn_hex_roundtrip` | `bn::BN_set_word(bn, 0xDEAD)` → `bn::BN_bn2hex(bn)` → verify hex string |
| `bn_is_zero` | `bn::BN_new()` → `bn::BN_is_zero(bn)` returns 1 (freshly created BIGNUM is zero) |

### EVP Digest

| Test | What it does |
|---|---|
| `evp_sha256_digest` | `evp::EVP_MD_CTX_new` → `evp::EVP_DigestInit_ex(sha256)` → `evp::EVP_DigestUpdate("hello")` → `evp::EVP_DigestFinal_ex` → assert known SHA-256 hash |
| `evp_sha256_returns_nonnull` | `evp::EVP_sha256()` returns non-null `*const EVP_MD` |
| `evp_md_ctx_new_free` | `evp::EVP_MD_CTX_new()` → non-null, `evp::EVP_MD_CTX_free()` succeeds |
| `evp_max_md_size_constant` | `evp::EVP_MAX_MD_SIZE == 64` |

### SHA (One-Shot)

| Test | What it does |
|---|---|
| `sha_digest_length_constants` | `sha::SHA_DIGEST_LENGTH == 20`, `sha::SHA256_DIGEST_LENGTH == 32` |
| `sha256_one_shot` | `sha::SHA256(data, len, out)` produces known hash for "hello" |
| `sha1_one_shot` | `sha::SHA1(data, len, out)` produces known hash for "hello" |

### BIO

| Test | What it does |
|---|---|
| `bio_mem_write_read` | `bio::BIO_new(bio::BIO_s_mem())` → `bio::BIO_write("hello")` → `bio::BIO_read` → assert "hello" |
| `bio_new_free` | `bio::BIO_new(bio::BIO_s_mem())` returns non-null, `bio::BIO_free()` returns 1 |
| `bio_ctrl_pending` | Write 5 bytes → `bio::BIO_ctrl_pending()` returns 5 |

### SSL

| Test | What it does |
|---|---|
| `tls_client_method_nonnull` | `ssl::TLS_client_method()` returns non-null |
| `ssl_ctx_new_free` | `ssl::SSL_CTX_new(ssl::TLS_client_method())` → non-null, `ssl::SSL_CTX_free()` succeeds |
| `ssl_error_constants` | `ssl::SSL_ERROR_NONE == 0`, `ssl::SSL_ERROR_SSL == 1` |
| `ssl_new_free` | `ssl::SSL_new(ctx)` returns non-null, `ssl::SSL_free()` succeeds |

---

## Roundtrip Tests — `bnd-winmd/tests/roundtrip_openssl.rs`

| Test | Assertion |
|---|---|
| `types_opaque_typedefs_present` | `openssl.types` namespace contains 14 key opaque types (`EVP_MD`, `SSL`, `BIO`, `BIGNUM`, `X509`, etc.) |
| `types_pem_password_cb_delegate` | `pem_password_cb` delegate typedef is present |
| `crypto_functions_present` | `OpenSSL_version`, `OPENSSL_version_major/minor/patch`, `CRYPTO_malloc`, `CRYPTO_free` |
| `crypto_version_constants` | `OPENSSL_VERSION`, `OPENSSL_VERSION_STRING` constants present |
| `crypto_pinvoke_library_is_crypto` | `OpenSSL_version` has ImplMap with `import_scope.name() == "crypto"` |
| `rand_functions_present` | `RAND_bytes`, `RAND_status`, `RAND_seed`, `RAND_priv_bytes` |
| `rand_pinvoke_library_is_crypto` | `RAND_bytes` has ImplMap with `import_scope.name() == "crypto"` |
| `bn_functions_present` | `BN_new`, `BN_free`, `BN_set_word`, `BN_get_word`, `BN_num_bits`, `BN_bn2hex` |
| `evp_functions_present` | `EVP_MD_CTX_new/free`, `EVP_DigestInit_ex/Update/Final_ex`, `EVP_sha256` |
| `evp_constants_present` | `EVP_MAX_MD_SIZE` constant present |
| `sha_functions_present` | `SHA256`, `SHA1` |
| `sha_constants_present` | `SHA_DIGEST_LENGTH`, `SHA256_DIGEST_LENGTH` |
| `bio_functions_present` | `BIO_new/free/read/write/s_mem/ctrl_pending` |
| `ssl_functions_present` | `SSL_CTX_new/free`, `SSL_new/free`, `TLS_client_method` |
| `ssl_pinvoke_library_is_ssl` | `SSL_CTX_new` has ImplMap with `import_scope.name() == "ssl"` — validates two-library partitioning |
| `ssl_error_constants` | `SSL_ERROR_NONE`, `SSL_ERROR_SSL` constants present |

---

## What This Validates

| Layer | Test |
|---|---|
| **Opaque typedef scale** | 120 incomplete-struct typedefs → all emitted as `isize` |
| **Multi-library partitioning** | `library = "crypto"` vs `"ssl"` produce correct ImplMap |
| **Cross-partition TypeRef fan-out** | SSL functions reference EVP, BIO, X509 types from other partitions |
| **Deprecated-but-still-visible APIs** | SHA context structs extracted despite deprecation attribute |
| **`const T *` returns** | `EVP_sha256()` → `*const EVP_MD` (becomes `*mut isize`) |
| **Callback typedefs** | `pem_password_cb`, `BIO_callback_fn_ex` emitted as delegates |
| **Real crypto operations** | SHA-256 digest verified against known test vector |
| **Alloc/free lifecycle** | EVP_MD_CTX, EVP_CIPHER_CTX, BN, BIO, SSL_CTX — create, use, destroy |
| **winmd format** | `windows-bindgen` accepts multi-partition, multi-library winmd |

---

## Dependencies

- **`libssl-dev`** package — provides `/usr/include/openssl/*.h` and
  `/usr/lib/x86_64-linux-gnu/libssl.so`, `libcrypto.so`
- CI: `apt-get install libssl-dev libclang-dev`

---

## Implementation Steps

1. ✅ Create `tests/fixtures/openssl/openssl.toml`
   Started with types + crypto + rand, expanded to all 8 active partitions
2. ✅ Add `bnd-openssl` and `bnd-openssl-gen` to workspace `Cargo.toml` members
3. ✅ Create `bnd-openssl-gen/` generator crate
   Cargo.toml, src/lib.rs, src/main.rs, tests/up_to_date.rs
4. ✅ Create `bnd-openssl/` product crate
   Cargo.toml (with `Foundation = []` and `# generated features` marker),
   build.rs (link directives), src/lib.rs
5. ✅ Run generator: `cargo run -p bnd-openssl-gen`
6. ✅ Add roundtrip tests in `bnd-winmd/tests/roundtrip_openssl.rs`
7. ✅ Iteratively expand partitions in `openssl.toml`
   Added bn, evp, sha, bio, ssl; skipped err (LHASH issue); fixed
   crypto traverse (struct_tm.h, struct_FILE.h) and ssl traverse (tls1.h)
8. ✅ Write E2E tests in `bnd-openssl/tests/` (per-partition files)
9. ⬜ Update `docs/WIP.md` — add OpenSSL to candidate/status table
10. ✅ Verify all tests pass: `cargo test` (full workspace)

### Iterative Approach (Actual)

```
Phase 1: Types + Crypto + Rand    ← proved multi-partition extraction
  - Fixed: crypto traverse needed bits/types/struct_tm.h, struct_FILE.h
  - Fixed: include_paths needed /usr/include/x86_64-linux-gnu
Phase 2: + Bn + Evp + Sha + Bio   ← bulk expansion, all worked first try
Phase 3: + Ssl                    ← fixed traverse (needed openssl/tls1.h)
Phase 4: Err attempted → skipped  ← LHASH macro generates unresolvable types
```

---

## Known Limitations

### err partition (LHASH macro)

`openssl/err.h` uses the `DEFINE_LHASH_OF_INTERNAL(ERR_STRING_DATA)` macro,
which expands to a struct containing an inline anonymous union field:

```c
struct lhash_st_ERR_STRING_DATA {
    union lh_ERR_STRING_DATA_dummy { ... } dummy;
    // ...
};
```

bnd-winmd extracts `lhash_st_ERR_STRING_DATA` with the union as a field
type, but the union itself is not emitted as a standalone type in the
winmd. windows-bindgen then panics with `"type not found:
openssl.err.lh_ERR_STRING_DATA_dummy"`. Adding `openssl/lhash.h` to the
traverse makes it worse (introduces `lh_OPENSSL_STRING_dummy`).

**Fix needed**: bnd-winmd should either:
- (a) Emit inline anonymous union/struct types as standalone types, or
- (b) Skip structs that contain unresolvable inline type references

The err partition is commented out in `openssl.toml` with an explanatory
note. All error-handling APIs (`ERR_get_error`, `ERR_clear_error`, etc.)
are unavailable until this is resolved.

---

## Comparison with Existing System Tests

| Aspect | zlib | bnd-posix | OpenSSL |
|---|---|---|---|
| Headers | 2 | ~40 across 15 partitions | 8 active from 133 |
| Libraries | 1 (`libz`) | 1 (`libc`) | 2 (`libssl`, `libcrypto`) |
| Partitions | 2 | 15 | 8 (+ 1 skipped) |
| Generated lines | ~500 | ~3000 | ~5248 |
| Opaque types | 1 (`internal_state`) | 1 (`DIR`) | ~130 |
| Non-opaque structs | 3 | ~40 | 3-6 (SHA contexts, BN_GENCB) |
| Constants | ~29 | ~600 | ~200 |
| Callbacks | 4 | ~5 | ~10 |
| Binding mode | `--flat --sys` | `--package --sys` | `--package --sys` |
| Key new pattern | typedef-to-typedef | sub-header traverse | multi-library + opaque scale |
