//! Round-trip integration test: parse simple.h → emit winmd → read back and verify contents.

use std::path::Path;
use std::sync::LazyLock;

static SIMPLE_WINMD: LazyLock<Vec<u8>> = LazyLock::new(|| {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/simple.toml");
    bindscrape::generate(&path).expect("generate simple winmd")
});

fn open_index() -> windows_metadata::reader::Index {
    let file = windows_metadata::reader::File::new(SIMPLE_WINMD.clone()).expect("parse winmd");
    windows_metadata::reader::Index::new(vec![file])
}

#[test]
fn roundtrip_typedefs_present() {
    assert!(!SIMPLE_WINMD.is_empty());
    let index = open_index();

    // Collect all type names
    let types: Vec<(String, String)> = index
        .all()
        .map(|td| (td.namespace().to_string(), td.name().to_string()))
        .collect();

    let has = |name: &str| types.iter().any(|(_, n)| n == name);

    assert!(has("Color"), "Color enum missing. Found: {types:?}");
    assert!(has("Rect"), "Rect struct missing. Found: {types:?}");
    assert!(has("Widget"), "Widget struct missing. Found: {types:?}");
    assert!(
        has("CompareFunc"),
        "CompareFunc delegate missing. Found: {types:?}"
    );
    assert!(has("Apis"), "Apis class missing. Found: {types:?}");
}

#[test]
fn roundtrip_enum_variants() {
    let index = open_index();

    let color = index.expect("SimpleTest", "Color");

    // Should extend System.Enum
    let extends = color.extends().expect("enum must extend something");
    let extends_str = format!("{extends:?}");
    assert!(
        extends_str.contains("Enum"),
        "Color should extend System.Enum, got: {extends_str}"
    );

    // Should have value__ + 3 variant fields = 4 total fields
    let fields: Vec<String> = color.fields().map(|f| f.name().to_string()).collect();
    assert!(
        fields.contains(&"value__".to_string()),
        "missing value__ field. Fields: {fields:?}"
    );
    assert!(
        fields.contains(&"COLOR_RED".to_string()),
        "missing COLOR_RED. Fields: {fields:?}"
    );
    assert!(
        fields.contains(&"COLOR_GREEN".to_string()),
        "missing COLOR_GREEN. Fields: {fields:?}"
    );
    assert!(
        fields.contains(&"COLOR_BLUE".to_string()),
        "missing COLOR_BLUE. Fields: {fields:?}"
    );
}

#[test]
fn roundtrip_struct_fields() {
    let index = open_index();

    let rect = index.expect("SimpleTest", "Rect");
    let fields: Vec<String> = rect.fields().map(|f| f.name().to_string()).collect();
    assert_eq!(
        fields.len(),
        4,
        "Rect should have 4 fields, got: {fields:?}"
    );
    assert!(fields.contains(&"x".to_string()));
    assert!(fields.contains(&"y".to_string()));
    assert!(fields.contains(&"width".to_string()));
    assert!(fields.contains(&"height".to_string()));
}

#[test]
fn roundtrip_functions() {
    let index = open_index();

    let apis = index.expect("SimpleTest", "Apis");
    let methods: Vec<String> = apis.methods().map(|m| m.name().to_string()).collect();

    assert!(
        methods.contains(&"create_widget".to_string()),
        "missing create_widget. Methods: {methods:?}"
    );
    assert!(
        methods.contains(&"destroy_widget".to_string()),
        "missing destroy_widget. Methods: {methods:?}"
    );
    assert!(
        methods.contains(&"widget_count".to_string()),
        "missing widget_count. Methods: {methods:?}"
    );
}

#[test]
fn roundtrip_function_params() {
    let index = open_index();

    let apis = index.expect("SimpleTest", "Apis");
    let create = apis
        .methods()
        .find(|m| m.name() == "create_widget")
        .expect("create_widget not found");

    let params: Vec<String> = create.params().map(|p| p.name().to_string()).collect();
    // Should have a return param + 3 params, or just 3 named params depending on emit
    assert!(
        params.len() >= 3,
        "create_widget should have at least 3 params, got: {params:?}"
    );
}

#[test]
fn roundtrip_constants() {
    let index = open_index();

    let apis = index.expect("SimpleTest", "Apis");
    let fields: Vec<String> = apis.fields().map(|f| f.name().to_string()).collect();

    assert!(
        fields.contains(&"MAX_WIDGETS".to_string()),
        "missing MAX_WIDGETS. Fields: {fields:?}"
    );
    assert!(
        fields.contains(&"DEFAULT_WIDTH".to_string()),
        "missing DEFAULT_WIDTH. Fields: {fields:?}"
    );
    assert!(
        fields.contains(&"DEFAULT_HEIGHT".to_string()),
        "missing DEFAULT_HEIGHT. Fields: {fields:?}"
    );

    // Check constant values
    let max_w = apis.fields().find(|f| f.name() == "MAX_WIDGETS").unwrap();
    let val = max_w
        .constant()
        .expect("MAX_WIDGETS should have a constant");
    match val.value() {
        windows_metadata::Value::I32(v) => assert_eq!(v, 256, "MAX_WIDGETS should be 256"),
        windows_metadata::Value::I64(v) => assert_eq!(v, 256, "MAX_WIDGETS should be 256"),
        other => panic!("unexpected constant type for MAX_WIDGETS: {other:?}"),
    }
}

#[test]
fn roundtrip_delegate() {
    let index = open_index();

    let cmp = index.expect("SimpleTest", "CompareFunc");

    // Should extend System.MulticastDelegate
    let extends = cmp.extends().expect("delegate must extend something");
    let extends_str = format!("{extends:?}");
    assert!(
        extends_str.contains("MulticastDelegate"),
        "CompareFunc should extend MulticastDelegate, got: {extends_str}"
    );

    // Should have an Invoke method
    let methods: Vec<String> = cmp.methods().map(|m| m.name().to_string()).collect();
    assert!(
        methods.contains(&"Invoke".to_string()),
        "delegate should have Invoke. Methods: {methods:?}"
    );
}

#[test]
fn roundtrip_pinvoke() {
    let index = open_index();

    let apis = index.expect("SimpleTest", "Apis");
    let create = apis
        .methods()
        .find(|m| m.name() == "create_widget")
        .expect("create_widget not found");

    let impl_map = create
        .impl_map()
        .expect("create_widget should have P/Invoke import");
    assert_eq!(
        impl_map.import_scope().name(),
        "simple",
        "DLL name should be 'simple'"
    );
}
