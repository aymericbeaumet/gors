pub(super) type Names = std::collections::HashSet<String>;

pub(super) fn collect(file: &syn::File) -> Names {
    file.items
        .iter()
        .filter_map(|item| {
            let syn::Item::Struct(item_struct) = item else {
                return None;
            };
            let syn::Fields::Unnamed(fields) = &item_struct.fields else {
                return None;
            };
            (fields.unnamed.len() == 1).then(|| item_struct.ident.to_string())
        })
        .collect()
}

pub(super) fn coerce_assignment(
    assign: &mut syn::ExprAssign,
    impl_self_types: &[String],
    tuple_newtypes: &Names,
) {
    if let Some(self_ty) = impl_self_types.last()
        && tuple_newtypes.contains(self_ty)
        && is_deref_self_expr(&assign.left)
        && rhs_takes_self_underlying(&assign.right)
    {
        let ident = syn::Ident::new(self_ty, proc_macro2::Span::mixed_site());
        let right = assign.right.clone();
        *assign.right = syn::parse_quote! { #ident(#right) };
    }
}

pub(super) fn coerce_cast(
    cast: &mut syn::ExprCast,
    impl_self_types: &[String],
    tuple_newtypes: &Names,
) {
    if let Some(self_ty) = impl_self_types.last()
        && tuple_newtypes.contains(self_ty)
        && is_self_or_deref_self_expr(&cast.expr)
    {
        *cast.expr = syn::parse_quote! { self.0 };
    }
}

pub(super) fn coerce_numeric_from_call(
    call: &mut syn::ExprCall,
    impl_self_types: &[String],
    tuple_newtypes: &Names,
) {
    if let Some(self_ty) = impl_self_types.last()
        && tuple_newtypes.contains(self_ty)
        && is_numeric_from_call(&call.func)
        && call.args.first().is_some_and(super::is_self_expr)
        && let Some(first) = call.args.first_mut()
    {
        *first = syn::parse_quote! { *self };
    }
}

fn is_deref_self_expr(expr: &syn::Expr) -> bool {
    if let syn::Expr::Paren(paren) = expr {
        return is_deref_self_expr(&paren.expr);
    }
    let syn::Expr::Unary(unary) = expr else {
        return false;
    };
    matches!(unary.op, syn::UnOp::Deref(_)) && super::is_self_expr(&unary.expr)
}

fn is_self_or_deref_self_expr(expr: &syn::Expr) -> bool {
    if let syn::Expr::Paren(paren) = expr {
        return is_self_or_deref_self_expr(&paren.expr);
    }
    super::is_self_expr(expr) || is_deref_self_expr(expr)
}

fn rhs_takes_self_underlying(expr: &syn::Expr) -> bool {
    let syn::Expr::Call(call) = expr else {
        return false;
    };
    if super::is_path_call(&call.func, &["crate", "builtin", "append"])
        || super::is_path_call(&call.func, &["builtin", "append"])
    {
        return false;
    }
    call.args.first().is_some_and(is_mem_take_self_call)
}

fn is_mem_take_self_call(expr: &syn::Expr) -> bool {
    let syn::Expr::Call(call) = expr else {
        return false;
    };
    if !super::is_path_call(&call.func, &["std", "mem", "take"]) {
        return false;
    }
    call.args.first().is_some_and(super::is_self_expr)
}

fn is_numeric_from_call(func: &syn::Expr) -> bool {
    const NUMERIC_TYPES: &[&str] = &[
        "isize", "i8", "i16", "i32", "i64", "usize", "u8", "u16", "u32", "u64", "f32", "f64",
    ];
    NUMERIC_TYPES
        .iter()
        .any(|ty| super::is_path_call(func, &[*ty, "from"]))
}
