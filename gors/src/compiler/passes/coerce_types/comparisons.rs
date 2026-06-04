use super::super::super::syn_inspect::{arc_mutex_new_inner_expr, box_new_call_arg};

pub(super) fn coerce_binary_expr(binary: &mut syn::ExprBinary) {
    if is_rune_self_path(&binary.right) && matches!(&*binary.left, syn::Expr::Index(_)) {
        let left = (*binary.left).clone();
        *binary.left = syn::parse_quote! { (#left as u32) };
    }

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

fn is_rune_self_path(expr: &syn::Expr) -> bool {
    let syn::Expr::Path(path) = expr else {
        return false;
    };
    path.path
        .segments
        .last()
        .is_some_and(|seg| seg.ident == "RuneSelf")
}
