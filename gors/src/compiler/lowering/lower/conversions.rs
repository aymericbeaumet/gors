//! Exact selection of ABI-owned numeric conversion operations.

use crate::compiler::Diagnostic;
use crate::compiler::lowering::type_lowering::{
    complex_kind, float_kind, integer_kind, lower_type,
};
use crate::compiler::rust_ir as out;
use crate::compiler::types::Ty;
use gors_runtime_abi::PrimitiveOp;

pub(super) fn lower_conversion_operation(
    from: &Ty,
    to: &Ty,
) -> Result<Option<out::ValueOp>, Diagnostic> {
    let operation = match (from.underlying(), to.underlying()) {
        (Ty::Int(_) | Ty::Uint(_), Ty::Int(_) | Ty::Uint(_)) => Some(PrimitiveOp::IntegerConvert {
            from: integer_kind(from)
                .ok_or_else(|| Diagnostic::backend("missing source integer representation"))?,
            to: integer_kind(to)
                .ok_or_else(|| Diagnostic::backend("missing destination integer representation"))?,
        }),
        (Ty::Int(_) | Ty::Uint(_), Ty::Float(to)) => Some(PrimitiveOp::IntegerToFloat {
            from: integer_kind(from)
                .ok_or_else(|| Diagnostic::backend("missing source integer representation"))?,
            to: float_kind(*to),
        }),
        (Ty::Float(from), Ty::Int(_) | Ty::Uint(_)) => Some(PrimitiveOp::FloatToInteger {
            from: float_kind(*from),
            to: integer_kind(to)
                .ok_or_else(|| Diagnostic::backend("missing destination integer representation"))?,
        }),
        (Ty::Float(from), Ty::Float(to)) => {
            PrimitiveOp::float_conversion(float_kind(*from), float_kind(*to))
        }
        (Ty::Complex(from), Ty::Complex(to)) => {
            PrimitiveOp::complex_conversion(complex_kind(*from), complex_kind(*to))
        }
        _ => None,
    };
    let from_representation = lower_type(from)?;
    let to_representation = lower_type(to)?;
    if from_representation != to_representation && operation.is_none() {
        return Err(Diagnostic::backend(format!(
            "representation-preserving conversion changed Rust type from {from_representation:?} to {to_representation:?}"
        )));
    }
    Ok(operation.map(out::ValueOp::Primitive))
}
