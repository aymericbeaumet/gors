//! Construction helpers that attach exact effects and panic edges to MIR nodes.

use super::{
    LocalDecl, Operand, PanicEdge, Place, Provenance, Rvalue, RvalueKind, Statement, Terminator,
    TerminatorKind,
};
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::types::Ty;

pub(super) fn operand_ty(operand: &Operand, locals: &[LocalDecl]) -> Result<Ty, Diagnostic> {
    match operand {
        Operand::Read(place) => locals
            .get(place.local.0 as usize)
            .map(|local| local.ty.clone())
            .ok_or_else(|| Diagnostic::backend(format!("invalid operand local {}", place.local.0))),
        Operand::Constant(_, ty) => Ok(ty.clone()),
        Operand::Unit => Ok(Ty::Unit),
    }
}

pub(super) fn assignment_binary_op(op: hir::AssignOp) -> hir::BinaryOp {
    match op {
        hir::AssignOp::Set | hir::AssignOp::Add => hir::BinaryOp::Add,
        hir::AssignOp::Sub => hir::BinaryOp::Sub,
        hir::AssignOp::Mul => hir::BinaryOp::Mul,
        hir::AssignOp::Div => hir::BinaryOp::Div,
        hir::AssignOp::Rem => hir::BinaryOp::Rem,
        hir::AssignOp::BitAnd => hir::BinaryOp::BitAnd,
        hir::AssignOp::BitOr => hir::BinaryOp::BitOr,
        hir::AssignOp::BitXor => hir::BinaryOp::BitXor,
        hir::AssignOp::Shl => hir::BinaryOp::Shl,
        hir::AssignOp::Shr => hir::BinaryOp::Shr,
        hir::AssignOp::AndNot => hir::BinaryOp::AndNot,
    }
}

pub(super) fn make_statement(
    destination: Place,
    value: Rvalue,
    provenance: Provenance,
) -> Statement {
    let effects = value.effects.union(hir::Effects {
        may_write: true,
        ..hir::Effects::default()
    });
    Statement {
        destination,
        value,
        effects,
        provenance,
    }
}

pub(super) fn make_rvalue(
    kind: RvalueKind,
    intrinsic_effects: hir::Effects,
    provenance: Provenance,
) -> Rvalue {
    let may_read = match &kind {
        RvalueKind::Use(operand)
        | RvalueKind::Unary { operand, .. }
        | RvalueKind::Conversion { operand, .. } => operand_reads(operand),
        RvalueKind::Binary { left, right, .. } => operand_reads(left) || operand_reads(right),
        RvalueKind::ArrayIndexI64 { array, index } => operand_reads(array) || operand_reads(index),
        RvalueKind::ArrayIndex { array, index } => operand_reads(array) || operand_reads(index),
        RvalueKind::ArraySetI64 {
            array,
            index,
            value,
        } => operand_reads(array) || operand_reads(index) || operand_reads(value),
        RvalueKind::ArraySet {
            array,
            index,
            value,
        } => operand_reads(array) || operand_reads(index) || operand_reads(value),
        RvalueKind::ArrayLiteral { elements, .. } => elements.iter().any(operand_reads),
        RvalueKind::StructLiteral { fields, .. } => fields.iter().any(operand_reads),
        RvalueKind::StructField { structure, .. } => operand_reads(structure),
        RvalueKind::StructSet {
            structure, value, ..
        } => operand_reads(structure) || operand_reads(value),
        RvalueKind::Recover { .. } => true,
        RvalueKind::SliceLiteralI64 { .. }
        | RvalueKind::SliceLiteralU8(_)
        | RvalueKind::SliceLiteralBool(_)
        | RvalueKind::ArrayLiteralI64(_) => false,
    };
    let effects = intrinsic_effects.union(hir::Effects {
        may_read,
        ..hir::Effects::default()
    });
    Rvalue {
        kind,
        effects,
        panic: panic_edge(effects),
        provenance,
    }
}

pub(super) fn make_terminator(
    kind: TerminatorKind,
    intrinsic_effects: hir::Effects,
    provenance: Provenance,
) -> Terminator {
    let may_read = match &kind {
        TerminatorKind::SwitchBool { condition, .. } => operand_reads(condition),
        TerminatorKind::Call { args, .. } | TerminatorKind::Return(args) => {
            args.iter().any(operand_reads)
        }
        TerminatorKind::Goto(_)
        | TerminatorKind::SpawnEmpty { .. }
        | TerminatorKind::Unreachable => false,
    };
    let effects = intrinsic_effects.union(hir::Effects {
        may_read,
        ..hir::Effects::default()
    });
    Terminator {
        kind,
        effects,
        panic: panic_edge(effects),
        provenance,
    }
}

pub(super) fn binary_effects(op: hir::BinaryOp, result: &Ty, right: &Ty) -> hir::Effects {
    hir::Effects {
        may_allocate: op == hir::BinaryOp::Add && result == &Ty::String,
        may_panic: matches!(op, hir::BinaryOp::Div | hir::BinaryOp::Rem)
            || (matches!(op, hir::BinaryOp::Shl | hir::BinaryOp::Shr)
                && matches!(right.underlying(), Ty::Int(_))),
        ..hir::Effects::default()
    }
}

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

pub(super) fn spawn_empty_effects() -> hir::Effects {
    hir::Effects {
        may_call: true,
        may_allocate: true,
        ..hir::Effects::default()
    }
}

fn operand_reads(operand: &Operand) -> bool {
    matches!(operand, Operand::Read(_))
}

fn panic_edge(effects: hir::Effects) -> PanicEdge {
    if effects.may_panic {
        PanicEdge::Propagate
    } else {
        PanicEdge::None
    }
}
