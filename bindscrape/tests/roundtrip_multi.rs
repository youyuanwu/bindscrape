//! Round-trip integration test: parse multi-partition config → emit winmd → read back and verify.

use std::path::Path;
use std::sync::LazyLock;

static MULTI_WINMD: LazyLock<Vec<u8>> = LazyLock::new(|| {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/multi/multi.toml");
    bindscrape::generate(&path).expect("generate multi winmd")
});

fn open_multi_index() -> windows_metadata::reader::Index {
    let file = windows_metadata::reader::File::new(MULTI_WINMD.clone()).expect("parse multi winmd");
    windows_metadata::reader::Index::new(vec![file])
}

#[test]
fn multi_types_in_correct_namespace() {
    assert!(!MULTI_WINMD.is_empty());
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
