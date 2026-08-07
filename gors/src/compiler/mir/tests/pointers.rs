use super::*;

fn integer_pointer_equality_call(file: &mut File) -> (&mut Vec<Operand>, &mut Vec<Place>) {
    file.functions
        .iter_mut()
        .flat_map(|function| &mut function.blocks)
        .find_map(|block| match &mut block.terminator.kind {
            TerminatorKind::Call {
                callee: hir::Callee::Builtin(hir::Builtin::PointerI64Equal),
                args,
                destinations,
                ..
            } => Some((args, destinations)),
            _ => None,
        })
        .expect("expected integer pointer equality call")
}

#[test]
fn verifier_rejects_invalid_integer_pointer_interface_calls() {
    let source = r#"
        package main
        type Counter int
        func main() {
            value := 1
            var boxed any = &value
            _, _ = boxed.(*int)
            named := Counter(2)
            var namedBox any = &named
            _, _ = namedBox.(*Counter)
        }
    "#;

    for (builtin, expected) in [
        (hir::Builtin::InterfaceBoxPointerI64, "interface boxing"),
        (
            hir::Builtin::InterfaceUnboxPointerI64,
            "interface integer pointer extraction",
        ),
    ] {
        let mut file = lower(source);
        let arguments = file
            .functions
            .iter_mut()
            .flat_map(|function| &mut function.blocks)
            .find_map(|block| match &mut block.terminator.kind {
                TerminatorKind::Call {
                    callee: hir::Callee::Builtin(actual),
                    args,
                    ..
                } if *actual == builtin => Some(args),
                _ => None,
            })
            .expect("expected integer-pointer interface call");
        arguments.clear();

        let error = file.verify().unwrap_err();
        assert!(error.message.contains(expected), "{builtin:?}: {error:?}");
    }
}

#[test]
fn verifier_enforces_integer_pointer_equality_shape_and_exact_types() {
    let source = r#"
        package main
        type Counter int
        func equal(left, right *Counter, plain *int) bool { return left == right }
    "#;
    let file = lower(source);
    file.verify().expect("typed pointer equality must verify");

    let mut bad_arity = file.clone();
    integer_pointer_equality_call(&mut bad_arity).0.pop();
    let error = bad_arity.verify().unwrap_err();
    assert!(
        error.message.contains("integer pointer equality"),
        "{error:?}"
    );

    let mut bad_type = file.clone();
    integer_pointer_equality_call(&mut bad_type).0[1] =
        Operand::Constant(ConstValue::Bool(false), Ty::Bool);
    let error = bad_type.verify().unwrap_err();
    assert!(
        error.message.contains("integer pointer equality"),
        "{error:?}"
    );

    let mut bad_semantic_type = file.clone();
    let plain = bad_semantic_type
        .functions
        .iter()
        .flat_map(|function| &function.locals)
        .find(|local| local.name.as_deref() == Some("plain"))
        .expect("plain integer pointer parameter")
        .id;
    integer_pointer_equality_call(&mut bad_semantic_type).0[1] =
        Operand::Read(Place { local: plain });
    let error = bad_semantic_type.verify().unwrap_err();
    assert!(
        error
            .message
            .contains("integer pointer equality type mismatch"),
        "{error:?}"
    );

    let mut bad_destination = file;
    integer_pointer_equality_call(&mut bad_destination)
        .1
        .clear();
    let error = bad_destination.verify().unwrap_err();
    assert!(
        error.message.contains("0 destinations for 1 result values"),
        "{error:?}"
    );
}
