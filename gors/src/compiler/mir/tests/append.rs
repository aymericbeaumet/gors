use super::*;

fn call_parts_mut(file: &mut File, builtin: hir::Builtin) -> (&mut Vec<Operand>, &mut Vec<Place>) {
    file.functions
        .iter_mut()
        .flat_map(|function| &mut function.blocks)
        .find_map(|block| match &mut block.terminator.kind {
            TerminatorKind::Call {
                callee: hir::Callee::Builtin(actual),
                args,
                destinations,
                ..
            } if *actual == builtin => Some((args, destinations)),
            _ => None,
        })
        .expect("expected MIR builtin call")
}

#[test]
fn verifier_rejects_append_source_and_result_type_corruption() {
    let source = r#"
        package main
        func values(destination, source []int, wrong []int32) []int {
            return append(destination, source...)
        }
        func main() {}
    "#;

    let mut wrong_source = lower(source);
    let wrong = wrong_source
        .functions
        .iter()
        .find(|function| function.name == "values")
        .into_iter()
        .flat_map(|function| &function.locals)
        .find(|local| local.name.as_deref() == Some("wrong"))
        .expect("wrong-typed parameter")
        .id;
    let (arguments, _) = call_parts_mut(&mut wrong_source, hir::Builtin::SliceI64AppendSlice);
    *arguments.get_mut(1).expect("append source operand") = Operand::Read(Place { local: wrong });
    let error = wrong_source.verify().unwrap_err();
    assert!(
        error.message.contains("invalid MIR integer slice append"),
        "{error:?}"
    );

    let mut wrong_result = lower(source);
    let destination = call_parts_mut(&mut wrong_result, hir::Builtin::SliceI64AppendSlice)
        .1
        .first()
        .copied()
        .expect("append result");
    wrong_result
        .functions
        .iter_mut()
        .find(|function| function.name == "values")
        .expect("values function")
        .locals
        .get_mut(destination.local.0 as usize)
        .expect("append result local")
        .ty = Ty::Slice(Box::new(Ty::Int(IntTy::Int32)));
    let error = wrong_result.verify().unwrap_err();
    assert!(
        error.message.contains("invalid MIR integer slice append"),
        "{error:?}"
    );
}

#[test]
fn verifier_rejects_direct_interface_slice_tag_type_corruption() {
    let source = r#"
        package main
        type I interface{}
        type J interface{}
        func first(values []I, wrong J) I { return values[0] }
        func put(values []I, wrong J) []I { return append(values, wrong) }
        func main() {}
    "#;
    let mut file = lower(source);
    let wrong_ty = file
        .functions
        .iter()
        .find(|function| function.name == "first")
        .into_iter()
        .flat_map(|function| &function.locals)
        .find(|local| local.name.as_deref() == Some("wrong"))
        .expect("wrong interface parameter")
        .ty
        .clone();
    let destination = call_parts_mut(&mut file, hir::Builtin::AggregateSliceIndexTagged)
        .1
        .first()
        .copied()
        .expect("tagged index result");
    file.functions
        .iter_mut()
        .find(|function| function.name == "first")
        .expect("first function")
        .locals
        .get_mut(destination.local.0 as usize)
        .expect("tagged index result local")
        .ty = wrong_ty;

    let error = file.verify().unwrap_err();
    assert!(
        error
            .message
            .contains("invalid MIR aggregate container call"),
        "{error:?}"
    );

    let mut wrong_set = lower(source);
    let wrong = wrong_set
        .functions
        .iter()
        .find(|function| function.name == "put")
        .into_iter()
        .flat_map(|function| &function.locals)
        .find(|local| local.name.as_deref() == Some("wrong"))
        .expect("wrong interface parameter")
        .id;
    let (arguments, _) = call_parts_mut(&mut wrong_set, hir::Builtin::AggregateSliceSetTagged);
    *arguments.get_mut(2).expect("tagged set value") = Operand::Read(Place { local: wrong });
    let error = wrong_set.verify().unwrap_err();
    assert!(
        error
            .message
            .contains("invalid MIR aggregate container call"),
        "{error:?}"
    );
}

#[test]
fn verifier_rejects_aggregate_nil_for_scalar_slices() {
    let mut file = lower(
        r#"
            package main
            func zero() []any { var values []any; return values }
            func main() {}
        "#,
    );
    let destination = call_parts_mut(&mut file, hir::Builtin::AggregateSliceNil)
        .1
        .first()
        .copied()
        .expect("aggregate nil result");
    file.functions
        .iter_mut()
        .find(|function| function.name == "zero")
        .expect("zero function")
        .locals
        .get_mut(destination.local.0 as usize)
        .expect("aggregate nil result local")
        .ty = Ty::Slice(Box::new(Ty::Int(IntTy::Int)));

    let error = file.verify().unwrap_err();
    assert!(
        error
            .message
            .contains("invalid MIR aggregate slice nil operation"),
        "{error:?}"
    );
}
