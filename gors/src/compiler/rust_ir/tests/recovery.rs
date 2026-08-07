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

#[test]
fn deferred_action_replacement_plans_are_fully_verified() {
    let source = r#"
        package main
        func guarded() (result string) {
            defer func() { result, _ = recover().(string) }()
            defer func() { panic("replacement") }()
            panic("original")
        }
        func main() { println(guarded()) }
    "#;
    let file = lower(source);
    file.verify().unwrap();

    let mut uncleared = file.clone();
    let function = uncleared
        .functions
        .iter_mut()
        .find(|function| function.name == "guarded")
        .unwrap();
    let entry = function.panic_cleanup.as_ref().unwrap().actions[0].entry;
    function.blocks[entry.0 as usize].statements[0].value.kind =
        RvalueKind::Use(Operand::Constant(Constant::Bool(true)));
    assert!(
        uncleared
            .verify()
            .unwrap_err()
            .message
            .contains("begin by clearing its registration flag")
    );

    let mut bad_capture = file.clone();
    bad_capture
        .functions
        .iter_mut()
        .find(|function| function.name == "guarded")
        .unwrap()
        .panic_cleanup
        .as_mut()
        .unwrap()
        .actions[0]
        .replacement
        .capture
        .effects = Effects::default();
    assert!(
        bad_capture
            .verify()
            .unwrap_err()
            .message
            .contains("replacement capture effect mismatch")
    );

    let mut bad_replacement = file.clone();
    let function = bad_replacement
        .functions
        .iter_mut()
        .find(|function| function.name == "guarded")
        .unwrap();
    let action = &mut function.panic_cleanup.as_mut().unwrap().actions[0];
    action.replacement.target = action.entry;
    assert!(
        bad_replacement
            .verify()
            .unwrap_err()
            .message
            .contains("invalid continuation or state")
    );

    let mut bad_provenance = file.clone();
    bad_provenance
        .functions
        .iter_mut()
        .find(|function| function.name == "guarded")
        .unwrap()
        .panic_cleanup
        .as_mut()
        .unwrap()
        .actions[0]
        .replacement
        .provenance = Provenance::Synthetic(SyntheticOrigin::NamedResultInitialization);
    assert!(
        bad_provenance
            .verify()
            .unwrap_err()
            .message
            .contains("replacement has invalid provenance")
    );

    let mut bad_region = file.clone();
    let function = bad_region
        .functions
        .iter_mut()
        .find(|function| function.name == "guarded")
        .unwrap();
    let action = &mut function.panic_cleanup.as_mut().unwrap().actions[0];
    action.blocks.push(action.dispatch);
    assert!(
        bad_region
            .verify()
            .unwrap_err()
            .message
            .contains("block set is not canonical")
    );

    let mut bad_order = file;
    let function = bad_order
        .functions
        .iter_mut()
        .find(|function| function.name == "guarded")
        .unwrap();
    let cleanup = function.panic_cleanup.as_mut().unwrap();
    cleanup.actions[0].continuation = cleanup.completion;
    assert!(
        bad_order
            .verify()
            .unwrap_err()
            .message
            .contains("ordered cleanup chain")
    );
}
