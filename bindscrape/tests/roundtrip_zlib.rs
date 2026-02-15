//! Round-trip integration test: parse zlib.h (system header) → emit winmd → read back and verify.

use std::path::Path;
use std::sync::LazyLock;

static ZLIB_WINMD: LazyLock<Vec<u8>> = LazyLock::new(|| {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/zlib/zlib.toml");
    bindscrape::generate(&path).expect("generate zlib winmd")
});

fn open_index() -> windows_metadata::reader::Index {
    let file = windows_metadata::reader::File::new(ZLIB_WINMD.clone()).expect("parse zlib winmd");
    windows_metadata::reader::Index::new(vec![file])
}

#[test]
fn zlib_structs_present() {
    assert!(!ZLIB_WINMD.is_empty());
    let index = open_index();

    let types: Vec<(String, String)> = index
        .all()
        .map(|td| (td.namespace().to_string(), td.name().to_string()))
        .collect();

    let has = |name: &str| types.iter().any(|(ns, n)| ns == "Zlib" && n == name);

    assert!(
        has("z_stream_s"),
        "z_stream_s struct missing. Found: {types:?}"
    );
    assert!(
        has("gz_header_s"),
        "gz_header_s struct missing. Found: {types:?}"
    );
    assert!(has("Apis"), "Apis class missing. Found: {types:?}");
}

#[test]
fn zlib_delegates_present() {
    let index = open_index();

    let types: Vec<(String, String)> = index
        .all()
        .map(|td| (td.namespace().to_string(), td.name().to_string()))
        .collect();

    let has = |name: &str| types.iter().any(|(ns, n)| ns == "Zlib" && n == name);

    assert!(
        has("alloc_func"),
        "alloc_func delegate missing. Found: {types:?}"
    );
    assert!(
        has("free_func"),
        "free_func delegate missing. Found: {types:?}"
    );
    assert!(has("in_func"), "in_func delegate missing. Found: {types:?}");
    assert!(
        has("out_func"),
        "out_func delegate missing. Found: {types:?}"
    );
}

#[test]
fn zlib_functions_present() {
    let index = open_index();

    let apis = index.expect("Zlib", "Apis");
    let methods: Vec<String> = apis.methods().map(|m| m.name().to_string()).collect();

    let check = |name: &str| {
        assert!(
            methods.contains(&name.to_string()),
            "missing {name}. Methods: {methods:?}"
        );
    };

    // Simple utility functions
    check("zlibVersion");
    check("compress");
    check("compress2");
    check("compressBound");
    check("uncompress");

    // Deflate/inflate (real _ suffixed functions)
    check("deflateInit_");
    check("inflateInit_");
    check("deflate");
    check("deflateEnd");
    check("inflate");
    check("inflateEnd");

    // Checksums
    check("crc32");
    check("adler32");
}

#[test]
fn zlib_constants_present() {
    let index = open_index();

    let apis = index.expect("Zlib", "Apis");
    let fields: Vec<String> = apis.fields().map(|f| f.name().to_string()).collect();

    let check = |name: &str| {
        assert!(
            fields.contains(&name.to_string()),
            "missing {name}. Fields: {fields:?}"
        );
    };

    check("Z_OK");
    check("Z_STREAM_END");
    check("Z_NEED_DICT");
    check("Z_NO_FLUSH");
    check("Z_FINISH");
    check("Z_DEFLATED");
    check("Z_NULL");
    check("Z_NO_COMPRESSION");
    check("Z_BEST_SPEED");
    check("Z_BEST_COMPRESSION");
    check("Z_DEFAULT_STRATEGY");

    // Verify Z_OK = 0
    let z_ok = apis.fields().find(|f| f.name() == "Z_OK").unwrap();
    let val = z_ok.constant().expect("Z_OK should have a constant");
    match val.value() {
        windows_metadata::Value::I32(v) => assert_eq!(v, 0, "Z_OK should be 0"),
        windows_metadata::Value::I64(v) => assert_eq!(v, 0, "Z_OK should be 0"),
        other => panic!("unexpected constant type for Z_OK: {other:?}"),
    }

    // Verify Z_DEFLATED = 8
    let z_deflated = apis.fields().find(|f| f.name() == "Z_DEFLATED").unwrap();
    let val = z_deflated
        .constant()
        .expect("Z_DEFLATED should have a constant");
    match val.value() {
        windows_metadata::Value::I32(v) => assert_eq!(v, 8, "Z_DEFLATED should be 8"),
        windows_metadata::Value::I64(v) => assert_eq!(v, 8, "Z_DEFLATED should be 8"),
        other => panic!("unexpected constant type for Z_DEFLATED: {other:?}"),
    }
}

#[test]
fn zlib_z_stream_fields() {
    let index = open_index();

    let z_stream = index.expect("Zlib", "z_stream_s");
    let fields: Vec<String> = z_stream.fields().map(|f| f.name().to_string()).collect();

    assert_eq!(
        fields.len(),
        14,
        "z_stream_s should have 14 fields, got: {fields:?}"
    );

    let check = |name: &str| {
        assert!(
            fields.contains(&name.to_string()),
            "missing field {name}. Fields: {fields:?}"
        );
    };

    check("next_in");
    check("avail_in");
    check("total_in");
    check("next_out");
    check("avail_out");
    check("total_out");
    check("msg");
    check("state");
    check("zalloc");
    check("zfree");
    check("opaque");
    check("data_type");
    check("adler");
    check("reserved");
}

#[test]
fn zlib_pinvoke() {
    let index = open_index();

    let apis = index.expect("Zlib", "Apis");
    let compress = apis
        .methods()
        .find(|m| m.name() == "compress")
        .expect("compress not found");

    let impl_map = compress
        .impl_map()
        .expect("compress should have P/Invoke import");
    assert_eq!(
        impl_map.import_scope().name(),
        "z",
        "library name should be 'z'"
    );
}
