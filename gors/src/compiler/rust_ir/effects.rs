//! Canonical effect derivation for the selected Rust representation.

use super::{
    CallTarget, Constant, Effects, Operand, PrimitiveOp, ReadOp, RuntimeOp, Rvalue, RvalueKind,
    TerminatorKind, ValueOp,
};
use gors_runtime_abi::{
    AllocationEffect, ArgumentMutationEffect, BlockingEffect, HostIoEffect, RuntimeType,
};

pub(in crate::compiler) fn statement_effects(value: &Rvalue) -> Effects {
    let mut effects = value.effects;
    effects.may_write = true;
    effects
}

pub(in crate::compiler) fn rvalue_effects(kind: &RvalueKind) -> Effects {
    let intrinsic = match kind {
        RvalueKind::Use(_)
        | RvalueKind::ArrayLiteral { .. }
        | RvalueKind::StructLiteral { .. }
        | RvalueKind::StructField { .. }
        | RvalueKind::StructLiteralI64(_)
        | RvalueKind::StructFieldI64 { .. }
        | RvalueKind::AggregateEqualI64 { .. } => Effects::default(),
        RvalueKind::StructSet { .. } | RvalueKind::StructSetI64 { .. } => Effects {
            may_write: true,
            ..Effects::default()
        },
        RvalueKind::Unary { op, .. } | RvalueKind::Binary { op, .. } => value_op_effects(*op),
        RvalueKind::RecoverCompareNil { .. } => Effects {
            may_read: true,
            may_write: true,
            ..Effects::default()
        },
        RvalueKind::ArrayIndexI64 { .. } | RvalueKind::ArrayIndex { .. } => Effects {
            may_call: true,
            may_panic: true,
            ..Effects::default()
        },
        RvalueKind::ArraySetI64 { .. } | RvalueKind::ArraySet { .. } => Effects {
            may_write: true,
            may_call: true,
            may_panic: true,
            ..Effects::default()
        },
    };
    let operands = match kind {
        RvalueKind::Use(operand) | RvalueKind::Unary { operand, .. } => operand_effects(operand),
        RvalueKind::Binary { left, right, .. } => {
            union(operand_effects(left), operand_effects(right))
        }
        RvalueKind::ArrayIndexI64 { array, index } | RvalueKind::ArrayIndex { array, index } => {
            union(operand_effects(array), operand_effects(index))
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
        } => union(
            union(operand_effects(array), operand_effects(index)),
            operand_effects(value),
        ),
        RvalueKind::ArrayLiteral { elements, .. } => elements
            .iter()
            .fold(Effects::default(), |effects, element| {
                union(effects, operand_effects(element))
            }),
        RvalueKind::StructLiteral { fields, .. } | RvalueKind::StructLiteralI64(fields) => {
            fields.iter().fold(Effects::default(), |effects, field| {
                union(effects, operand_effects(field))
            })
        }
        RvalueKind::StructField { structure, .. }
        | RvalueKind::StructFieldI64 { structure, .. } => operand_effects(structure),
        RvalueKind::StructSet {
            structure, value, ..
        }
        | RvalueKind::StructSetI64 {
            structure, value, ..
        } => union(operand_effects(structure), operand_effects(value)),
        RvalueKind::AggregateEqualI64 { left, right, .. } => {
            union(operand_effects(left), operand_effects(right))
        }
        RvalueKind::RecoverCompareNil { .. } => Effects::default(),
    };
    union(intrinsic, operands)
}

pub(in crate::compiler) fn terminator_effects(kind: &TerminatorKind) -> Effects {
    let intrinsic = match kind {
        TerminatorKind::Call { target, .. } => match target {
            CallTarget::Function(_) => user_call_effects(),
            CallTarget::Runtime(operation) => runtime_effects(*operation),
        },
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
    let mut effects = union(intrinsic, operands);
    if matches!(kind, TerminatorKind::Call { destinations, .. } if !destinations.is_empty()) {
        effects.may_write = true;
    }
    effects
}

#[cfg(test)]
pub(super) fn panic_edge(effects: Effects) -> super::PanicEdge {
    if effects.may_panic {
        super::PanicEdge::Propagate
    } else {
        super::PanicEdge::None
    }
}

fn value_op_effects(op: ValueOp) -> Effects {
    match op {
        ValueOp::Primitive(operation) => primitive_effects(operation),
        ValueOp::Runtime(operation) => runtime_effects(operation),
    }
}

fn primitive_effects(operation: PrimitiveOp) -> Effects {
    let signature = operation.signature();
    let may_call = signature
        .parameters()
        .iter()
        .copied()
        .chain(std::iter::once(signature.result()))
        .any(|ty| ty == RuntimeType::GoString);
    Effects {
        may_call,
        ..Effects::default()
    }
}

fn runtime_effects(operation: RuntimeOp) -> Effects {
    let effects = operation.effects();
    let host_io = effects.host_io() != HostIoEffect::None;
    Effects {
        may_call: true,
        may_allocate: effects.allocation() == AllocationEffect::MayAllocate,
        may_write: effects.argument_mutation() == ArgumentMutationEffect::MayMutateOwnedArgument
            || host_io,
        may_block: effects.blocking() == BlockingEffect::MayBlock,
        may_panic: !effects.go_panics().is_empty(),
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
            ..Effects::default()
        },
        Operand::Read {
            op: ReadOp::ProvenLastUseMove,
            ..
        } => Effects {
            may_read: true,
            may_write: true,
            ..Effects::default()
        },
        Operand::Constant(
            Constant::RuntimeStaticBytes { op, .. }
            | Constant::RuntimeStaticI64s { op, .. }
            | Constant::RuntimeStaticBools { op, .. }
            | Constant::RuntimeStaticU8s { op, .. },
        ) => runtime_effects(*op),
        Operand::Constant(
            Constant::Bool(_)
            | Constant::I64(_)
            | Constant::F64(_)
            | Constant::Complex128 { .. }
            | Constant::StaticI64Array(_),
        )
        | Operand::Unit => Effects::default(),
    }
}

fn user_call_effects() -> Effects {
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
