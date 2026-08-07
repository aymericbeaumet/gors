use crate::compiler::Diagnostic;
use crate::compiler::rust_ir::{IntegerKind, IntegerPrimitive};

pub(super) fn emit_integer_primitive(
    operation: IntegerPrimitive,
    kind: IntegerKind,
    args: &[syn::Expr],
) -> Result<syn::Expr, Diagnostic> {
    let expression = match (operation, args) {
        (IntegerPrimitive::BitNot, [value]) => {
            emit_integer_canonicalization(kind, syn::parse_quote! { !(#value) })
        }
        (IntegerPrimitive::WrappingNeg, [value]) => {
            emit_integer_canonicalization(kind, syn::parse_quote! { (#value).wrapping_neg() })
        }
        (IntegerPrimitive::BitAnd, [left, right]) => {
            emit_integer_canonicalization(kind, syn::parse_quote! { (#left) & (#right) })
        }
        (IntegerPrimitive::BitOr, [left, right]) => {
            emit_integer_canonicalization(kind, syn::parse_quote! { (#left) | (#right) })
        }
        (IntegerPrimitive::BitXor, [left, right]) => {
            emit_integer_canonicalization(kind, syn::parse_quote! { (#left) ^ (#right) })
        }
        (IntegerPrimitive::AndNot, [left, right]) => {
            emit_integer_canonicalization(kind, syn::parse_quote! { (#left) & !(#right) })
        }
        (IntegerPrimitive::WrappingAdd, [left, right]) => {
            emit_integer_canonicalization(kind, syn::parse_quote! { (#left).wrapping_add(#right) })
        }
        (IntegerPrimitive::WrappingSub, [left, right]) => {
            emit_integer_canonicalization(kind, syn::parse_quote! { (#left).wrapping_sub(#right) })
        }
        (IntegerPrimitive::WrappingMul, [left, right]) => {
            emit_integer_canonicalization(kind, syn::parse_quote! { (#left).wrapping_mul(#right) })
        }
        (IntegerPrimitive::Equal, [left, right]) => {
            syn::parse_quote! { (#left) == (#right) }
        }
        (IntegerPrimitive::NotEqual, [left, right]) => {
            syn::parse_quote! { (#left) != (#right) }
        }
        (IntegerPrimitive::Less, [left, right]) => {
            emit_integer_comparison(kind, left, right, IntegerPrimitive::Less)?
        }
        (IntegerPrimitive::LessEqual, [left, right]) => {
            emit_integer_comparison(kind, left, right, IntegerPrimitive::LessEqual)?
        }
        (IntegerPrimitive::Greater, [left, right]) => {
            emit_integer_comparison(kind, left, right, IntegerPrimitive::Greater)?
        }
        (IntegerPrimitive::GreaterEqual, [left, right]) => {
            emit_integer_comparison(kind, left, right, IntegerPrimitive::GreaterEqual)?
        }
        (IntegerPrimitive::Min, [left, right]) => {
            let comparison = emit_integer_comparison(kind, left, right, IntegerPrimitive::Less)?;
            syn::parse_quote! { if #comparison { #left } else { #right } }
        }
        (IntegerPrimitive::Max, [left, right]) => {
            let comparison = emit_integer_comparison(kind, left, right, IntegerPrimitive::Greater)?;
            syn::parse_quote! { if #comparison { #left } else { #right } }
        }
        (operation, _) => {
            return Err(Diagnostic::backend(format!(
                "invalid verified {kind:?} integer operation arity for {operation:?}"
            )));
        }
    };
    Ok(expression)
}

pub(super) fn emit_integer_canonicalization(kind: IntegerKind, value: syn::Expr) -> syn::Expr {
    match kind {
        IntegerKind::I8 => syn::parse_quote! { ((#value) as i8) as i64 },
        IntegerKind::I16 => syn::parse_quote! { ((#value) as i16) as i64 },
        IntegerKind::I32 => syn::parse_quote! { ((#value) as i32) as i64 },
        IntegerKind::I64 | IntegerKind::U64 => syn::parse_quote! { #value },
        IntegerKind::U8 => syn::parse_quote! { ((#value) as u8) as i64 },
        IntegerKind::U16 => syn::parse_quote! { ((#value) as u16) as i64 },
        IntegerKind::U32 => syn::parse_quote! { ((#value) as u32) as i64 },
    }
}

fn emit_integer_comparison(
    kind: IntegerKind,
    left: &syn::Expr,
    right: &syn::Expr,
    operation: IntegerPrimitive,
) -> Result<syn::Expr, Diagnostic> {
    let left = if kind.is_signed() {
        left.clone()
    } else {
        syn::parse_quote! { (#left) as u64 }
    };
    let right = if kind.is_signed() {
        right.clone()
    } else {
        syn::parse_quote! { (#right) as u64 }
    };
    Ok(match operation {
        IntegerPrimitive::Less => syn::parse_quote! { (#left) < (#right) },
        IntegerPrimitive::LessEqual => syn::parse_quote! { (#left) <= (#right) },
        IntegerPrimitive::Greater => syn::parse_quote! { (#left) > (#right) },
        IntegerPrimitive::GreaterEqual => syn::parse_quote! { (#left) >= (#right) },
        operation => {
            return Err(Diagnostic::backend(format!(
                "invalid verified integer comparison operation {operation:?}"
            )));
        }
    })
}
