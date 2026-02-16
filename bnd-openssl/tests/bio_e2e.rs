//! End-to-end tests for OpenSSL BIO bindings against real libcrypto.

use bnd_openssl::openssl::bio;

#[test]
fn bio_new_free() {
    unsafe {
        let method = bio::BIO_s_mem();
        assert!(!method.is_null(), "BIO_s_mem should return non-null");
        let b = bio::BIO_new(method);
        assert!(!b.is_null(), "BIO_new should return non-null");
        let ret = bio::BIO_free(b);
        assert_eq!(ret, 1, "BIO_free should return 1 on success");
    }
}

#[test]
fn bio_mem_write_read() {
    unsafe {
        let b = bio::BIO_new(bio::BIO_s_mem());
        assert!(!b.is_null());

        let data = b"hello";
        let written = bio::BIO_write(b, data.as_ptr() as *const _, data.len() as i32);
        assert_eq!(written, 5, "BIO_write should write 5 bytes");

        let mut buf = [0u8; 32];
        let read = bio::BIO_read(b, buf.as_mut_ptr() as *mut _, buf.len() as i32);
        assert_eq!(read, 5, "BIO_read should read 5 bytes");
        assert_eq!(&buf[..5], b"hello", "BIO_read should return 'hello'");

        bio::BIO_free(b);
    }
}

#[test]
fn bio_ctrl_pending() {
    unsafe {
        let b = bio::BIO_new(bio::BIO_s_mem());
        assert!(!b.is_null());

        let data = b"12345";
        bio::BIO_write(b, data.as_ptr() as *const _, data.len() as i32);

        let pending = bio::BIO_ctrl_pending(b);
        assert_eq!(pending, 5, "BIO_ctrl_pending should return 5");

        bio::BIO_free(b);
    }
}
