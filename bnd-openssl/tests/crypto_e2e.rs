//! End-to-end tests for OpenSSL crypto bindings against real libcrypto.

use bnd_openssl::openssl::crypto;

#[test]
fn openssl_version_major_is_3() {
    unsafe {
        let major = crypto::OPENSSL_version_major();
        assert_eq!(major, 3, "Expected OpenSSL 3.x, got major version {major}");
    }
}

#[test]
fn openssl_version_string_starts_with_openssl() {
    unsafe {
        let ptr = crypto::OpenSSL_version(crypto::OPENSSL_VERSION);
        assert!(!ptr.is_null(), "OpenSSL_version returned null");

        let cstr = core::ffi::CStr::from_ptr(ptr);
        let s = cstr.to_str().expect("version string is not valid UTF-8");
        assert!(
            s.starts_with("OpenSSL"),
            "Expected version to start with 'OpenSSL', got: {s}"
        );
    }
}

#[test]
fn openssl_version_num_nonzero() {
    unsafe {
        let num = crypto::OpenSSL_version_num();
        assert!(num > 0, "OpenSSL_version_num should be nonzero");
    }
}

#[test]
fn openssl_version_constants() {
    assert_eq!(crypto::OPENSSL_VERSION, 0);
    assert_eq!(crypto::OPENSSL_VERSION_STRING, 6);
}

#[test]
fn crypto_malloc_free() {
    unsafe {
        let ptr = crypto::CRYPTO_malloc(64, c"test".as_ptr(), 0);
        assert!(!ptr.is_null(), "CRYPTO_malloc returned null");
        crypto::CRYPTO_free(ptr, c"test".as_ptr(), 0);
    }
}
