use super::*;

#[test]
fn pointer_interface_fingerprints_encode_dynamic_type_and_runtime_operations() {
    let (builtin_hir, builtin_mir, builtin_rust_ir) = lower_stages(
        "package main\nfunc roundtrip(value *int) *int { var boxed any = value; return boxed.(*int) }\nfunc main() {}\n",
    );
    let (named_hir, named_mir, named_rust_ir) = lower_stages(
        "package main\ntype Counter int\nfunc roundtrip(value *Counter) *Counter { var boxed any = value; return boxed.(*Counter) }\nfunc main() {}\n",
    );

    assert_ne!(
        hir_function(hir_named(&builtin_hir, "roundtrip")),
        hir_function(hir_named(&named_hir, "roundtrip"))
    );
    assert_ne!(
        mir_function(mir_named(&builtin_mir, "roundtrip")),
        mir_function(mir_named(&named_mir, "roundtrip"))
    );
    assert_ne!(
        rust_ir_function(rust_ir_named(&builtin_rust_ir, "roundtrip")),
        rust_ir_function(rust_ir_named(&named_rust_ir, "roundtrip"))
    );

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
        let mut changed = builtin_rust_ir.clone();
        *runtime_call_op_mut(&mut changed, "roundtrip", expected) = replacement;
        assert_ne!(
            rust_ir_function(rust_ir_named(&builtin_rust_ir, "roundtrip")),
            rust_ir_function(rust_ir_named(&changed, "roundtrip"))
        );
    }
}

#[test]
fn integer_pointer_equality_operation_tags_change_each_stage_fingerprint() {
    let (original_hir, original_mir, original_rust_ir) =
        lower_stages("package main\nfunc equal(left, right *int) bool { return left == right }\n");

    let mut changed_hir = original_hir.clone();
    let function = changed_hir
        .functions
        .iter_mut()
        .find(|function| function.name == "equal")
        .expect("equal HIR function");
    let hir::StmtKind::Return(values) = &mut function
        .body
        .stmts
        .first_mut()
        .expect("pointer equality return")
        .kind
    else {
        panic!("expected pointer equality return");
    };
    let hir::ExprKind::Call {
        callee: hir::Callee::Builtin(builtin),
        ..
    } = &mut values
        .first_mut()
        .expect("pointer equality return value")
        .kind
    else {
        panic!("expected pointer equality HIR call");
    };
    assert_eq!(*builtin, hir::Builtin::PointerI64Equal);
    *builtin = hir::Builtin::PointerStructI64Equal;
    assert_ne!(
        hir_function(hir_named(&original_hir, "equal")),
        hir_function(hir_named(&changed_hir, "equal"))
    );

    let mut changed_mir = original_mir.clone();
    let builtin = changed_mir
        .functions
        .iter_mut()
        .find(|function| function.name == "equal")
        .expect("equal MIR function")
        .blocks
        .iter_mut()
        .find_map(|block| match &mut block.terminator.kind {
            mir::TerminatorKind::Call {
                callee: hir::Callee::Builtin(builtin),
                ..
            } if *builtin == hir::Builtin::PointerI64Equal => Some(builtin),
            _ => None,
        })
        .expect("pointer equality MIR call");
    *builtin = hir::Builtin::PointerStructI64Equal;
    assert_ne!(
        mir_function(mir_named(&original_mir, "equal")),
        mir_function(mir_named(&changed_mir, "equal"))
    );

    let mut changed_rust_ir = original_rust_ir.clone();
    *runtime_call_op_mut(&mut changed_rust_ir, "equal", RuntimeOp::GoPointerI64Equal) =
        RuntimeOp::GoPointerStructI64Equal;
    assert_ne!(
        rust_ir_function(rust_ir_named(&original_rust_ir, "equal")),
        rust_ir_function(rust_ir_named(&changed_rust_ir, "equal"))
    );
}
