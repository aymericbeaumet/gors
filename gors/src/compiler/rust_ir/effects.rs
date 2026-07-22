//! Canonical effect derivation for the selected Rust representation.

use super::{
    BinaryOp, Constant, Effects, Operand, PanicEdge, ReadOp, Rvalue, RvalueKind, TerminatorKind,
    UnaryOp,
};

pub(in crate::compiler) fn statement_effects(value: &Rvalue) -> Effects {
    let mut effects = value.effects;
    effects.may_write = true;
    effects
}

pub(in crate::compiler) fn rvalue_effects(kind: &RvalueKind) -> Effects {
    let intrinsic = match kind {
        RvalueKind::Use(_) => Effects::default(),
        RvalueKind::Unary { op, .. } => unary_effects(*op),
        RvalueKind::Binary { op, .. } => binary_effects(*op),
    };
    let operands = match kind {
        RvalueKind::Use(operand) | RvalueKind::Unary { operand, .. } => operand_effects(operand),
        RvalueKind::Binary { left, right, .. } => {
            union(operand_effects(left), operand_effects(right))
        }
    };
    union(intrinsic, operands)
}

pub(in crate::compiler) fn terminator_effects(kind: &TerminatorKind) -> Effects {
    let intrinsic = match kind {
        TerminatorKind::Call { .. } => call_effects(),
        TerminatorKind::Goto(_)
        | TerminatorKind::SwitchBool { .. }
        | TerminatorKind::Return(_)
        | TerminatorKind::Unreachable => Effects::default(),
    };
    let operands = match kind {
        TerminatorKind::SwitchBool { condition, .. } => operand_effects(condition),
        TerminatorKind::Call { args, .. } | TerminatorKind::Return(args) => {
            args.iter().fold(Effects::default(), |effects, operand| {
                union(effects, operand_effects(operand))
            })
        }
        TerminatorKind::Goto(_) | TerminatorKind::Unreachable => Effects::default(),
    };
    union(intrinsic, operands)
}

pub(in crate::compiler) fn panic_edge(effects: Effects) -> PanicEdge {
    if effects.may_panic {
        PanicEdge::Propagate
    } else {
        PanicEdge::None
    }
}

fn unary_effects(op: UnaryOp) -> Effects {
    Effects {
        may_call: matches!(op, UnaryOp::IntNeg),
        ..Effects::default()
    }
}

fn binary_effects(op: BinaryOp) -> Effects {
    let runtime_call = matches!(
        op,
        BinaryOp::IntAdd
            | BinaryOp::IntSub
            | BinaryOp::IntMul
            | BinaryOp::IntDiv
            | BinaryOp::IntRem
            | BinaryOp::IntShl
            | BinaryOp::IntShr
            | BinaryOp::StringConcat
    );
    Effects {
        may_call: runtime_call,
        may_allocate: matches!(op, BinaryOp::StringConcat),
        may_panic: matches!(
            op,
            BinaryOp::IntDiv | BinaryOp::IntRem | BinaryOp::IntShl | BinaryOp::IntShr
        ),
        ..Effects::default()
    }
}

fn operand_effects(operand: &Operand) -> Effects {
    match operand {
        Operand::Read {
            op: ReadOp::ProvenInitializedCopy,
            ..
        } => Effects {
            may_read: true,
            ..Effects::default()
        },
        Operand::Read {
            op: ReadOp::ProvenInitializedClone,
            ..
        } => Effects {
            may_read: true,
            may_call: true,
            may_allocate: true,
            ..Effects::default()
        },
        Operand::Constant(Constant::GoString(_)) => Effects {
            may_call: true,
            may_allocate: true,
            ..Effects::default()
        },
        Operand::Constant(Constant::Bool(_) | Constant::I64(_)) | Operand::Unit => {
            Effects::default()
        }
    }
}

fn call_effects() -> Effects {
    Effects {
        may_call: true,
        may_allocate: true,
        may_block: true,
        may_panic: true,
        may_write: true,
        ..Effects::default()
    }
}

fn union(left: Effects, right: Effects) -> Effects {
    Effects {
        may_read: left.may_read || right.may_read,
        may_write: left.may_write || right.may_write,
        may_call: left.may_call || right.may_call,
        may_allocate: left.may_allocate || right.may_allocate,
        may_block: left.may_block || right.may_block,
        may_panic: left.may_panic || right.may_panic,
    }
}
