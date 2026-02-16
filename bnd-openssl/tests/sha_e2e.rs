//! End-to-end tests for OpenSSL SHA bindings against real libcrypto.

use bnd_openssl::openssl::sha;

#[test]
fn sha_digest_length_constants() {
    assert_eq!(sha::SHA_DIGEST_LENGTH, 20);
    assert_eq!(sha::SHA256_DIGEST_LENGTH, 32);
    assert_eq!(sha::SHA512_DIGEST_LENGTH, 64);
}

#[test]
fn sha256_one_shot() {
    unsafe {
        let data = b"hello";
        let mut hash = [0u8; 32];
        let ret = sha::SHA256(data.as_ptr(), data.len() as u64, hash.as_mut_ptr());
        assert!(!ret.is_null(), "SHA256 should return non-null");

        // Known SHA-256 of "hello"
        let expected: [u8; 32] = [
            0x2c, 0xf2, 0x4d, 0xba, 0x5f, 0xb0, 0xa3, 0x0e, 0x26, 0xe8, 0x3b, 0x2a, 0xc5, 0xb9,
            0xe2, 0x9e, 0x1b, 0x16, 0x1e, 0x5c, 0x1f, 0xa7, 0x42, 0x5e, 0x73, 0x04, 0x33, 0x62,
            0x93, 0x8b, 0x98, 0x24,
        ];
        assert_eq!(hash, expected, "SHA-256 of 'hello' mismatch");
    }
}

#[test]
fn sha1_one_shot() {
    unsafe {
        let data = b"hello";
        let mut hash = [0u8; 20];
        let ret = sha::SHA1(data.as_ptr(), data.len() as u64, hash.as_mut_ptr());
        assert!(!ret.is_null(), "SHA1 should return non-null");

        // Known SHA-1 of "hello"
        let expected: [u8; 20] = [
            0xaa, 0xf4, 0xc6, 0x1d, 0xdc, 0xc5, 0xe8, 0xa2, 0xda, 0xbe, 0xde, 0x0f, 0x3b, 0x48,
            0x2c, 0xd9, 0xae, 0xa9, 0x43, 0x4d,
        ];
        assert_eq!(hash, expected, "SHA-1 of 'hello' mismatch");
    }
}
