//! Round-trip integration test: parse OpenSSL headers → emit winmd → read back and verify.

use std::path::Path;
use std::sync::LazyLock;

static OPENSSL_WINMD: LazyLock<Vec<u8>> = LazyLock::new(|| {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/openssl/openssl.toml");
    bnd_winmd::generate(&path).expect("generate openssl winmd")
});

fn open_index() -> windows_metadata::reader::Index {
    let file =
        windows_metadata::reader::File::new(OPENSSL_WINMD.clone()).expect("parse openssl winmd");
    windows_metadata::reader::Index::new(vec![file])
}

// ---------------------------------------------------------------------------
// Types partition (types.h) — opaque typedefs
// ---------------------------------------------------------------------------

#[test]
fn types_opaque_typedefs_present() {
    let index = open_index();

    // Opaque types are emitted as TypeDefs in the openssl.types namespace.
    // index.expect("namespace", "TypeName") panics if the type is missing.
    let expected = [
        "EVP_MD",
        "EVP_MD_CTX",
        "EVP_CIPHER",
        "EVP_CIPHER_CTX",
        "EVP_PKEY",
        "EVP_PKEY_CTX",
        "SSL",
        "SSL_CTX",
        "BIO",
        "BIGNUM",
        "BN_CTX",
        "X509",
        "OSSL_LIB_CTX",
        "OSSL_PARAM",
    ];

    for name in expected {
        // This will panic with a clear message if the typedef is missing
        let _ = index.expect("openssl.types", name);
    }
}

#[test]
fn types_pem_password_cb_delegate() {
    let index = open_index();

    // pem_password_cb should be a delegate typedef in openssl.types
    let _ = index.expect("openssl.types", "pem_password_cb");
}

// ---------------------------------------------------------------------------
// Crypto partition (crypto.h) — version queries
// ---------------------------------------------------------------------------

#[test]
fn crypto_functions_present() {
    let index = open_index();

    let apis = index.expect("openssl.crypto", "Apis");
    let methods: Vec<String> = apis.methods().map(|m| m.name().to_string()).collect();

    let expected = [
        "OpenSSL_version",
        "OPENSSL_version_major",
        "OPENSSL_version_minor",
        "OPENSSL_version_patch",
        "CRYPTO_malloc",
        "CRYPTO_free",
    ];

    for name in expected {
        assert!(
            methods.contains(&name.to_string()),
            "{name} missing from openssl.crypto. Methods: {methods:?}"
        );
    }
}

#[test]
fn crypto_version_constants() {
    let index = open_index();

    let apis = index.expect("openssl.crypto", "Apis");
    let fields: Vec<String> = apis.fields().map(|f| f.name().to_string()).collect();

    assert!(
        fields.contains(&"OPENSSL_VERSION".to_string()),
        "OPENSSL_VERSION constant missing. Fields: {fields:?}"
    );
    assert!(
        fields.contains(&"OPENSSL_VERSION_STRING".to_string()),
        "OPENSSL_VERSION_STRING constant missing. Fields: {fields:?}"
    );
}

#[test]
fn crypto_pinvoke_library_is_crypto() {
    let index = open_index();

    let apis = index.expect("openssl.crypto", "Apis");
    let method = apis
        .methods()
        .find(|m| m.name() == "OpenSSL_version")
        .expect("OpenSSL_version method not found");

    let impl_map = method.impl_map().expect("OpenSSL_version has no ImplMap");
    let scope = impl_map.import_scope();
    let lib_name = scope.name();
    assert_eq!(
        lib_name, "crypto",
        "OpenSSL_version should link to libcrypto, got: {lib_name}"
    );
}

// ---------------------------------------------------------------------------
// Rand partition (rand.h) — random number generation
// ---------------------------------------------------------------------------

#[test]
fn rand_functions_present() {
    let index = open_index();

    let apis = index.expect("openssl.rand", "Apis");
    let methods: Vec<String> = apis.methods().map(|m| m.name().to_string()).collect();

    let expected = ["RAND_bytes", "RAND_status", "RAND_seed", "RAND_priv_bytes"];

    for name in expected {
        assert!(
            methods.contains(&name.to_string()),
            "{name} missing from openssl.rand. Methods: {methods:?}"
        );
    }
}

#[test]
fn rand_pinvoke_library_is_crypto() {
    let index = open_index();

    let apis = index.expect("openssl.rand", "Apis");
    let method = apis
        .methods()
        .find(|m| m.name() == "RAND_bytes")
        .expect("RAND_bytes method not found");

    let impl_map = method.impl_map().expect("RAND_bytes has no ImplMap");
    let scope = impl_map.import_scope();
    let lib_name = scope.name();
    assert_eq!(
        lib_name, "crypto",
        "RAND_bytes should link to libcrypto, got: {lib_name}"
    );
}

// ---------------------------------------------------------------------------
// Bn partition (bn.h) — big number arithmetic
// ---------------------------------------------------------------------------

#[test]
fn bn_functions_present() {
    let index = open_index();

    let apis = index.expect("openssl.bn", "Apis");
    let methods: Vec<String> = apis.methods().map(|m| m.name().to_string()).collect();

    let expected = [
        "BN_new",
        "BN_free",
        "BN_set_word",
        "BN_get_word",
        "BN_num_bits",
        "BN_bn2hex",
    ];

    for name in expected {
        assert!(
            methods.contains(&name.to_string()),
            "{name} missing from openssl.bn. Methods: {methods:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// EVP partition (evp.h) — high-level crypto
// ---------------------------------------------------------------------------

#[test]
fn evp_functions_present() {
    let index = open_index();

    let apis = index.expect("openssl.evp", "Apis");
    let methods: Vec<String> = apis.methods().map(|m| m.name().to_string()).collect();

    let expected = [
        "EVP_MD_CTX_new",
        "EVP_MD_CTX_free",
        "EVP_DigestInit_ex",
        "EVP_DigestUpdate",
        "EVP_DigestFinal_ex",
        "EVP_sha256",
    ];

    for name in expected {
        assert!(
            methods.contains(&name.to_string()),
            "{name} missing from openssl.evp. Methods: {methods:?}"
        );
    }
}

#[test]
fn evp_constants_present() {
    let index = open_index();

    let apis = index.expect("openssl.evp", "Apis");
    let fields: Vec<String> = apis.fields().map(|f| f.name().to_string()).collect();

    assert!(
        fields.contains(&"EVP_MAX_MD_SIZE".to_string()),
        "EVP_MAX_MD_SIZE missing. Fields: {fields:?}"
    );
}

// ---------------------------------------------------------------------------
// SHA partition (sha.h) — one-shot hash
// ---------------------------------------------------------------------------

#[test]
fn sha_functions_present() {
    let index = open_index();

    let apis = index.expect("openssl.sha", "Apis");
    let methods: Vec<String> = apis.methods().map(|m| m.name().to_string()).collect();

    assert!(
        methods.contains(&"SHA256".to_string()),
        "SHA256 missing from openssl.sha. Methods: {methods:?}"
    );
    assert!(
        methods.contains(&"SHA1".to_string()),
        "SHA1 missing from openssl.sha. Methods: {methods:?}"
    );
}

#[test]
fn sha_constants_present() {
    let index = open_index();

    let apis = index.expect("openssl.sha", "Apis");
    let fields: Vec<String> = apis.fields().map(|f| f.name().to_string()).collect();

    assert!(
        fields.contains(&"SHA_DIGEST_LENGTH".to_string()),
        "SHA_DIGEST_LENGTH missing. Fields: {fields:?}"
    );
    assert!(
        fields.contains(&"SHA256_DIGEST_LENGTH".to_string()),
        "SHA256_DIGEST_LENGTH missing. Fields: {fields:?}"
    );
}

// ---------------------------------------------------------------------------
// BIO partition (bio.h) — I/O abstraction
// ---------------------------------------------------------------------------

#[test]
fn bio_functions_present() {
    let index = open_index();

    let apis = index.expect("openssl.bio", "Apis");
    let methods: Vec<String> = apis.methods().map(|m| m.name().to_string()).collect();

    let expected = [
        "BIO_new",
        "BIO_free",
        "BIO_read",
        "BIO_write",
        "BIO_s_mem",
        "BIO_ctrl_pending",
    ];

    for name in expected {
        assert!(
            methods.contains(&name.to_string()),
            "{name} missing from openssl.bio. Methods: {methods:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// SSL partition (ssl.h) — TLS protocol
// ---------------------------------------------------------------------------

#[test]
fn ssl_functions_present() {
    let index = open_index();

    let apis = index.expect("openssl.ssl", "Apis");
    let methods: Vec<String> = apis.methods().map(|m| m.name().to_string()).collect();

    let expected = [
        "SSL_CTX_new",
        "SSL_CTX_free",
        "SSL_new",
        "SSL_free",
        "TLS_client_method",
    ];

    for name in expected {
        assert!(
            methods.contains(&name.to_string()),
            "{name} missing from openssl.ssl. Methods: {methods:?}"
        );
    }
}

#[test]
fn ssl_pinvoke_library_is_ssl() {
    let index = open_index();

    let apis = index.expect("openssl.ssl", "Apis");
    let method = apis
        .methods()
        .find(|m| m.name() == "SSL_CTX_new")
        .expect("SSL_CTX_new method not found");

    let impl_map = method.impl_map().expect("SSL_CTX_new has no ImplMap");
    let scope = impl_map.import_scope();
    let lib_name = scope.name();
    assert_eq!(
        lib_name, "ssl",
        "SSL_CTX_new should link to libssl, got: {lib_name}"
    );
}

#[test]
fn ssl_error_constants() {
    let index = open_index();

    let apis = index.expect("openssl.ssl", "Apis");
    let fields: Vec<String> = apis.fields().map(|f| f.name().to_string()).collect();

    assert!(
        fields.contains(&"SSL_ERROR_NONE".to_string()),
        "SSL_ERROR_NONE missing. Fields: {fields:?}"
    );
    assert!(
        fields.contains(&"SSL_ERROR_SSL".to_string()),
        "SSL_ERROR_SSL missing. Fields: {fields:?}"
    );
}
