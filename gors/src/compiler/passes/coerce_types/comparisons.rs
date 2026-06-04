use super::super::super::syn_inspect::{
    arc_mutex_new_inner_expr, box_new_call_arg, pat_ident_name, type_path_ident_name,
    vec_type_inner,
};
use std::collections::{HashMap, HashSet};

pub(super) type IntegerConstTypes = HashMap<String, syn::Type>;

#[derive(Default)]
pub(super) struct ByteIndexScope {
    roots: HashSet<String>,
}

impl ByteIndexScope {
    pub(super) fn collect(sig: &syn::Signature) -> Self {
        let roots = sig
            .inputs
            .iter()
            .filter_map(|arg| {
                let syn::FnArg::Typed(pat_type) = arg else {
                    return None;
                };
                is_byte_sequence_type(&pat_type.ty).then(|| pat_ident_name(&pat_type.pat))?
            })
            .collect();
        Self { roots }
    }

    fn contains(&self, name: &str) -> bool {
        self.roots.contains(name)
    }
}

pub(super) fn collect_integer_const_types(file: &syn::File) -> IntegerConstTypes {
    file.items
        .iter()
        .filter_map(|item| {
            let syn::Item::Const(item_const) = item else {
                return None;
            };
            is_integer_type(&item_const.ty)
                .then(|| (item_const.ident.to_string(), (*item_const.ty).clone()))
        })
        .collect()
}

pub(super) fn coerce_binary_expr(
    binary: &mut syn::ExprBinary,
    integer_const_types: &IntegerConstTypes,
    byte_index_scope: Option<&ByteIndexScope>,
) {
    coerce_byte_index_const_comparison(binary, integer_const_types, byte_index_scope);
    if !matches!(binary.op, syn::BinOp::Eq(_) | syn::BinOp::Ne(_)) {
        return;
    }

    if let Some(inner) = box_new_call_arg(&binary.right) {
        let left = binary.left.clone();
        *binary.left = syn::parse_quote! { *#left };
        *binary.right = inner;
    } else if let Some(inner) = box_new_call_arg(&binary.left) {
        let right = binary.right.clone();
        *binary.left = inner;
        *binary.right = syn::parse_quote! { *#right };
    } else if let (Some(left), Some(right)) = (
        arc_mutex_new_inner_expr(&binary.left),
        arc_mutex_new_inner_expr(&binary.right),
    ) {
        *binary.left = left;
        *binary.right = right;
    }
}

fn coerce_byte_index_const_comparison(
    binary: &mut syn::ExprBinary,
    integer_const_types: &IntegerConstTypes,
    byte_index_scope: Option<&ByteIndexScope>,
) {
    if !is_comparison_op(&binary.op) {
        return;
    }
    let Some(scope) = byte_index_scope else {
        return;
    };

    if let Some(ty) = integer_const_type(&binary.right, integer_const_types)
        && byte_index_expr(&binary.left, scope)
        && !is_u8_type(ty)
    {
        let left = (*binary.left).clone();
        let ty = ty.clone();
        *binary.left = syn::parse_quote! { (#left as #ty) };
    } else if let Some(ty) = integer_const_type(&binary.left, integer_const_types)
        && byte_index_expr(&binary.right, scope)
        && !is_u8_type(ty)
    {
        let right = (*binary.right).clone();
        let ty = ty.clone();
        *binary.right = syn::parse_quote! { (#right as #ty) };
    }
}

fn integer_const_type<'a>(
    expr: &syn::Expr,
    integer_const_types: &'a IntegerConstTypes,
) -> Option<&'a syn::Type> {
    let syn::Expr::Path(path) = expr else {
        return None;
    };
    if path.qself.is_some() || path.path.segments.len() != 1 {
        return None;
    }
    let name = path.path.segments.first()?.ident.to_string();
    integer_const_types.get(&name)
}

fn byte_index_expr(expr: &syn::Expr, scope: &ByteIndexScope) -> bool {
    let syn::Expr::Index(index) = expr else {
        return false;
    };
    byte_sequence_expr(&index.expr, scope)
}

fn byte_sequence_expr(expr: &syn::Expr, scope: &ByteIndexScope) -> bool {
    match expr {
        syn::Expr::Path(path) if path.qself.is_none() && path.path.segments.len() == 1 => path
            .path
            .segments
            .first()
            .is_some_and(|seg| scope.contains(&seg.ident.to_string())),
        syn::Expr::MethodCall(call) if call.args.is_empty() && call.method == "as_bytes" => true,
        syn::Expr::Paren(paren) => byte_sequence_expr(&paren.expr, scope),
        syn::Expr::Group(group) => byte_sequence_expr(&group.expr, scope),
        _ => false,
    }
}

fn is_byte_sequence_type(ty: &syn::Type) -> bool {
    let ty = match ty {
        syn::Type::Reference(reference) => &*reference.elem,
        _ => ty,
    };
    if let Some(inner) = vec_type_inner(ty) {
        return type_path_ident_name(&inner).is_some_and(|name| name == "u8");
    }
    if let syn::Type::Slice(slice) = ty {
        return type_path_ident_name(&slice.elem).is_some_and(|name| name == "u8");
    }
    false
}

fn is_integer_type(ty: &syn::Type) -> bool {
    type_path_ident_name(ty).is_some_and(|name| {
        matches!(
            name.as_str(),
            "isize"
                | "usize"
                | "i8"
                | "i16"
                | "i32"
                | "i64"
                | "i128"
                | "u8"
                | "u16"
                | "u32"
                | "u64"
                | "u128"
        )
    })
}

fn is_u8_type(ty: &syn::Type) -> bool {
    type_path_ident_name(ty).is_some_and(|name| name == "u8")
}

fn is_comparison_op(op: &syn::BinOp) -> bool {
    matches!(
        op,
        syn::BinOp::Eq(_)
            | syn::BinOp::Ne(_)
            | syn::BinOp::Lt(_)
            | syn::BinOp::Le(_)
            | syn::BinOp::Gt(_)
            | syn::BinOp::Ge(_)
    )
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::super::pass;
    use quote::quote;

    #[test]
    fn byte_index_comparisons_use_integer_const_type_not_const_name() {
        let mut file: syn::File = syn::parse_quote! {
            pub const Limit: u32 = 128;
            pub const ByteLimit: u8 = 7;

            pub fn check(mut data: Vec<u8>, mut other: Vec<i32>, mut text: String) -> bool {
                data[0] < Limit
                    && Limit > data[1]
                    && data[2] == ByteLimit
                    && other[0] < Limit
                    && text.as_bytes()[0] < Limit
            }
        };

        pass(&mut file);

        let tokens = quote!(#file).to_string();
        assert!(
            tokens.contains("(data [0] as u32) < Limit"),
            "expected byte index to cast to typed integer const: {tokens}"
        );
        assert!(
            tokens.contains("Limit > (data [1] as u32)"),
            "expected right-side byte index to cast to typed integer const: {tokens}"
        );
        assert!(
            tokens.contains("data [2] == ByteLimit"),
            "expected u8 constants not to force a byte index cast: {tokens}"
        );
        assert!(
            tokens.contains("other [0] < Limit"),
            "expected non-byte indexed values not to be rewritten: {tokens}"
        );
        assert!(
            tokens.contains("(text . as_bytes () [0] as u32) < Limit"),
            "expected String byte reads to use the same typed const rule: {tokens}"
        );
        assert!(
            !tokens.contains("RuneSelf"),
            "regression should not depend on unicode/utf8 constant names: {tokens}"
        );
    }
}
