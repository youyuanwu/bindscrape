# bnd-openssl

Rust FFI bindings for OpenSSL 3.x (`libssl` + `libcrypto`), auto-generated from OpenSSL headers via [`bnd-winmd`](../bnd-winmd/) and [`windows-bindgen`](https://crates.io/crates/windows-bindgen).

**Do not edit `src/openssl/` by hand** — run `cargo run -p bnd-openssl-gen` to regenerate.

## Modules

| Module | Library | APIs |
|---|---|---|
| `types` | — | ~130 opaque typedefs (`EVP_MD`, `SSL`, `BIO`, `BIGNUM`, …) |
| `crypto` | `libcrypto` | Version queries, `CRYPTO_malloc`/`CRYPTO_free` |
| `rand` | `libcrypto` | `RAND_bytes`, `RAND_status` |
| `bn` | `libcrypto` | `BN_new`, `BN_set_word`, `BN_bn2hex` |
| `evp` | `libcrypto` | `EVP_DigestInit_ex`, `EVP_sha256`, `EVP_MAX_MD_SIZE` |
| `sha` | `libcrypto` | `SHA1`, `SHA256`, digest length constants |
| `bio` | `libcrypto` | `BIO_new`, `BIO_read`, `BIO_write`, memory BIOs |
| `ssl` | `libssl` | `SSL_CTX_new`, `SSL_new`, `TLS_client_method`, `SSL_ERROR_*` |

Each module is a Cargo feature (all enabled by default).

## Prerequisites

- **`libssl-dev`** — `apt install libssl-dev`

## Example

```rust
use bnd_openssl::openssl::{evp, sha};

unsafe {
    let md = evp::EVP_sha256();
    assert!(!md.is_null());
    assert_eq!(sha::SHA256_DIGEST_LENGTH, 32);
}
```

All function bindings are `unsafe` — they call directly into the OpenSSL shared libraries.
