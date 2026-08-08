use super::*;

const SOURCE: &str = r#"
    package main
    func main() {
        var values []string
        _ = values == nil
        values = make([]string, 1, 2)
        values[0] = "a"
        _ = len(values)
        _ = cap(values)
        _ = values[0]
        _ = values[:]
        values = append(values, "b")
        _ = copy(values, []string{"c"})
        clear(values)
        var boxed any = values
        _, _ = boxed.([]string)
    }
"#;

#[test]
fn string_slice_lowering_selects_and_verifies_the_complete_typed_abi() {
    let file = lower(SOURCE);
    let requirement = file.verify().expect("string-slice Rust IR must verify");
    for operation in [
        RuntimeOp::GoSliceGoStringNil,
        RuntimeOp::GoSliceGoStringIsNil,
        RuntimeOp::GoSliceGoStringMake,
        RuntimeOp::GoSliceGoStringSet,
        RuntimeOp::GoSliceGoStringLen,
        RuntimeOp::GoSliceGoStringCap,
        RuntimeOp::GoSliceGoStringIndex,
        RuntimeOp::GoSliceGoStringRange,
        RuntimeOp::GoSliceGoStringAppend,
        RuntimeOp::GoSliceGoStringCopy,
        RuntimeOp::GoSliceGoStringClear,
        RuntimeOp::GoInterfaceBoxGoSliceGoString,
        RuntimeOp::GoInterfaceUnboxGoSliceGoString,
    ] {
        assert!(requirement.contains(operation), "missing {operation:?}");
    }
}

#[test]
fn verifier_rejects_a_string_slice_call_retyped_as_an_integer_slice_call() {
    let mut file = lower(SOURCE);
    *runtime_call_target_mut(&mut file, RuntimeOp::GoSliceGoStringSet) = RuntimeOp::GoSliceI64Set;
    refresh_test_effects(&mut file);

    let error = file.verify().unwrap_err();
    assert!(
        error
            .message
            .contains("runtime call argument 0 type mismatch")
    );
}
