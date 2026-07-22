use super::*;
use crate::compiler::rust_ir::{
    ControlFlowPlan, Operand, ReadOp, RvalueKind, SlotInitialization, StorageClass, TerminatorKind,
};

fn lower_source(source: &str) -> rust_ir::File {
    let ast = crate::parser::parse_file("lowering.go", source).unwrap();
    let hir = crate::compiler::lower_to_hir(ast.ast()).unwrap();
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

    assert_eq!(function.control_flow, ControlFlowPlan::PcDispatchU32);
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

fn rvalue_operands(kind: &RvalueKind) -> Vec<&Operand> {
    match kind {
        RvalueKind::Use(operand) | RvalueKind::Unary { operand, .. } => vec![operand],
        RvalueKind::Binary { left, right, .. } => vec![left, right],
    }
}

fn terminator_operands(kind: &TerminatorKind) -> Vec<&Operand> {
    match kind {
        TerminatorKind::SwitchBool { condition, .. } => vec![condition],
        TerminatorKind::Call { args, .. } | TerminatorKind::Return(args) => args.iter().collect(),
        TerminatorKind::Goto(_) | TerminatorKind::Unreachable => Vec::new(),
    }
}
