//! Mechanical emission of verified ABI-owned primitive operations.

use crate::compiler::Diagnostic;
use crate::compiler::rust_ir::{FloatKind, FloatPrimitive, IntegerKind, PrimitiveOp};

use super::integer::{emit_integer_canonicalization, emit_integer_primitive};

pub(super) fn emit_primitive_op(
    operation: PrimitiveOp,
    args: &[syn::Expr],
) -> Result<syn::Expr, Diagnostic> {
    use PrimitiveOp::*;
    let expression = match (operation, args) {
        (BoolNot, [value]) => syn::parse_quote! { !(#value) },
        (Integer { op, kind }, arguments) => emit_integer_primitive(op, kind, arguments)?,
        (IntegerConvert { to, .. }, [value]) => emit_integer_canonicalization(to, value.clone()),
        (IntegerToFloat { from, to }, [value]) => emit_integer_to_float(from, to, value),
        (FloatToInteger { from, to }, [value]) => emit_float_to_integer(from, to, value),
        (Float32 { op }, arguments) => emit_float_primitive(op, FloatKind::F32, arguments)?,
        (FloatNeg, arguments) => {
            emit_float_primitive(FloatPrimitive::Neg, FloatKind::F64, arguments)?
        }
        (FloatAdd, arguments) => {
            emit_float_primitive(FloatPrimitive::Add, FloatKind::F64, arguments)?
        }
        (FloatSub, arguments) => {
            emit_float_primitive(FloatPrimitive::Sub, FloatKind::F64, arguments)?
        }
        (FloatMul, arguments) => {
            emit_float_primitive(FloatPrimitive::Mul, FloatKind::F64, arguments)?
        }
        (FloatDiv, arguments) => {
            emit_float_primitive(FloatPrimitive::Div, FloatKind::F64, arguments)?
        }
        (FloatEqual, arguments) => {
            emit_float_primitive(FloatPrimitive::Equal, FloatKind::F64, arguments)?
        }
        (FloatNotEqual, arguments) => {
            emit_float_primitive(FloatPrimitive::NotEqual, FloatKind::F64, arguments)?
        }
        (FloatLess, arguments) => {
            emit_float_primitive(FloatPrimitive::Less, FloatKind::F64, arguments)?
        }
        (FloatLessEqual, arguments) => {
            emit_float_primitive(FloatPrimitive::LessEqual, FloatKind::F64, arguments)?
        }
        (FloatGreater, arguments) => {
            emit_float_primitive(FloatPrimitive::Greater, FloatKind::F64, arguments)?
        }
        (FloatGreaterEqual, arguments) => {
            emit_float_primitive(FloatPrimitive::GreaterEqual, FloatKind::F64, arguments)?
        }
        (FloatMin, arguments) => {
            emit_float_primitive(FloatPrimitive::Min, FloatKind::F64, arguments)?
        }
        (FloatMax, arguments) => {
            emit_float_primitive(FloatPrimitive::Max, FloatKind::F64, arguments)?
        }
        (FloatRound32, [value]) => syn::parse_quote! { ((#value) as f32) as f64 },
        (FloatWiden64, [value]) => syn::parse_quote! { #value },
        (Complex128ToComplex64, [value]) => syn::parse_quote! {
            [((#value)[0] as f32) as f64, ((#value)[1] as f32) as f64]
        },
        (Complex64ToComplex128, [value]) => syn::parse_quote! { #value },
        (ComplexNeg, [value]) => syn::parse_quote! { [-(#value)[0], -(#value)[1]] },
        (ComplexReal | Complex64Real, [value]) => syn::parse_quote! { (#value)[0] },
        (ComplexImag | Complex64Imag, [value]) => syn::parse_quote! { (#value)[1] },
        (ComplexFromParts, [real, imag]) => syn::parse_quote! { [#real, #imag] },
        (ComplexAdd, [left, right]) => {
            syn::parse_quote! { [(#left)[0] + (#right)[0], (#left)[1] + (#right)[1]] }
        }
        (ComplexSub, [left, right]) => {
            syn::parse_quote! { [(#left)[0] - (#right)[0], (#left)[1] - (#right)[1]] }
        }
        (ComplexMul, [left, right]) => syn::parse_quote! {
            [
                (#left)[0] * (#right)[0] - (#left)[1] * (#right)[1],
                (#left)[0] * (#right)[1] + (#left)[1] * (#right)[0],
            ]
        },
        (ComplexDiv, [left, right]) => syn::parse_quote! {
            {
                let __gors_denominator = (#right)[0] * (#right)[0] + (#right)[1] * (#right)[1];
                [
                    ((#left)[0] * (#right)[0] + (#left)[1] * (#right)[1]) / __gors_denominator,
                    ((#left)[1] * (#right)[0] - (#left)[0] * (#right)[1]) / __gors_denominator,
                ]
            }
        },
        (BoolEqual | StringEqual, [left, right]) => syn::parse_quote! { (#left) == (#right) },
        (BoolNotEqual | StringNotEqual, [left, right]) => {
            syn::parse_quote! { (#left) != (#right) }
        }
        (ComplexEqual, [left, right]) => syn::parse_quote! {
            (#left)[0] == (#right)[0] && (#left)[1] == (#right)[1]
        },
        (ComplexNotEqual, [left, right]) => syn::parse_quote! {
            (#left)[0] != (#right)[0] || (#left)[1] != (#right)[1]
        },
        (StringLess, [left, right]) => syn::parse_quote! { (#left) < (#right) },
        (StringLessEqual, [left, right]) => syn::parse_quote! { (#left) <= (#right) },
        (StringGreater, [left, right]) => syn::parse_quote! { (#left) > (#right) },
        (StringGreaterEqual, [left, right]) => syn::parse_quote! { (#left) >= (#right) },
        (operation, _) => {
            return Err(Diagnostic::backend(format!(
                "invalid verified primitive operation arity for {operation:?}"
            )));
        }
    };
    Ok(expression)
}

fn emit_float_primitive(
    operation: FloatPrimitive,
    kind: FloatKind,
    args: &[syn::Expr],
) -> Result<syn::Expr, Diagnostic> {
    let narrow = |value: &syn::Expr| -> syn::Expr {
        match kind {
            FloatKind::F32 => syn::parse_quote! { (#value) as f32 },
            FloatKind::F64 => value.clone(),
        }
    };
    let widen = |value: syn::Expr| -> syn::Expr {
        match kind {
            FloatKind::F32 => syn::parse_quote! { (#value) as f64 },
            FloatKind::F64 => value,
        }
    };
    let result = match (operation, args) {
        (FloatPrimitive::Neg, [value]) => {
            let value = narrow(value);
            widen(syn::parse_quote! { -(#value) })
        }
        (operation, [left, right]) => {
            let left = narrow(left);
            let right = narrow(right);
            match operation {
                FloatPrimitive::Add => widen(syn::parse_quote! { (#left) + (#right) }),
                FloatPrimitive::Sub => widen(syn::parse_quote! { (#left) - (#right) }),
                FloatPrimitive::Mul => widen(syn::parse_quote! { (#left) * (#right) }),
                FloatPrimitive::Div => widen(syn::parse_quote! { (#left) / (#right) }),
                FloatPrimitive::Equal => syn::parse_quote! { (#left) == (#right) },
                FloatPrimitive::NotEqual => syn::parse_quote! { (#left) != (#right) },
                FloatPrimitive::Less => syn::parse_quote! { (#left) < (#right) },
                FloatPrimitive::LessEqual => syn::parse_quote! { (#left) <= (#right) },
                FloatPrimitive::Greater => syn::parse_quote! { (#left) > (#right) },
                FloatPrimitive::GreaterEqual => syn::parse_quote! { (#left) >= (#right) },
                FloatPrimitive::Min => widen(emit_float_min(left, right)),
                FloatPrimitive::Max => widen(emit_float_max(left, right)),
                FloatPrimitive::Neg => {
                    return Err(Diagnostic::backend(
                        "verified float negation has binary arity",
                    ));
                }
            }
        }
        _ => {
            return Err(Diagnostic::backend(format!(
                "invalid verified {kind:?} operation arity for {operation:?}"
            )));
        }
    };
    Ok(result)
}

fn emit_float_min(left: syn::Expr, right: syn::Expr) -> syn::Expr {
    syn::parse_quote! {
        if (#left).is_nan() {
            #left
        } else if (#right).is_nan() {
            #right
        } else if (#left) == 0.0 && (#right) == 0.0 {
            if (#left).is_sign_negative() { #left } else { #right }
        } else if (#left) < (#right) {
            #left
        } else {
            #right
        }
    }
}

fn emit_float_max(left: syn::Expr, right: syn::Expr) -> syn::Expr {
    syn::parse_quote! {
        if (#left).is_nan() {
            #left
        } else if (#right).is_nan() {
            #right
        } else if (#left) == 0.0 && (#right) == 0.0 {
            if (#left).is_sign_positive() { #left } else { #right }
        } else if (#left) > (#right) {
            #left
        } else {
            #right
        }
    }
}

fn emit_integer_to_float(from: IntegerKind, to: FloatKind, value: &syn::Expr) -> syn::Expr {
    let integer: syn::Expr = match from {
        IntegerKind::I8 => syn::parse_quote! { (#value) as i8 },
        IntegerKind::I16 => syn::parse_quote! { (#value) as i16 },
        IntegerKind::I32 => syn::parse_quote! { (#value) as i32 },
        IntegerKind::I64 => syn::parse_quote! { #value },
        IntegerKind::U8 => syn::parse_quote! { (#value) as u8 },
        IntegerKind::U16 => syn::parse_quote! { (#value) as u16 },
        IntegerKind::U32 => syn::parse_quote! { (#value) as u32 },
        IntegerKind::U64 => syn::parse_quote! { (#value) as u64 },
    };
    match to {
        FloatKind::F32 => syn::parse_quote! { ((#integer) as f32) as f64 },
        FloatKind::F64 => syn::parse_quote! { (#integer) as f64 },
    }
}

fn emit_float_to_integer(from: FloatKind, to: IntegerKind, value: &syn::Expr) -> syn::Expr {
    let float: syn::Expr = match from {
        FloatKind::F32 => syn::parse_quote! { (#value) as f32 },
        FloatKind::F64 => value.clone(),
    };
    match to {
        IntegerKind::I8 => syn::parse_quote! { ((#float) as i8) as i64 },
        IntegerKind::I16 => syn::parse_quote! { ((#float) as i16) as i64 },
        IntegerKind::I32 => syn::parse_quote! { ((#float) as i32) as i64 },
        IntegerKind::I64 => syn::parse_quote! { (#float) as i64 },
        IntegerKind::U8 => syn::parse_quote! { ((#float) as u8) as i64 },
        IntegerKind::U16 => syn::parse_quote! { ((#float) as u16) as i64 },
        IntegerKind::U32 => syn::parse_quote! { ((#float) as u32) as i64 },
        IntegerKind::U64 => syn::parse_quote! { ((#float) as u64) as i64 },
    }
}
