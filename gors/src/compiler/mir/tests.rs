use super::*;
use crate::compiler::ids::LocalId;
use crate::compiler::types::{ConstValue, IntTy, Ty};

fn lower(source: &str) -> File {
    let hir = crate::compiler::lower_to_hir("verify.go", source).unwrap();
    lower_file(&hir).unwrap()
}

#[test]
fn verifier_rejects_mutated_ids_types_and_call_abis() {
    let source = r#"
        package main
        func identity(x int) int { return x }
        func main() {
            if true { println(identity(1)) }
        }
    "#;

    let mut bad_local_id = lower(source);
    bad_local_id.functions[0].locals[0].id = LocalId(99);
    let error = bad_local_id.verify().unwrap_err();
    assert!(error.message.contains("local IDs are not dense"));

    let mut bad_switch = lower(source);
    let main = bad_switch
        .functions
        .iter_mut()
        .find(|function| function.name == "main")
        .unwrap();
    let switch = main
        .blocks
        .iter_mut()
        .find_map(|block| match &mut block.terminator.kind {
            TerminatorKind::SwitchBool { condition, .. } => Some(condition),
            _ => None,
        })
        .unwrap();
    *switch = Operand::Constant(ConstValue::Int("1".into()), Ty::Int(IntTy::Int));
    let error = bad_switch.verify().unwrap_err();
    assert!(error.message.contains("switch condition type mismatch"));

    let mut bad_call = lower(source);
    let main = bad_call
        .functions
        .iter_mut()
        .find(|function| function.name == "main")
        .unwrap();
    let arguments = main
        .blocks
        .iter_mut()
        .find_map(|block| match &mut block.terminator.kind {
            TerminatorKind::Call {
                callee: hir::Callee::Function(_),
                args,
                ..
            } => Some(args),
            _ => None,
        })
        .unwrap();
    arguments.clear();
    let error = bad_call.verify().unwrap_err();
    assert!(error.message.contains("has 0 arguments but expects 1"));

    let mut bad_call_result = lower(source);
    let main = bad_call_result
        .functions
        .iter_mut()
        .find(|function| function.name == "main")
        .unwrap();
    let destination = main
        .blocks
        .iter()
        .find_map(|block| match &block.terminator.kind {
            TerminatorKind::Call {
                callee: hir::Callee::Function(_),
                destinations,
                ..
            } => destinations.first().copied(),
            _ => None,
        })
        .unwrap();
    main.locals[destination.local.0 as usize].ty = Ty::Bool;
    let error = bad_call_result.verify().unwrap_err();
    assert!(error.message.contains("call destination type mismatch"));
}

#[test]
fn verifier_rejects_uninitialized_reads() {
    let source = r#"
        package main
        func twice(x int) int {
            y := x
            return y + y
        }
        func main() { println(twice(2)) }
    "#;

    let mut uninitialized = lower(source);
    let function = uninitialized
        .functions
        .iter_mut()
        .find(|function| function.name == "twice")
        .unwrap();
    let y = function
        .locals
        .iter()
        .find(|local| local.name.as_deref() == Some("y"))
        .unwrap()
        .id;
    for block in &mut function.blocks {
        block
            .statements
            .retain(|statement| statement.destination.local != y);
    }
    let error = uninitialized.verify().unwrap_err();
    assert!(error.message.contains("before initialization"));
}

#[test]
fn verifier_rejects_invalid_byte_slice_runtime_calls() {
    let source = r#"
        package main
        func main() {
            destination := make([]byte, 2)
            source := make([]byte, 2)
            destination[0] = 'x'
            _ = copy(destination, source)
        }
    "#;

    for (builtin, expected) in [
        (
            hir::Builtin::SliceU8Make,
            "invalid MIR byte slice make argument types",
        ),
        (
            hir::Builtin::SliceU8Set,
            "invalid MIR byte slice set argument types",
        ),
        (
            hir::Builtin::SliceU8Copy,
            "invalid MIR byte slice copy arguments",
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
            .expect("expected byte-slice runtime call");
        arguments.clear();

        let error = file.verify().unwrap_err();
        assert!(error.message.contains(expected), "{error:?}");
    }
}

#[test]
fn verifier_rejects_invalid_float_interface_runtime_calls() {
    let source = r#"
        package main
        func main() {
            var left any = 1.5
            var right any = 2.5
            _ = left == right
            _, _ = left.(float64)
        }
    "#;

    for (builtin, expected) in [
        (hir::Builtin::InterfaceBoxF64, "interface boxing"),
        (hir::Builtin::InterfaceEqual, "interface equality"),
        (
            hir::Builtin::InterfaceUnboxF64,
            "interface float extraction",
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
            .expect("expected float-interface runtime call");
        arguments.clear();

        let error = file.verify().unwrap_err();
        assert!(error.message.contains(expected), "{error:?}");
    }
}

#[test]
fn verifier_rejects_a_mutated_return_type() {
    let mut file = lower("package main\nfunc answer() int { return 42 }\n");
    let function = &mut file.functions[0];
    let values = function
        .blocks
        .iter_mut()
        .find_map(|block| match &mut block.terminator.kind {
            TerminatorKind::Return(values) if !values.is_empty() => Some(values),
            _ => None,
        })
        .unwrap();
    values[0] = Operand::Constant(ConstValue::Bool(true), Ty::Bool);

    let error = file.verify().unwrap_err();
    assert!(error.message.contains("return value type mismatch"));
}

#[test]
fn string_concat_allocation_survives_hir_to_normalized_mir() {
    let source = r#"
        package main
        func main() {
            left := "a"
            right := "b"
            value := left + right
            println(value)
        }
    "#;
    let hir = crate::compiler::lower_to_hir("concat.go", source).unwrap();
    let main = hir
        .functions
        .iter()
        .find(|function| function.name == "main")
        .unwrap();
    let hir::StmtKind::Let { values, .. } = &main.body.stmts[2].kind else {
        panic!("expected string initializer")
    };
    let expression = &values[0];
    assert!(matches!(
        expression.kind,
        hir::ExprKind::Binary {
            op: hir::BinaryOp::Add,
            ..
        }
    ));
    assert_eq!(expression.ty, Ty::String);
    assert!(expression.effects.may_allocate);

    let mir = crate::compiler::lower_to_mir(&hir).unwrap();
    assert!(string_concat(mir.as_file()).effects.may_allocate);
    let normalized = normalize(mir).unwrap();
    let concat = string_concat(normalized.as_file());
    assert!(concat.effects.may_allocate);
    assert_eq!(concat.panic, PanicEdge::None);
}

#[test]
fn verifier_rejects_mutated_effects_panic_edges_and_provenance() {
    let source = r#"
        package main
        func unrelated() {}
        func calculate(left int, right int) int {
            return left / right + left % right + (left << right)
        }
    "#;
    let file = lower(source);
    for op in [hir::BinaryOp::Div, hir::BinaryOp::Rem, hir::BinaryOp::Shl] {
        let rvalue = binary_rvalue(&file, op);
        assert!(rvalue.effects.may_panic);
        assert_eq!(rvalue.panic, PanicEdge::Propagate);
        assert!(matches!(rvalue.provenance, Provenance::Source(_)));
    }

    let mut bad_effect = file.clone();
    binary_rvalue_mut(&mut bad_effect, hir::BinaryOp::Div)
        .effects
        .may_panic = false;
    assert!(
        bad_effect
            .verify()
            .unwrap_err()
            .message
            .contains("effect mismatch")
    );

    let mut bad_edge = file.clone();
    binary_rvalue_mut(&mut bad_edge, hir::BinaryOp::Rem).panic = PanicEdge::None;
    assert!(
        bad_edge
            .verify()
            .unwrap_err()
            .message
            .contains("panic edge mismatch")
    );

    let unrelated_source = file
        .functions
        .iter()
        .find(|function| function.name == "unrelated")
        .unwrap()
        .source;
    let mut bad_provenance = file.clone();
    binary_rvalue_mut(&mut bad_provenance, hir::BinaryOp::Shl).provenance =
        Provenance::Source(unrelated_source);
    assert!(
        bad_provenance
            .verify()
            .unwrap_err()
            .message
            .contains("source reference is owned by")
    );

    let mut bad_function_source = file;
    bad_function_source
        .functions
        .iter_mut()
        .find(|function| function.name == "calculate")
        .unwrap()
        .source = unrelated_source;
    assert!(
        bad_function_source
            .verify()
            .unwrap_err()
            .message
            .contains("function source reference is owned by")
    );
}

#[test]
fn verifier_rejects_corrupt_deferred_action_boundaries() {
    let source = r#"
        package main
        func guarded() (result string) {
            defer func() { result, _ = recover().(string) }()
            defer func() { panic("replacement") }()
            panic("original")
        }
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
        RvalueKind::Use(Operand::Constant(ConstValue::Bool(true), Ty::Bool));
    assert!(
        uncleared
            .verify()
            .unwrap_err()
            .message
            .contains("begin by clearing its registration flag")
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

fn string_concat(file: &File) -> &Rvalue {
    binary_rvalue(file, hir::BinaryOp::Add)
}

fn binary_rvalue(file: &File, expected: hir::BinaryOp) -> &Rvalue {
    file.functions
        .iter()
        .flat_map(|function| &function.blocks)
        .flat_map(|block| &block.statements)
        .map(|statement| &statement.value)
        .find(|rvalue| {
            matches!(
                rvalue.kind,
                RvalueKind::Binary { op, .. } if op == expected
            )
        })
        .unwrap()
}

fn binary_rvalue_mut(file: &mut File, expected: hir::BinaryOp) -> &mut Rvalue {
    file.functions
        .iter_mut()
        .flat_map(|function| &mut function.blocks)
        .flat_map(|block| &mut block.statements)
        .map(|statement| &mut statement.value)
        .find(|rvalue| {
            matches!(
                rvalue.kind,
                RvalueKind::Binary { op, .. } if op == expected
            )
        })
        .unwrap()
}
