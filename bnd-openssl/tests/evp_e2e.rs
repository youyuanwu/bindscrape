//! End-to-end tests for OpenSSL EVP bindings against real libcrypto.

use bnd_openssl::openssl::evp;

#[test]
fn evp_sha256_returns_nonnull() {
    unsafe {
        let md = evp::EVP_sha256();
        assert!(!md.is_null(), "EVP_sha256() should return non-null");
    }
}

#[test]
fn evp_md_ctx_new_free() {
    unsafe {
        let ctx = evp::EVP_MD_CTX_new();
        assert!(!ctx.is_null(), "EVP_MD_CTX_new should return non-null");
        evp::EVP_MD_CTX_free(ctx);
    }
}

#[test]
fn evp_max_md_size_constant() {
    assert_eq!(evp::EVP_MAX_MD_SIZE, 64);
}

#[test]
fn evp_sha256_digest() {
    unsafe {
        let ctx = evp::EVP_MD_CTX_new();
        assert!(!ctx.is_null());

        let md = evp::EVP_sha256();
        let ret = evp::EVP_DigestInit_ex(ctx, md, core::ptr::null());
        assert_eq!(ret, 1, "EVP_DigestInit_ex should return 1");

        let data = b"hello";
        let ret = evp::EVP_DigestUpdate(ctx, data.as_ptr() as *const _, data.len() as u64);
        assert_eq!(ret, 1, "EVP_DigestUpdate should return 1");

        let mut hash = [0u8; 32];
        let mut hash_len: u32 = 0;
        #[allow(clippy::unnecessary_mut_passed)]
        let ret = evp::EVP_DigestFinal_ex(ctx, hash.as_mut_ptr(), &mut hash_len);
        assert_eq!(ret, 1, "EVP_DigestFinal_ex should return 1");
        assert_eq!(hash_len, 32, "SHA-256 hash should be 32 bytes");

        // Known SHA-256 of "hello"
        let expected: [u8; 32] = [
            0x2c, 0xf2, 0x4d, 0xba, 0x5f, 0xb0, 0xa3, 0x0e, 0x26, 0xe8, 0x3b, 0x2a, 0xc5, 0xb9,
            0xe2, 0x9e, 0x1b, 0x16, 0x1e, 0x5c, 0x1f, 0xa7, 0x42, 0x5e, 0x73, 0x04, 0x33, 0x62,
            0x93, 0x8b, 0x98, 0x24,
        ];
        assert_eq!(hash, expected, "SHA-256 of 'hello' mismatch");

        evp::EVP_MD_CTX_free(ctx);
    }
}
