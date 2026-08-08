//! Exact folding for semantic Go constants.

use num_bigint::BigInt;
use num_traits::{ToPrimitive, Zero};

use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::provenance::SourceRef;
use crate::compiler::types::{ConstValue, ExactNumber, Ty, UintTy};

pub(super) fn fold_constant_unary(
    op: hir::UnaryOp,
    value: &ConstValue,
    ty: &Ty,
    source: SourceRef,
) -> Result<Option<ConstValue>, Diagnostic> {
    let folded = match (op, value) {
        (hir::UnaryOp::Positive, ConstValue::Int(_)) => value.clone(),
        (hir::UnaryOp::Negative, ConstValue::Int(value)) => {
            let value = BigInt::parse_bytes(value.as_bytes(), 10)
                .ok_or_else(|| Diagnostic::semantic("invalid exact integer constant", source))?;
            ConstValue::Int((-value).to_string())
        }
        (hir::UnaryOp::Not, ConstValue::Bool(value)) => ConstValue::Bool(!value),
        (hir::UnaryOp::Positive, ConstValue::Float(_) | ConstValue::Complex { .. }) => {
            value.clone()
        }
        (hir::UnaryOp::Negative, ConstValue::Float(value)) => ConstValue::Float(value.negated()),
        (hir::UnaryOp::Negative, ConstValue::Complex { real, imag }) => ConstValue::Complex {
            real: real.negated(),
            imag: imag.negated(),
        },
        (hir::UnaryOp::BitNot, ConstValue::Int(value)) => {
            let value = BigInt::parse_bytes(value.as_bytes(), 10)
                .ok_or_else(|| Diagnostic::semantic("invalid exact integer constant", source))?;
            let value = if let Ty::Uint(kind) = ty.underlying() {
                let width = match kind {
                    UintTy::Uint | UintTy::Uint64 | UintTy::Uintptr => 64,
                    UintTy::Uint8 => 8,
                    UintTy::Uint16 => 16,
                    UintTy::Uint32 => 32,
                };
                ((BigInt::from(1_u8) << width) - BigInt::from(1_u8)) ^ value
            } else {
                !value
            };
            ConstValue::Int(value.to_string())
        }
        _ => return Ok(None),
    };
    Ok(Some(folded))
}

pub(super) fn fold_constant_binary(
    op: hir::BinaryOp,
    left: &ConstValue,
    right: &ConstValue,
    source: SourceRef,
) -> Result<Option<ConstValue>, Diagnostic> {
    let folded = match (left, right) {
        (ConstValue::Int(left), ConstValue::Int(right)) => {
            fold_integer_binary(op, left, right, source)?
        }
        (ConstValue::Bool(left), ConstValue::Bool(right)) => match op {
            hir::BinaryOp::Equal => ConstValue::Bool(left == right),
            hir::BinaryOp::NotEqual => ConstValue::Bool(left != right),
            hir::BinaryOp::LogicalAnd => ConstValue::Bool(*left && *right),
            hir::BinaryOp::LogicalOr => ConstValue::Bool(*left || *right),
            _ => return Ok(None),
        },
        (ConstValue::String(left), ConstValue::String(right)) => match op {
            hir::BinaryOp::Add => {
                let mut result = left.clone();
                result.extend_from_slice(right);
                ConstValue::String(result)
            }
            hir::BinaryOp::Equal => ConstValue::Bool(left == right),
            hir::BinaryOp::NotEqual => ConstValue::Bool(left != right),
            hir::BinaryOp::Less => ConstValue::Bool(left < right),
            hir::BinaryOp::LessEqual => ConstValue::Bool(left <= right),
            hir::BinaryOp::Greater => ConstValue::Bool(left > right),
            hir::BinaryOp::GreaterEqual => ConstValue::Bool(left >= right),
            hir::BinaryOp::Min => ConstValue::String(left.min(right).clone()),
            hir::BinaryOp::Max => ConstValue::String(left.max(right).clone()),
            _ => return Ok(None),
        },
        (
            ConstValue::Complex { .. },
            ConstValue::Int(_) | ConstValue::Float(_) | ConstValue::Complex { .. },
        )
        | (ConstValue::Int(_) | ConstValue::Float(_), ConstValue::Complex { .. }) => {
            fold_exact_complex(op, left, right, source)?
        }
        (ConstValue::Float(_), ConstValue::Int(_) | ConstValue::Float(_))
        | (ConstValue::Int(_), ConstValue::Float(_)) => fold_exact_real(op, left, right, source)?,
        _ => return Ok(None),
    };
    Ok(Some(folded))
}

fn fold_integer_binary(
    op: hir::BinaryOp,
    left: &str,
    right: &str,
    source: SourceRef,
) -> Result<ConstValue, Diagnostic> {
    let left = BigInt::parse_bytes(left.as_bytes(), 10)
        .ok_or_else(|| Diagnostic::semantic("invalid exact integer constant", source))?;
    let right = BigInt::parse_bytes(right.as_bytes(), 10)
        .ok_or_else(|| Diagnostic::semantic("invalid exact integer constant", source))?;
    Ok(match op {
        hir::BinaryOp::Add => ConstValue::Int((left + right).to_string()),
        hir::BinaryOp::Sub => ConstValue::Int((left - right).to_string()),
        hir::BinaryOp::Mul => ConstValue::Int((left * right).to_string()),
        hir::BinaryOp::Div | hir::BinaryOp::Rem if right.is_zero() => {
            return Err(Diagnostic::semantic("division by zero", source));
        }
        hir::BinaryOp::Div => ConstValue::Int((left / right).to_string()),
        hir::BinaryOp::Rem => ConstValue::Int((left % right).to_string()),
        hir::BinaryOp::BitAnd => ConstValue::Int((left & right).to_string()),
        hir::BinaryOp::BitOr => ConstValue::Int((left | right).to_string()),
        hir::BinaryOp::BitXor => ConstValue::Int((left ^ right).to_string()),
        hir::BinaryOp::AndNot => ConstValue::Int((left & !right).to_string()),
        hir::BinaryOp::Shl | hir::BinaryOp::Shr => {
            let shift = right.to_usize().ok_or_else(|| {
                Diagnostic::semantic("shift count must be a non-negative integer", source)
            })?;
            if shift > 4096 {
                return Err(Diagnostic::unsupported(
                    "constant shifts larger than 4096 bits are not implemented",
                    source,
                ));
            }
            let value = if op == hir::BinaryOp::Shl {
                left << shift
            } else {
                left >> shift
            };
            ConstValue::Int(value.to_string())
        }
        hir::BinaryOp::Equal => ConstValue::Bool(left == right),
        hir::BinaryOp::NotEqual => ConstValue::Bool(left != right),
        hir::BinaryOp::Less => ConstValue::Bool(left < right),
        hir::BinaryOp::LessEqual => ConstValue::Bool(left <= right),
        hir::BinaryOp::Greater => ConstValue::Bool(left > right),
        hir::BinaryOp::GreaterEqual => ConstValue::Bool(left >= right),
        hir::BinaryOp::Min => ConstValue::Int(left.min(right).to_string()),
        hir::BinaryOp::Max => ConstValue::Int(left.max(right).to_string()),
        hir::BinaryOp::LogicalAnd | hir::BinaryOp::LogicalOr | hir::BinaryOp::Complex => {
            return Err(Diagnostic::backend(
                "invalid integer operator reached exact constant folding",
            ));
        }
    })
}

fn fold_exact_real(
    op: hir::BinaryOp,
    left: &ConstValue,
    right: &ConstValue,
    source: SourceRef,
) -> Result<ConstValue, Diagnostic> {
    let left = left
        .exact_number()
        .ok_or_else(|| Diagnostic::backend("missing exact left real constant"))?;
    let right = right
        .exact_number()
        .ok_or_else(|| Diagnostic::backend("missing exact right real constant"))?;
    Ok(match op {
        hir::BinaryOp::Add => ConstValue::Float(left.add(&right)),
        hir::BinaryOp::Sub => ConstValue::Float(left.sub(&right)),
        hir::BinaryOp::Mul => ConstValue::Float(left.mul(&right)),
        hir::BinaryOp::Div => ConstValue::Float(
            left.div(&right)
                .ok_or_else(|| Diagnostic::semantic("division by zero", source))?,
        ),
        hir::BinaryOp::Equal => ConstValue::Bool(left == right),
        hir::BinaryOp::NotEqual => ConstValue::Bool(left != right),
        hir::BinaryOp::Less => ConstValue::Bool(left < right),
        hir::BinaryOp::LessEqual => ConstValue::Bool(left <= right),
        hir::BinaryOp::Greater => ConstValue::Bool(left > right),
        hir::BinaryOp::GreaterEqual => ConstValue::Bool(left >= right),
        hir::BinaryOp::Min => ConstValue::Float(left.min(right)),
        hir::BinaryOp::Max => ConstValue::Float(left.max(right)),
        _ => {
            return Err(Diagnostic::backend(
                "invalid real operator reached exact constant folding",
            ));
        }
    })
}

fn fold_exact_complex(
    op: hir::BinaryOp,
    left: &ConstValue,
    right: &ConstValue,
    source: SourceRef,
) -> Result<ConstValue, Diagnostic> {
    let (left_real, left_imag) = exact_complex_parts(left)?;
    let (right_real, right_imag) = exact_complex_parts(right)?;
    let value = match op {
        hir::BinaryOp::Add => ConstValue::Complex {
            real: left_real.add(&right_real),
            imag: left_imag.add(&right_imag),
        },
        hir::BinaryOp::Sub => ConstValue::Complex {
            real: left_real.sub(&right_real),
            imag: left_imag.sub(&right_imag),
        },
        hir::BinaryOp::Mul => ConstValue::Complex {
            real: left_real.mul(&right_real).sub(&left_imag.mul(&right_imag)),
            imag: left_real.mul(&right_imag).add(&left_imag.mul(&right_real)),
        },
        hir::BinaryOp::Div => {
            let denominator = right_real
                .mul(&right_real)
                .add(&right_imag.mul(&right_imag));
            if denominator.is_zero() {
                return Err(Diagnostic::semantic("division by zero", source));
            }
            ConstValue::Complex {
                real: left_real
                    .mul(&right_real)
                    .add(&left_imag.mul(&right_imag))
                    .div(&denominator)
                    .ok_or_else(|| Diagnostic::backend("nonzero complex denominator vanished"))?,
                imag: left_imag
                    .mul(&right_real)
                    .sub(&left_real.mul(&right_imag))
                    .div(&denominator)
                    .ok_or_else(|| Diagnostic::backend("nonzero complex denominator vanished"))?,
            }
        }
        hir::BinaryOp::Equal => {
            ConstValue::Bool(left_real == right_real && left_imag == right_imag)
        }
        hir::BinaryOp::NotEqual => {
            ConstValue::Bool(left_real != right_real || left_imag != right_imag)
        }
        _ => {
            return Err(Diagnostic::backend(
                "invalid complex operator reached exact constant folding",
            ));
        }
    };
    Ok(value)
}

fn exact_complex_parts(value: &ConstValue) -> Result<(ExactNumber, ExactNumber), Diagnostic> {
    match value {
        ConstValue::Int(_) | ConstValue::Float(_) => Ok((
            value
                .exact_number()
                .ok_or_else(|| Diagnostic::backend("missing exact scalar constant"))?,
            ExactNumber::zero(),
        )),
        ConstValue::Complex { real, imag } => Ok((real.clone(), imag.clone())),
        ConstValue::Bool(_) | ConstValue::String(_) => Err(Diagnostic::backend(
            "nonnumeric value reached exact complex constant folding",
        )),
    }
}

/// Exact constant-shift folding for an untyped left operand.
pub(super) fn fold_untyped_constant_shift(
    op: hir::BinaryOp,
    left: &ConstValue,
    right: &ConstValue,
    source: SourceRef,
) -> Result<ConstValue, Diagnostic> {
    let left = constant_shift_integer_operand(left, "shifted operand", source)?;
    let right = constant_shift_integer_operand(right, "shift count", source)?;
    fold_constant_binary(op, &left, &right, source)?
        .ok_or_else(|| Diagnostic::backend("integer constant shift did not fold"))
}

pub(super) fn constant_shift_integer_operand(
    value: &ConstValue,
    role: &str,
    source: SourceRef,
) -> Result<ConstValue, Diagnostic> {
    value.exact_integer().ok_or_else(|| {
        Diagnostic::semantic(
            format!(
                "invalid operation: {role} {} must be an integer constant",
                describe_constant(value)
            ),
            source,
        )
    })
}

fn describe_constant(value: &ConstValue) -> String {
    match value {
        ConstValue::Bool(value) => value.to_string(),
        ConstValue::Int(spelling) => spelling.clone(),
        ConstValue::Float(value) => value.to_string(),
        ConstValue::Complex { real, imag } => format!("({real} + {imag}i)"),
        ConstValue::String(_) => "of string type".to_string(),
    }
}
