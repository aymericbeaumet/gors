use super::*;
use crate::compiler::rust_ir::{
    ControlFlowPlan, Operand, ReadOp, RvalueKind, SlotInitialization, StorageClass, TerminatorKind,
};

#[test]
fn mandatory_lowering_selects_explicit_storage_reads_and_control_flow() {
    let ast = crate::parser::parse_file(
        "lowering.go",
        "package main\nfunc echo(value string) string { return value }\n",
    )
    .unwrap();
    let hir = crate::compiler::lower_to_hir(&ast).unwrap();
    let mir = crate::compiler::lower_to_mir(&hir).unwrap();
    let rust_ir = lower(mir).unwrap();
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

    let reads = function
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
            Operand::Read { op, .. } => Some(*op),
            Operand::Constant(_) | Operand::Unit => None,
        })
        .collect::<Vec<_>>();
    assert!(!reads.is_empty());
    assert!(
        reads
            .iter()
            .all(|read| *read == ReadOp::ProvenInitializedClone)
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
