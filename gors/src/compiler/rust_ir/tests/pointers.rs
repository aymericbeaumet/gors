use super::*;

#[test]
fn verifier_checks_integer_pointer_interface_runtime_signatures() {
    let source = r#"
        package main
        func main() {
            value := 1
            var boxed any = &value
            _, _ = boxed.(*int)
        }
    "#;
    let file = lower(source);
    let requirement = file.verify().expect("pointer interface calls must verify");
    for operation in [
        RuntimeOp::GoInterfaceBoxPointerI64,
        RuntimeOp::GoInterfaceUnboxPointerI64,
    ] {
        assert!(requirement.contains(operation), "missing {operation:?}");
    }

    for (expected, replacement) in [
        (
            RuntimeOp::GoInterfaceBoxPointerI64,
            RuntimeOp::GoInterfaceBoxPointerStructI64,
        ),
        (
            RuntimeOp::GoInterfaceUnboxPointerI64,
            RuntimeOp::GoInterfaceUnboxPointerStructI64,
        ),
    ] {
        let mut corrupt = file.clone();
        *runtime_call_target_mut(&mut corrupt, expected) = replacement;
        refresh_test_effects(&mut corrupt);
        let error = corrupt.verify().unwrap_err();
        assert!(
            error.message.contains("runtime call")
                || error.message.contains("call destination type mismatch"),
            "{expected:?} mutation produced {error:?}"
        );
    }
}

#[test]
fn verifier_checks_integer_pointer_equality_runtime_signature_and_requirement() {
    let source = r#"
        package main
        func equal(left, right *int) bool { return left == right }
        func notEqual(left, right *int) bool { return left != right }
    "#;
    let file = lower(source);
    let requirement = file.verify().expect("pointer equality calls must verify");
    assert!(requirement.contains(RuntimeOp::GoPointerI64Equal));

    let mut wrong_representation = file.clone();
    *runtime_call_target_mut(&mut wrong_representation, RuntimeOp::GoPointerI64Equal) =
        RuntimeOp::GoPointerStructI64Equal;
    refresh_test_effects(&mut wrong_representation);
    let error = wrong_representation.verify().unwrap_err();
    assert!(error.message.contains("runtime call"), "{error:?}");

    let mut wrong_arity = file;
    *runtime_call_target_mut(&mut wrong_arity, RuntimeOp::GoPointerI64Equal) =
        RuntimeOp::GoPointerI64IsNil;
    refresh_test_effects(&mut wrong_arity);
    let error = wrong_arity.verify().unwrap_err();
    assert!(error.message.contains("runtime call"), "{error:?}");
}
