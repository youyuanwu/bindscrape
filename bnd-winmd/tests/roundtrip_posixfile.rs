//! Round-trip integration test: parse POSIX file I/O headers → emit winmd → read back and verify.

use std::path::Path;
use std::sync::LazyLock;

static POSIXFILE_WINMD: LazyLock<Vec<u8>> = LazyLock::new(|| {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../bnd-posix-gen/bnd-posix.toml");
    bnd_winmd::generate(&path).expect("generate posixfile winmd")
});

fn open_index() -> windows_metadata::reader::Index {
    let file = windows_metadata::reader::File::new(POSIXFILE_WINMD.clone())
        .expect("parse posixfile winmd");
    windows_metadata::reader::Index::new(vec![file])
}

// ---------------------------------------------------------------------------
// Fcntl partition (fcntl.h)
// ---------------------------------------------------------------------------

#[test]
fn fcntl_functions_present() {
    let index = open_index();

    let apis = index.expect("posix.fcntl", "Apis");
    let methods: Vec<String> = apis.methods().map(|m| m.name().to_string()).collect();

    // creat is non-variadic and should be present
    assert!(
        methods.contains(&"creat".to_string()),
        "creat missing. Methods: {methods:?}"
    );

    // open is variadic and should be skipped
    assert!(
        !methods.contains(&"open".to_string()),
        "open should be skipped (variadic). Methods: {methods:?}"
    );

    // fcntl is variadic and should be skipped
    assert!(
        !methods.contains(&"fcntl".to_string()),
        "fcntl should be skipped (variadic). Methods: {methods:?}"
    );
}

#[test]
fn fcntl_o_rdonly_constant() {
    let index = open_index();

    let apis = index.expect("posix.fcntl", "Apis");
    let fields: Vec<String> = apis.fields().map(|f| f.name().to_string()).collect();

    assert!(
        fields.contains(&"O_RDONLY".to_string()),
        "O_RDONLY missing. Fields: {fields:?}"
    );
    assert!(
        fields.contains(&"O_WRONLY".to_string()),
        "O_WRONLY missing. Fields: {fields:?}"
    );
    assert!(
        fields.contains(&"O_RDWR".to_string()),
        "O_RDWR missing. Fields: {fields:?}"
    );
    assert!(
        fields.contains(&"O_CREAT".to_string()),
        "O_CREAT missing. Fields: {fields:?}"
    );
    assert!(
        fields.contains(&"O_TRUNC".to_string()),
        "O_TRUNC missing. Fields: {fields:?}"
    );

    // Verify O_RDONLY = 0
    let o_rdonly = apis.fields().find(|f| f.name() == "O_RDONLY").unwrap();
    let val = o_rdonly
        .constant()
        .expect("O_RDONLY should have a constant");
    match val.value() {
        windows_metadata::Value::I32(v) => assert_eq!(v, 0, "O_RDONLY should be 0"),
        other => panic!("unexpected constant type for O_RDONLY: {other:?}"),
    }
}

#[test]
fn fcntl_pinvoke() {
    let index = open_index();

    let apis = index.expect("posix.fcntl", "Apis");
    let creat = apis
        .methods()
        .find(|m| m.name() == "creat")
        .expect("creat not found");

    let impl_map = creat.impl_map().expect("creat should have P/Invoke import");
    assert_eq!(
        impl_map.import_scope().name(),
        "c",
        "library name should be 'c'"
    );
}

// ---------------------------------------------------------------------------
// Unistd partition (unistd.h)
// ---------------------------------------------------------------------------

#[test]
fn unistd_functions_present() {
    let index = open_index();

    let apis = index.expect("posix.unistd", "Apis");
    let methods: Vec<String> = apis.methods().map(|m| m.name().to_string()).collect();

    let check = |name: &str| {
        assert!(
            methods.contains(&name.to_string()),
            "missing {name}. Methods: {methods:?}"
        );
    };

    check("read");
    check("write");
    check("close");
    check("lseek");
    check("unlink");
    check("access");
    check("getpid");
    check("dup");
    check("dup2");
    check("pipe");
    check("fsync");
}

#[test]
fn unistd_constants_present() {
    let index = open_index();

    let apis = index.expect("posix.unistd", "Apis");
    let fields: Vec<String> = apis.fields().map(|f| f.name().to_string()).collect();

    assert!(
        fields.contains(&"STDIN_FILENO".to_string()),
        "STDIN_FILENO missing. Fields: {fields:?}"
    );
    assert!(
        fields.contains(&"STDOUT_FILENO".to_string()),
        "STDOUT_FILENO missing. Fields: {fields:?}"
    );
    assert!(
        fields.contains(&"STDERR_FILENO".to_string()),
        "STDERR_FILENO missing. Fields: {fields:?}"
    );
    assert!(
        fields.contains(&"R_OK".to_string()),
        "R_OK missing. Fields: {fields:?}"
    );
    assert!(
        fields.contains(&"F_OK".to_string()),
        "F_OK missing. Fields: {fields:?}"
    );
}

// ---------------------------------------------------------------------------
// Stat partition (sys/stat.h + bits/struct_stat.h)
// ---------------------------------------------------------------------------

#[test]
fn stat_struct_present() {
    let index = open_index();

    let types: Vec<(String, String)> = index
        .all()
        .map(|td| (td.namespace().to_string(), td.name().to_string()))
        .collect();

    assert!(
        types
            .iter()
            .any(|(ns, n)| ns == "posix.stat" && n == "stat"),
        "struct stat missing. Found: {types:?}"
    );
}

#[test]
fn stat_struct_fields() {
    let index = open_index();

    let stat = index.expect("posix.stat", "stat");
    let fields: Vec<String> = stat.fields().map(|f| f.name().to_string()).collect();

    let check = |name: &str| {
        assert!(
            fields.contains(&name.to_string()),
            "missing field {name}. Fields: {fields:?}"
        );
    };

    check("st_dev");
    check("st_ino");
    check("st_nlink");
    check("st_mode");
    check("st_uid");
    check("st_gid");
    check("st_size");
    check("st_blksize");
    check("st_blocks");
}

#[test]
fn stat_struct_size() {
    let index = open_index();

    let stat = index.expect("posix.stat", "stat");
    let field_count = stat.fields().count();

    // struct stat on Linux x86-64 has 15 fields
    assert!(
        field_count >= 13,
        "struct stat should have 13+ fields, got {field_count}"
    );
}

#[test]
fn stat_functions_present() {
    let index = open_index();

    let apis = index.expect("posix.stat", "Apis");
    let methods: Vec<String> = apis.methods().map(|m| m.name().to_string()).collect();

    let check = |name: &str| {
        assert!(
            methods.contains(&name.to_string()),
            "missing {name}. Methods: {methods:?}"
        );
    };

    check("stat");
    check("fstat");
    check("lstat");
    check("chmod");
    check("mkdir");
    check("umask");
}
