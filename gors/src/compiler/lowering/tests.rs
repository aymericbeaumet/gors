use super::*;
use crate::compiler::rust_ir::{
    CallTarget, ControlFlowPlan, Operand, PanicEdge, Provenance, ReadOp, RuntimeOp, RvalueKind,
    SlotInitialization, StorageClass, SyntheticOrigin, TerminatorKind,
};

fn lower_source(source: &str) -> rust_ir::File {
    let hir = crate::compiler::lower_to_hir("lowering.go", source).unwrap();
    let mir = crate::compiler::lower_to_mir(&hir).unwrap();
    super::lower(mir).unwrap()
}

fn named_function<'a>(file: &'a rust_ir::File, name: &str) -> &'a rust_ir::Function {
    file.functions
        .iter()
        .find(|function| function.name == name)
        .unwrap()
}

fn local_reads(function: &rust_ir::Function, name: &str) -> Vec<ReadOp> {
    let local = function
        .locals
        .iter()
        .find(|local| local.name.as_deref() == Some(name))
        .unwrap()
        .id;
    function
        .blocks
        .iter()
        .flat_map(|block| {
            block
                .statements
                .iter()
                .flat_map(|statement| rvalue_operands(&statement.value.kind))
                .chain(terminator_operands(&block.terminator.kind))
        })
        .filter_map(|operand| match operand {
            Operand::Read { place, op } if place.local == local => Some(*op),
            Operand::Read { .. } | Operand::Constant(_) | Operand::Unit => None,
        })
        .collect()
}

#[test]
fn mandatory_lowering_selects_explicit_storage_moves_and_control_flow() {
    let rust_ir = lower_source("package main\nfunc echo(value string) string { return value }\n");
    let function = &rust_ir.functions[0];

    assert_eq!(
        function.control_flow,
        ControlFlowPlan::StructuredLinear {
            order: vec![function.entry]
        }
    );
    assert!(
        function
            .locals
            .iter()
            .all(|local| local.storage == StorageClass::CheckedOptionSlot)
    );
    assert_eq!(
        function.locals[function.parameters[0].0 as usize].initialization,
        SlotInitialization::Parameter(0)
    );

    let reads = local_reads(function, "value");
    assert!(!reads.is_empty());
    assert!(reads.iter().all(|read| *read == ReadOp::ProvenLastUseMove));
}

#[test]
fn idiom_pass_keeps_branching_and_looping_cfgs_explicit() {
    let file = lower_source(
        "package main\nfunc choose(flag bool) int { if flag { return 1 }; return 2 }\n",
    );

    assert_eq!(
        named_function(&file, "choose").control_flow,
        ControlFlowPlan::PcDispatchU32
    );
}

#[test]
fn owned_reads_clone_when_any_successor_path_uses_the_value() {
    let file = lower_source(
        r#"
            package main
            func choose(flag bool, value string) string {
                saved := value
                if flag { print(value) }
                return saved
            }
        "#,
    );
    let function = named_function(&file, "choose");

    assert_eq!(
        local_reads(function, "value"),
        vec![ReadOp::ProvenInitializedClone, ReadOp::ProvenLastUseMove,]
    );
    assert_eq!(
        local_reads(function, "saved"),
        vec![ReadOp::ProvenLastUseMove]
    );
}

#[test]
fn independent_branch_last_uses_move_owned_values() {
    let file = lower_source(
        r#"
            package main
            func consume(flag bool, value string) {
                if flag {
                    print(value)
                    return
                }
                print(value)
            }
        "#,
    );

    assert_eq!(
        local_reads(named_function(&file, "consume"), "value"),
        vec![ReadOp::ProvenLastUseMove, ReadOp::ProvenLastUseMove]
    );
}

#[test]
fn loop_backedges_keep_owned_reads_live() {
    let file = lower_source(
        r#"
            package main
            func repeat(count int, value string) {
                for count > 0 {
                    print(value)
                    count = count - 1
                }
            }
        "#,
    );

    assert_eq!(
        local_reads(named_function(&file, "repeat"), "value"),
        vec![ReadOp::ProvenInitializedClone]
    );
}

#[test]
fn mandatory_lowering_preserves_explicit_synthetic_origins() {
    let file =
        lower_source("package main\nfunc named() (result int) { return }\nfunc implicit() {}\n");
    crate::compiler::rust_ir::verify(&file).unwrap();

    let named = named_function(&file, "named");
    assert!(
        named
            .blocks
            .iter()
            .flat_map(|block| &block.statements)
            .any(|statement| matches!(
                statement.provenance,
                Provenance::Synthetic(SyntheticOrigin::NamedResultInitialization)
            ) && matches!(
                statement.value.provenance,
                Provenance::Synthetic(SyntheticOrigin::NamedResultInitialization)
            ))
    );

    let implicit = named_function(&file, "implicit");
    assert!(implicit.blocks.iter().any(|block| matches!(
        block.terminator.provenance,
        Provenance::Synthetic(SyntheticOrigin::ImplicitReturn)
    )));
}

#[test]
fn panic_builtin_selects_typed_runtime_operations() {
    let file = lower_source(
        r#"
            package main
            func panicBool() { panic(true) }
            func panicInt() { panic(42) }
            func panicString() { panic("boom") }
        "#,
    );

    for (name, expected) in [
        ("panicBool", RuntimeOp::PanicBool),
        ("panicInt", RuntimeOp::PanicI64),
        ("panicString", RuntimeOp::PanicGoString),
    ] {
        let function = named_function(&file, name);
        let call = function
            .blocks
            .iter()
            .map(|block| &block.terminator)
            .find(|terminator| {
                matches!(
                    &terminator.kind,
                    TerminatorKind::Call {
                        target: CallTarget::Runtime(operation),
                        ..
                    } if *operation == expected
                )
            })
            .unwrap();
        assert!(call.effects.may_panic);
        assert_eq!(call.panic, PanicEdge::Propagate);
    }
}

fn rvalue_operands(kind: &RvalueKind) -> Vec<&Operand> {
    match kind {
        RvalueKind::Use(operand) | RvalueKind::Unary { operand, .. } => vec![operand],
        RvalueKind::Binary { left, right, .. } => vec![left, right],
        RvalueKind::ArrayIndexI64 { array, index } => vec![array, index],
        RvalueKind::ArraySetI64 {
            array,
            index,
            value,
        } => vec![array, index, value],
        RvalueKind::StructLiteralI64(fields) => fields.iter().collect(),
        RvalueKind::StructFieldI64 { structure, .. } => vec![structure],
        RvalueKind::RecoverCompareNil { .. } => Vec::new(),
    }
}

fn terminator_operands(kind: &TerminatorKind) -> Vec<&Operand> {
    match kind {
        TerminatorKind::SwitchBool { condition, .. } => vec![condition],
        TerminatorKind::Call { args, .. } | TerminatorKind::Return(args) => args.iter().collect(),
        TerminatorKind::Goto(_) | TerminatorKind::Unreachable => Vec::new(),
    }
}
