//! End-to-end tests for OpenSSL SSL/TLS bindings against real libssl.

use bnd_openssl::openssl::ssl;

#[test]
fn tls_client_method_nonnull() {
    unsafe {
        let method = ssl::TLS_client_method();
        assert!(
            !method.is_null(),
            "TLS_client_method should return non-null"
        );
    }
}

#[test]
fn ssl_ctx_new_free() {
    unsafe {
        let method = ssl::TLS_client_method();
        let ctx = ssl::SSL_CTX_new(method);
        assert!(!ctx.is_null(), "SSL_CTX_new should return non-null");
        ssl::SSL_CTX_free(ctx);
    }
}

#[test]
fn ssl_error_constants() {
    assert_eq!(ssl::SSL_ERROR_NONE, 0);
    assert_eq!(ssl::SSL_ERROR_SSL, 1);
}

#[test]
fn ssl_new_free() {
    unsafe {
        let method = ssl::TLS_client_method();
        let ctx = ssl::SSL_CTX_new(method);
        assert!(!ctx.is_null());

        let s = ssl::SSL_new(ctx);
        assert!(!s.is_null(), "SSL_new should return non-null");

        ssl::SSL_free(s);
        ssl::SSL_CTX_free(ctx);
    }
}
