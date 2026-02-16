//! End-to-end tests for OpenSSL BN (big number) bindings against real libcrypto.

use bnd_openssl::openssl::{bn, types};

#[test]
fn bn_new_free() {
    unsafe {
        let b = bn::BN_new();
        assert!(!b.is_null(), "BN_new should return non-null");
        bn::BN_free(b);
    }
}

#[test]
fn bn_set_word_get_word() {
    unsafe {
        let b = bn::BN_new();
        assert!(!b.is_null());
        let ret = bn::BN_set_word(b, 42);
        assert_eq!(ret, 1, "BN_set_word should return 1 on success");
        let val = bn::BN_get_word(b);
        assert_eq!(val, 42, "BN_get_word should return the value set");
        bn::BN_free(b);
    }
}

#[test]
fn bn_num_bits() {
    unsafe {
        let b = bn::BN_new();
        bn::BN_set_word(b, 255);
        let bits = bn::BN_num_bits(b);
        assert_eq!(bits, 8, "255 requires 8 bits");
        bn::BN_free(b);
    }
}

#[test]
fn bn_hex_roundtrip() {
    unsafe {
        let b = bn::BN_new();
        bn::BN_set_word(b, 0xDEAD);
        let hex_ptr = bn::BN_bn2hex(b);
        assert!(!hex_ptr.is_null(), "BN_bn2hex should return non-null");
        let hex_str = core::ffi::CStr::from_ptr(hex_ptr)
            .to_str()
            .expect("hex should be valid UTF-8");
        assert_eq!(hex_str, "DEAD", "Expected hex DEAD, got {hex_str}");

        // Free the hex string via OPENSSL_free (CRYPTO_free)
        use bnd_openssl::openssl::crypto;
        crypto::CRYPTO_free(hex_ptr as *const _, c"test".as_ptr(), 0);
        bn::BN_free(b);
    }
}

#[test]
fn bn_is_zero() {
    unsafe {
        let b = bn::BN_new();
        // Newly created BIGNUM is zero
        let zero = bn::BN_is_zero(b as *const types::BIGNUM);
        assert_eq!(zero, 1, "New BIGNUM should be zero");

        bn::BN_set_word(b, 1);
        let not_zero = bn::BN_is_zero(b as *const types::BIGNUM);
        assert_eq!(not_zero, 0, "BIGNUM set to 1 should not be zero");

        bn::BN_free(b);
    }
}
