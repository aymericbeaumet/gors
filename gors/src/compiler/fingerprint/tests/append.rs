use super::*;

fn append_arguments_mut(file: &mut hir::File) -> (&mut hir::AppendArguments, hir::Expr) {
    let function = file
        .functions
        .iter_mut()
        .find(|function| function.name == "values")
        .expect("values HIR function");
    let hir::StmtKind::Return(values) =
        &mut function.body.stmts.first_mut().expect("values return").kind
    else {
        panic!("expected values return");
    };
    let hir::ExprKind::Append { slice, arguments } =
        &mut values.first_mut().expect("append return value").kind
    else {
        panic!("expected append expression");
    };
    (arguments, slice.as_ref().clone())
}

#[test]
fn append_argument_values_order_and_form_participate_in_hir_fingerprints() {
    let (original, _, _) = lower_stages(
        "package main\nfunc values() []int { return append([]int{}, 1, 2) }\nfunc main() {}\n",
    );
    let original_fingerprint = hir_function(hir_named(&original, "values"));

    let mut changed_value = original.clone();
    let (arguments, _) = append_arguments_mut(&mut changed_value);
    let hir::AppendArguments::Elements(elements) = arguments else {
        panic!("expected append elements");
    };
    let replacement = elements.get(1).expect("second append element").clone();
    *elements.first_mut().expect("first append element") = replacement;
    assert_ne!(
        original_fingerprint,
        hir_function(hir_named(&changed_value, "values"))
    );

    let mut changed_order = original.clone();
    let (arguments, _) = append_arguments_mut(&mut changed_order);
    let hir::AppendArguments::Elements(elements) = arguments else {
        panic!("expected append elements");
    };
    elements.reverse();
    assert_ne!(
        original_fingerprint,
        hir_function(hir_named(&changed_order, "values"))
    );

    let mut changed_form = original;
    let (arguments, slice) = append_arguments_mut(&mut changed_form);
    *arguments = hir::AppendArguments::Spread(Box::new(slice));
    assert_ne!(
        original_fingerprint,
        hir_function(hir_named(&changed_form, "values"))
    );
}
