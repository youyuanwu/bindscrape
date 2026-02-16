//! End-to-end tests for OpenSSL RAND bindings against real libcrypto.

use bnd_openssl::openssl::rand;

#[test]
fn rand_bytes_fills_buffer() {
    unsafe {
        let mut buf = [0u8; 32];
        let ret = rand::RAND_bytes(buf.as_mut_ptr(), buf.len() as i32);
        assert_eq!(ret, 1, "RAND_bytes should return 1 on success");
        // Extremely unlikely that 32 random bytes are all zero
        assert!(
            buf.iter().any(|&b| b != 0),
            "RAND_bytes buffer should not be all zeros"
        );
    }
}

#[test]
fn rand_bytes_different_each_time() {
    unsafe {
        let mut buf1 = [0u8; 32];
        let mut buf2 = [0u8; 32];
        rand::RAND_bytes(buf1.as_mut_ptr(), buf1.len() as i32);
        rand::RAND_bytes(buf2.as_mut_ptr(), buf2.len() as i32);
        assert_ne!(
            buf1, buf2,
            "Two RAND_bytes calls should produce different output"
        );
    }
}

#[test]
fn rand_status_is_seeded() {
    unsafe {
        let status = rand::RAND_status();
        assert_eq!(status, 1, "RAND_status should return 1 (PRNG seeded)");
    }
}

#[test]
fn rand_priv_bytes_fills_buffer() {
    unsafe {
        let mut buf = [0u8; 16];
        let ret = rand::RAND_priv_bytes(buf.as_mut_ptr(), buf.len() as i32);
        assert_eq!(ret, 1, "RAND_priv_bytes should return 1 on success");
        assert!(
            buf.iter().any(|&b| b != 0),
            "RAND_priv_bytes buffer should not be all zeros"
        );
    }
}
