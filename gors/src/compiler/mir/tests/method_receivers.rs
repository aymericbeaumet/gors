use super::*;

fn promoted_hir() -> hir::File {
    crate::compiler::lower_to_hir(
        "method-receiver.go",
        r#"
            package main
            type Inner struct { value int }
            func (inner Inner) Read() int { return inner.value }
            type Outer struct { padding int; Inner }
            func main() { println(Outer{Inner: Inner{value: 3}}.Read()) }
        "#,
    )
    .unwrap()
}

fn receiver_plan(function: &mut hir::Function) -> &mut hir::MethodReceiverPlan {
    let statement = function.body.stmts.first_mut().expect("main statement");
    let hir::StmtKind::Expr(print) = &mut statement.kind else {
        panic!("expected expression statement");
    };
    let hir::ExprKind::Call { args, .. } = &mut print.kind else {
        panic!("expected print call");
    };
    let method = args.first_mut().expect("printed method call");
    let hir::ExprKind::Call { args, .. } = &mut method.kind else {
        panic!("expected method call");
    };
    let receiver = args.first_mut().expect("method receiver");
    let hir::ExprKind::MethodReceiver { plan, .. } = &mut receiver.kind else {
        panic!("expected explicit method receiver plan");
    };
    plan
}

fn corrupt_plan(mutate: impl FnOnce(&mut hir::MethodReceiverPlan)) -> Diagnostic {
    let mut file = promoted_hir();
    let function = file
        .functions
        .iter_mut()
        .find(|function| function.name == "main")
        .unwrap();
    mutate(receiver_plan(function));
    lower::lower_function(function).unwrap_err()
}

#[test]
fn receiver_plan_validates_owner_index_type_embedding_adjustment_and_result() {
    let valid = promoted_hir();
    let main = valid
        .functions
        .iter()
        .find(|function| function.name == "main")
        .unwrap();
    lower::lower_function(main).expect("valid promoted receiver plan");

    let error = corrupt_plan(|plan| plan.root_ty = Ty::Bool);
    assert!(error.message.contains("root type mismatch"), "{error:?}");

    let error = corrupt_plan(|plan| {
        plan.path.first_mut().expect("promoted path").owner_ty = Ty::Bool;
    });
    assert!(error.message.contains("field owner mismatch"), "{error:?}");

    let error = corrupt_plan(|plan| {
        plan.path.first_mut().expect("promoted path").field = u32::MAX;
    });
    assert!(
        error.message.contains("field index is out of bounds"),
        "{error:?}"
    );

    let error = corrupt_plan(|plan| {
        plan.path.first_mut().expect("promoted path").field = 0;
    });
    assert!(error.message.contains("non-embedded field"), "{error:?}");

    let error = corrupt_plan(|plan| {
        plan.path.first_mut().expect("promoted path").field_ty = Ty::Bool;
    });
    assert!(error.message.contains("field type mismatch"), "{error:?}");

    let error = corrupt_plan(|plan| plan.selected_ty = Ty::Bool);
    assert!(
        error.message.contains("selected type mismatch"),
        "{error:?}"
    );

    let error = corrupt_plan(|plan| {
        plan.adjustment = hir::MethodReceiverAdjustment::AutoIndirect;
    });
    assert!(
        error.message.contains("expected a pointer type"),
        "{error:?}"
    );

    let error = corrupt_plan(|plan| plan.receiver_ty = Ty::Bool);
    assert!(error.message.contains("result type mismatch"), "{error:?}");
}
