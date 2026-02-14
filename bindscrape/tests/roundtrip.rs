//! Round-trip integration test: parse simple.h → emit winmd → read back and verify contents.

use std::path::Path;
use std::sync::LazyLock;

/// Generate all winmd variants once. Combined into a single LazyLock because
/// the `clang` crate only allows one `Clang` instance at a time — concurrent
/// initialization from separate LazyLocks would race.
struct AllWinmd {
    simple: Vec<u8>,
    multi: Vec<u8>,
}

static ALL_WINMD: LazyLock<AllWinmd> = LazyLock::new(|| {
    let simple_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/simple.toml");
    let simple = bindscrape::generate(&simple_path).expect("generate simple winmd");

    let multi_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/multi/multi.toml");
    let multi = bindscrape::generate(&multi_path).expect("generate multi winmd");

    AllWinmd { simple, multi }
});

fn open_index() -> windows_metadata::reader::Index {
    let file = windows_metadata::reader::File::new(ALL_WINMD.simple.clone()).expect("parse winmd");
    windows_metadata::reader::Index::new(vec![file])
}

#[test]
fn roundtrip_typedefs_present() {
    assert!(!ALL_WINMD.simple.is_empty());
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

// ---------------------------------------------------------------------------
// Multi-partition round-trip tests
// ---------------------------------------------------------------------------

fn open_multi_index() -> windows_metadata::reader::Index {
    let file =
        windows_metadata::reader::File::new(ALL_WINMD.multi.clone()).expect("parse multi winmd");
    windows_metadata::reader::Index::new(vec![file])
}

#[test]
fn multi_types_in_correct_namespace() {
    assert!(!ALL_WINMD.multi.is_empty());
    let index = open_multi_index();

    // Types partition: Color, Rect, CompareFunc should be in MultiTest.Types
    let types: Vec<(String, String)> = index
        .all()
        .map(|td| (td.namespace().to_string(), td.name().to_string()))
        .collect();

    let has = |ns: &str, name: &str| types.iter().any(|(n, t)| n == ns && t == name);

    assert!(
        has("MultiTest.Types", "Color"),
        "Color should be in MultiTest.Types. Found: {types:?}"
    );
    assert!(
        has("MultiTest.Types", "Rect"),
        "Rect should be in MultiTest.Types. Found: {types:?}"
    );
    assert!(
        has("MultiTest.Types", "CompareFunc"),
        "CompareFunc should be in MultiTest.Types. Found: {types:?}"
    );
    assert!(
        has("MultiTest.Types", "Apis"),
        "Apis (constants) should be in MultiTest.Types. Found: {types:?}"
    );
}

#[test]
fn multi_widgets_in_correct_namespace() {
    let index = open_multi_index();

    let types: Vec<(String, String)> = index
        .all()
        .map(|td| (td.namespace().to_string(), td.name().to_string()))
        .collect();

    let has = |ns: &str, name: &str| types.iter().any(|(n, t)| n == ns && t == name);

    assert!(
        has("MultiTest.Widgets", "Widget"),
        "Widget should be in MultiTest.Widgets. Found: {types:?}"
    );
    assert!(
        has("MultiTest.Widgets", "Apis"),
        "Apis (functions) should be in MultiTest.Widgets. Found: {types:?}"
    );

    // Widget should NOT appear in MultiTest.Types
    assert!(
        !has("MultiTest.Types", "Widget"),
        "Widget should NOT be in MultiTest.Types. Found: {types:?}"
    );
}

#[test]
fn multi_traverse_filtering() {
    let index = open_multi_index();

    let types: Vec<(String, String)> = index
        .all()
        .map(|td| (td.namespace().to_string(), td.name().to_string()))
        .collect();

    let has = |ns: &str, name: &str| types.iter().any(|(n, t)| n == ns && t == name);

    // types.h types should NOT appear in MultiTest.Widgets namespace
    assert!(
        !has("MultiTest.Widgets", "Color"),
        "Color should NOT be in MultiTest.Widgets (traverse filtering)"
    );
    assert!(
        !has("MultiTest.Widgets", "Rect"),
        "Rect should NOT be in MultiTest.Widgets (traverse filtering)"
    );
    assert!(
        !has("MultiTest.Widgets", "CompareFunc"),
        "CompareFunc should NOT be in MultiTest.Widgets (traverse filtering)"
    );
}

#[test]
fn multi_cross_partition_typeref() {
    let index = open_multi_index();

    // Widget.color field should reference Color type.
    // The Widget struct is in MultiTest.Widgets.
    let widget = index.expect("MultiTest.Widgets", "Widget");
    let fields: Vec<String> = widget.fields().map(|f| f.name().to_string()).collect();
    assert!(
        fields.contains(&"color".to_string()),
        "Widget should have 'color' field. Fields: {fields:?}"
    );

    // create_widget function should exist in MultiTest.Widgets.Apis
    let apis = index.expect("MultiTest.Widgets", "Apis");
    let methods: Vec<String> = apis.methods().map(|m| m.name().to_string()).collect();
    assert!(
        methods.contains(&"create_widget".to_string()),
        "create_widget should be in MultiTest.Widgets.Apis. Methods: {methods:?}"
    );
}

#[test]
fn multi_constants_in_types_namespace() {
    let index = open_multi_index();

    let apis = index.expect("MultiTest.Types", "Apis");
    let fields: Vec<String> = apis.fields().map(|f| f.name().to_string()).collect();

    assert!(
        fields.contains(&"MAX_WIDGETS".to_string()),
        "MAX_WIDGETS should be in MultiTest.Types.Apis. Fields: {fields:?}"
    );
    assert!(
        fields.contains(&"DEFAULT_WIDTH".to_string()),
        "DEFAULT_WIDTH should be in MultiTest.Types.Apis. Fields: {fields:?}"
    );
    assert!(
        fields.contains(&"DEFAULT_HEIGHT".to_string()),
        "DEFAULT_HEIGHT should be in MultiTest.Types.Apis. Fields: {fields:?}"
    );
}
