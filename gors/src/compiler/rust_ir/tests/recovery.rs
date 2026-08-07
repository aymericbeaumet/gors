use super::*;

#[test]
fn panic_recovery_operations_are_required_and_fully_verified() {
    let source = r#"
        package main
        func guarded() {
            defer func() { _ = recover() }()
            panic("boom")
        }
        func main() { guarded() }
    "#;
    let file = lower(source);
    let requirement = file.verify().unwrap();
    for operation in [
        RuntimeOp::GoInterfaceNil,
        RuntimeOp::PanicGoInterface,
        RuntimeOp::GoPanicPayloadToInterface,
    ] {
        assert!(requirement.contains(operation), "missing {operation:?}");
    }

    let mut bad_capture_effects = file.clone();
    bad_capture_effects
        .functions
        .iter_mut()
        .find(|function| function.name == "guarded")
        .unwrap()
        .panic_cleanup
        .as_mut()
        .unwrap()
        .capture
        .effects = Effects::default();
    assert!(
        bad_capture_effects
            .verify()
            .unwrap_err()
            .message
            .contains("panic payload capture effect mismatch")
    );

    let mut bad_rethrow_operation = file.clone();
    bad_rethrow_operation
        .functions
        .iter_mut()
        .find(|function| function.name == "guarded")
        .unwrap()
        .panic_cleanup
        .as_mut()
        .unwrap()
        .rethrow
        .operation = RuntimeOp::GoInterfaceNil;
    assert!(
        bad_rethrow_operation
            .verify()
            .unwrap_err()
            .message
            .contains("invalid payload rethrow operation")
    );

    let mut bad_rethrow_provenance = file.clone();
    bad_rethrow_provenance
        .functions
        .iter_mut()
        .find(|function| function.name == "guarded")
        .unwrap()
        .panic_cleanup
        .as_mut()
        .unwrap()
        .rethrow
        .provenance = Provenance::Synthetic(SyntheticOrigin::NamedResultInitialization);
    assert!(
        bad_rethrow_provenance
            .verify()
            .unwrap_err()
            .message
            .contains("invalid for panic payload rethrow")
    );

    let mut bad_recover_nil = file;
    let nil = bad_recover_nil
        .functions
        .iter_mut()
        .flat_map(|function| &mut function.blocks)
        .flat_map(|block| &mut block.statements)
        .find_map(|statement| match &mut statement.value.kind {
            RvalueKind::Recover { nil, .. } => Some(nil),
            _ => None,
        })
        .unwrap();
    *nil = RuntimeOp::GoInterfaceIsNil;
    assert!(
        bad_recover_nil
            .verify()
            .unwrap_err()
            .message
            .contains("invalid nil-interface operation")
    );
}
