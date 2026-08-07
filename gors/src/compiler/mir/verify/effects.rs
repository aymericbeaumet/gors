//! MIR effect and panic-edge verification.

use crate::compiler::Diagnostic;
use crate::compiler::hir;

use super::super::{Operand, PanicEdge, RvalueKind, TerminatorKind};

pub(super) fn call_effects() -> hir::Effects {
    hir::Effects {
        may_call: true,
        may_allocate: true,
        may_block: true,
        may_panic: true,
        may_write: true,
        ..hir::Effects::default()
    }
}

pub(super) fn binary_effects(
    op: hir::BinaryOp,
    result: &crate::compiler::types::Ty,
    right: &crate::compiler::types::Ty,
) -> hir::Effects {
    hir::Effects {
        may_allocate: op == hir::BinaryOp::Add && result == &crate::compiler::types::Ty::String,
        may_panic: matches!(op, hir::BinaryOp::Div | hir::BinaryOp::Rem)
            || (matches!(op, hir::BinaryOp::Shl | hir::BinaryOp::Shr)
                && matches!(right.underlying(), crate::compiler::types::Ty::Int(_))),
        ..hir::Effects::default()
    }
}

pub(super) fn verify_effects(
    actual: hir::Effects,
    expected: hir::Effects,
    context: &str,
) -> Result<(), Diagnostic> {
    (actual == expected).then_some(()).ok_or_else(|| {
        Diagnostic::backend(format!(
            "MIR {context} effect mismatch: expected {expected:?}, found {actual:?}"
        ))
    })
}

pub(super) fn verify_panic_edge(
    effects: hir::Effects,
    edge: PanicEdge,
    context: &str,
) -> Result<(), Diagnostic> {
    let valid = if effects.may_panic {
        matches!(edge, PanicEdge::Propagate | PanicEdge::Cleanup(_))
    } else {
        edge == PanicEdge::None
    };
    valid.then_some(()).ok_or_else(|| {
        Diagnostic::backend(format!(
            "MIR {context} panic edge mismatch for effects {effects:?}: found {edge:?}"
        ))
    })
}

pub(super) fn read_effects<'a>(operands: impl IntoIterator<Item = &'a Operand>) -> hir::Effects {
    hir::Effects {
        may_read: operands
            .into_iter()
            .any(|operand| matches!(operand, Operand::Read(_))),
        ..hir::Effects::default()
    }
}

pub(super) fn rvalue_operands(kind: &RvalueKind) -> Vec<&Operand> {
    match kind {
        RvalueKind::Use(operand)
        | RvalueKind::Unary { operand, .. }
        | RvalueKind::Conversion { operand, .. } => vec![operand],
        RvalueKind::Binary { left, right, .. } => vec![left, right],
        RvalueKind::ArrayIndexI64 { array, index } | RvalueKind::ArrayIndex { array, index } => {
            vec![array, index]
        }
        RvalueKind::ArraySetI64 {
            array,
            index,
            value,
        }
        | RvalueKind::ArraySet {
            array,
            index,
            value,
        } => vec![array, index, value],
        RvalueKind::ArrayLiteral { elements, .. } => elements.iter().collect(),
        RvalueKind::StructLiteral { fields, .. } => fields.iter().collect(),
        RvalueKind::StructField { structure, .. } => vec![structure],
        RvalueKind::StructSet {
            structure, value, ..
        } => vec![structure, value],
        RvalueKind::Recover { value, .. } => vec![value],
        RvalueKind::SliceLiteralI64 { .. }
        | RvalueKind::SliceLiteralU8(_)
        | RvalueKind::SliceLiteralBool(_)
        | RvalueKind::ArrayLiteralI64(_) => Vec::new(),
    }
}

pub(super) fn terminator_operands(kind: &TerminatorKind) -> Vec<&Operand> {
    match kind {
        TerminatorKind::SwitchBool { condition, .. } => vec![condition],
        TerminatorKind::Call { args, .. } | TerminatorKind::Return(args) => args.iter().collect(),
        TerminatorKind::Goto(_)
        | TerminatorKind::SpawnEmpty { .. }
        | TerminatorKind::Unreachable => Vec::new(),
    }
}
