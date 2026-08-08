use super::*;

fn array_length_hir() -> hir::File {
    crate::compiler::lower_to_hir(
        "array-length.go",
        r#"
            package main
            func values() [4]int { return [4]int{1, 2, 3, 4} }
            func main() { println(len(values())) }
        "#,
    )
    .unwrap()
}

fn array_length_expression(file: &mut hir::File) -> &mut hir::Expr {
    file.functions
        .iter_mut()
        .find(|function| function.name == "main")
        .into_iter()
        .flat_map(|function| &mut function.body.stmts)
        .find_map(|statement| match &mut statement.kind {
            hir::StmtKind::Expr(expression) => match &mut expression.kind {
                hir::ExprKind::Call { args, .. } => args
                    .iter_mut()
                    .find(|argument| matches!(argument.kind, hir::ExprKind::ArrayLen { .. })),
                _ => None,
            },
            _ => None,
        })
        .expect("runtime array length expression")
}

#[test]
fn mir_lowering_rejects_mismatched_hir_array_length_metadata() {
    let mut hir = array_length_hir();
    let expression = array_length_expression(&mut hir);
    let hir::ExprKind::ArrayLen { length, .. } = &mut expression.kind else {
        panic!("expected ArrayLen expression");
    };
    *length = 5;

    let error = lower_file(&hir).unwrap_err();
    assert!(
        error.iter().any(|diagnostic| diagnostic
            .message
            .contains("records length 5, but its operand has length 4")),
        "{error:?}"
    );
}

#[test]
fn mir_lowering_rejects_a_non_array_hir_array_length_operand() {
    let mut hir = array_length_hir();
    let expression = array_length_expression(&mut hir);
    let hir::ExprKind::ArrayLen { array, .. } = &mut expression.kind else {
        panic!("expected ArrayLen expression");
    };
    array.ty = Ty::Slice(Box::new(Ty::Int(IntTy::Int)));

    let error = lower_file(&hir).unwrap_err();
    assert!(
        error.iter().any(|diagnostic| diagnostic
            .message
            .contains("operand is not an array or pointer to array")),
        "{error:?}"
    );
}
