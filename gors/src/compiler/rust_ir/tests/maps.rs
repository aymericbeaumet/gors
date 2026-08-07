use super::*;

const SOURCE: &str = r#"
    package main
    func main() {
        words := map[int]string{1: "one", 2: "two"}
        _, _ = words[1]
        for key, value := range words {
            if value == "one" { delete(words, key) }
        }
        values := map[string]int{"a": 1}
        for key := range values { delete(values, key) }
    }
"#;

#[test]
fn map_lowering_selects_snapshot_and_live_lookup_operations() {
    let file = lower(SOURCE);
    let requirement = file.verify().expect("map Rust IR must verify");
    for operation in [
        RuntimeOp::GoMapStringI64RangeKeys,
        RuntimeOp::GoMapStringI64Contains,
        RuntimeOp::GoMapI64GoStringMake,
        RuntimeOp::GoMapI64GoStringGet,
        RuntimeOp::GoMapI64GoStringContains,
        RuntimeOp::GoMapI64GoStringSet,
        RuntimeOp::GoMapI64GoStringDelete,
        RuntimeOp::GoMapI64GoStringRangeKeys,
    ] {
        assert!(requirement.contains(operation), "missing {operation:?}");
    }
    assert!(!requirement.contains(RuntimeOp::GoMapStringI64KeyAt));
}

#[test]
fn verifier_rejects_an_int_string_map_call_retyped_as_string_int() {
    let mut file = lower(SOURCE);
    *runtime_call_target_mut(&mut file, RuntimeOp::GoMapI64GoStringGet) =
        RuntimeOp::GoMapStringI64Get;
    refresh_test_effects(&mut file);

    let error = file.verify().unwrap_err();
    assert!(
        error
            .message
            .contains("runtime call argument 0 type mismatch"),
        "{error:?}"
    );
}
