pub(super) fn coerce_call(call: &mut syn::ExprCall) {
    if super::syntax::is_path_call(&call.func, &["crate", "builtin", "append"])
        || super::syntax::is_path_call(&call.func, &["builtin", "append"])
    {
        if let Some(first) = call.args.first_mut() {
            replace_self_deref_with_take(first);
            replace_self_field_with_take(first);
        }
        if let Some(second) = call.args.iter_mut().nth(1) {
            clone_field_or_path(second);
        }
    }
}

pub(super) fn coerce_local(local: &mut syn::Local) {
    if let Some(init) = &mut local.init {
        replace_self_deref_with_take(&mut init.expr);
    }
}

fn replace_self_deref_with_take(expr: &mut syn::Expr) {
    let replacement = match expr {
        syn::Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Deref(_)) => {
            if super::syntax::is_self_expr(&unary.expr) {
                Some(syn::parse_quote! { std::mem::take(self) })
            } else if is_self_field_expr(&unary.expr) {
                let inner = unary.expr.clone();
                Some(syn::parse_quote! { std::mem::take(&mut *#inner) })
            } else {
                None
            }
        }
        _ => None,
    };

    if let Some(replacement) = replacement {
        *expr = replacement;
    }
}

fn replace_self_field_with_take(expr: &mut syn::Expr) {
    if !matches!(expr, syn::Expr::Field(field) if super::syntax::is_self_expr(&field.base)) {
        return;
    }

    let inner = expr.clone();
    *expr = syn::parse_quote! { std::mem::take(&mut #inner) };
}

fn is_self_field_expr(expr: &syn::Expr) -> bool {
    matches!(expr, syn::Expr::Field(field) if super::syntax::is_self_expr(&field.base))
}

fn clone_field_or_path(expr: &mut syn::Expr) {
    if !matches!(expr, syn::Expr::Path(_) | syn::Expr::Field(_)) {
        return;
    }
    let inner = expr.clone();
    *expr = syn::parse_quote! { (#inner).clone() };
}
