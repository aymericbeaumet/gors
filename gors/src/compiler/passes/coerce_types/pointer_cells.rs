use syn::visit_mut::{self, VisitMut};

pub(super) fn pass_after_package_merge(file: &mut syn::File) {
    let mutable_ref_call_args = super::collect_mutable_ref_call_args(file);
    let pointer_cell_statics = super::collect_pointer_cell_statics(file);
    if mutable_ref_call_args.is_empty() {
        return;
    }
    CoercePointerCellArgs {
        mutable_ref_call_args,
        pointer_cell_statics,
        pointer_cell_names: Vec::new(),
        pointer_cell_iter_names: Vec::new(),
    }
    .visit_file_mut(file);
}

struct CoercePointerCellArgs {
    mutable_ref_call_args: std::collections::HashMap<String, std::collections::HashSet<usize>>,
    pointer_cell_statics: std::collections::HashSet<String>,
    pointer_cell_names: Vec<std::collections::HashSet<String>>,
    pointer_cell_iter_names: Vec<std::collections::HashSet<String>>,
}

impl VisitMut for CoercePointerCellArgs {
    fn visit_item_fn_mut(&mut self, func: &mut syn::ItemFn) {
        let scope = pointer_cell_arg_scope(&func.sig);
        self.pointer_cell_names.push(scope.values);
        self.pointer_cell_iter_names.push(scope.iterables);
        visit_mut::visit_item_fn_mut(self, func);
        self.pointer_cell_iter_names.pop();
        self.pointer_cell_names.pop();
    }

    fn visit_impl_item_fn_mut(&mut self, func: &mut syn::ImplItemFn) {
        let scope = pointer_cell_arg_scope(&func.sig);
        self.pointer_cell_names.push(scope.values);
        self.pointer_cell_iter_names.push(scope.iterables);
        visit_mut::visit_impl_item_fn_mut(self, func);
        self.pointer_cell_iter_names.pop();
        self.pointer_cell_names.pop();
    }

    fn visit_expr_for_loop_mut(&mut self, for_loop: &mut syn::ExprForLoop) {
        visit_mut::visit_expr_mut(self, &mut for_loop.expr);
        let bindings = pointer_cell_for_loop_bindings(
            &for_loop.pat,
            &for_loop.expr,
            &self.pointer_cell_iter_names,
        );
        if bindings.is_empty() {
            visit_mut::visit_block_mut(self, &mut for_loop.body);
            return;
        }

        self.pointer_cell_names.push(bindings);
        visit_mut::visit_block_mut(self, &mut for_loop.body);
        self.pointer_cell_names.pop();
    }

    fn visit_expr_call_mut(&mut self, call: &mut syn::ExprCall) {
        visit_mut::visit_expr_call_mut(self, call);
        coerce_pointer_cell_call_args(
            &call.func,
            &mut call.args,
            &self.mutable_ref_call_args,
            &self.pointer_cell_statics,
            &self.pointer_cell_names,
        );
    }
}

struct PointerCellArgScope {
    values: std::collections::HashSet<String>,
    iterables: std::collections::HashSet<String>,
}

fn pointer_cell_arg_scope(sig: &syn::Signature) -> PointerCellArgScope {
    let mut values = std::collections::HashSet::new();
    let mut iterables = std::collections::HashSet::new();

    for input in &sig.inputs {
        let syn::FnArg::Typed(pat_type) = input else {
            continue;
        };
        let Some(name) = super::pat_ident_name(&pat_type.pat) else {
            continue;
        };
        if type_is_pointer_cell(&pat_type.ty) {
            values.insert(name.clone());
        }
        if type_is_pointer_cell_iterable(&pat_type.ty) {
            iterables.insert(name);
        }
    }

    PointerCellArgScope { values, iterables }
}

fn type_is_pointer_cell(ty: &syn::Type) -> bool {
    let Some(inner) = super::first_type_arg_if_path_last_ident(ty, "Arc") else {
        return false;
    };
    super::first_type_arg_if_path_last_ident(inner, "Mutex").is_some()
}

fn type_is_pointer_cell_iterable(ty: &syn::Type) -> bool {
    match ty {
        syn::Type::Reference(reference) => type_is_pointer_cell_iterable(&reference.elem),
        syn::Type::Slice(slice) => type_is_pointer_cell(&slice.elem),
        _ => super::first_type_arg_if_path_last_ident(ty, "Vec").is_some_and(type_is_pointer_cell),
    }
}

fn pointer_cell_for_loop_bindings(
    pat: &syn::Pat,
    iter: &syn::Expr,
    pointer_cell_iter_scopes: &[std::collections::HashSet<String>],
) -> std::collections::HashSet<String> {
    let iter_names = pointer_cell_iter_scopes
        .iter()
        .flatten()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    if iter_names.is_empty()
        || !super::evaluation_order::expr_contains_any_path_ident(iter, &iter_names)
    {
        return std::collections::HashSet::new();
    }
    let skip_first = super::evaluation_order::expr_contains_method_call(iter, "enumerate");
    pat_value_ident_names(pat, skip_first)
}

fn pat_value_ident_names(
    pat: &syn::Pat,
    skip_first_tuple_field: bool,
) -> std::collections::HashSet<String> {
    match pat {
        syn::Pat::Tuple(tuple) if skip_first_tuple_field => tuple
            .elems
            .iter()
            .skip(1)
            .flat_map(pat_ident_names)
            .collect(),
        _ => pat_ident_names(pat).into_iter().collect(),
    }
}

fn pat_ident_names(pat: &syn::Pat) -> Vec<String> {
    match pat {
        syn::Pat::Ident(ident) => vec![ident.ident.to_string()],
        syn::Pat::Reference(reference) => pat_ident_names(&reference.pat),
        syn::Pat::Tuple(tuple) => tuple.elems.iter().flat_map(pat_ident_names).collect(),
        syn::Pat::TupleStruct(tuple) => tuple.elems.iter().flat_map(pat_ident_names).collect(),
        syn::Pat::Type(pat_type) => pat_ident_names(&pat_type.pat),
        _ => Vec::new(),
    }
}

fn coerce_pointer_cell_call_args(
    func: &syn::Expr,
    args: &mut syn::punctuated::Punctuated<syn::Expr, syn::token::Comma>,
    mutable_ref_call_args: &std::collections::HashMap<String, std::collections::HashSet<usize>>,
    pointer_cell_statics: &std::collections::HashSet<String>,
    pointer_cell_name_scopes: &[std::collections::HashSet<String>],
) {
    let Some(name) = super::call_func_name(func) else {
        return;
    };
    let Some(indices) = mutable_ref_call_args.get(&name) else {
        return;
    };
    for (index, arg) in args.iter_mut().enumerate() {
        if indices.contains(&index) {
            borrow_pointer_cell_expr(arg, pointer_cell_statics, pointer_cell_name_scopes);
        }
    }
}

pub(super) fn borrow_static_expr(
    expr: &mut syn::Expr,
    pointer_cell_statics: &std::collections::HashSet<String>,
) -> bool {
    let Some(name) = deref_clone_path_name(expr) else {
        return false;
    };
    if !pointer_cell_statics.contains(&name) {
        return false;
    }
    let ident = syn::Ident::new(&name, proc_macro2::Span::mixed_site());
    *expr = syn::parse_quote! { &mut *((*#ident)).lock().unwrap() };
    true
}

fn borrow_pointer_cell_expr(
    expr: &mut syn::Expr,
    pointer_cell_statics: &std::collections::HashSet<String>,
    pointer_cell_name_scopes: &[std::collections::HashSet<String>],
) -> bool {
    if borrow_static_expr(expr, pointer_cell_statics) {
        return true;
    }
    let Some(name) = pointer_cell_arg_name(expr) else {
        return false;
    };
    if !pointer_cell_name_scopes
        .iter()
        .rev()
        .any(|scope| scope.contains(&name))
    {
        return false;
    }
    let ident = syn::Ident::new(&name, proc_macro2::Span::mixed_site());
    *expr = syn::parse_quote! { &mut *#ident.lock().unwrap() };
    true
}

fn pointer_cell_arg_name(expr: &syn::Expr) -> Option<String> {
    mut_reference_path_name(expr)
        .or_else(|| super::path_ident_name(expr))
        .or_else(|| strip_clone_method_call(expr).and_then(|expr| super::path_ident_name(&expr)))
}

fn mut_reference_path_name(expr: &syn::Expr) -> Option<String> {
    let expr = strip_paren_or_group(expr);
    let syn::Expr::Reference(reference) = expr else {
        return None;
    };
    reference.mutability.as_ref()?;
    super::path_ident_name(&reference.expr)
}

fn strip_clone_method_call(expr: &syn::Expr) -> Option<syn::Expr> {
    let expr = strip_paren_or_group(expr);
    let syn::Expr::MethodCall(method) = expr else {
        return None;
    };
    if method.method != "clone" || !method.args.is_empty() {
        return None;
    }
    Some((*method.receiver).clone())
}

fn deref_clone_path_name(expr: &syn::Expr) -> Option<String> {
    let expr = strip_paren_or_group(expr);
    let syn::Expr::MethodCall(method) = expr else {
        return None;
    };
    if method.method != "clone" || !method.args.is_empty() {
        return None;
    }
    deref_path_name(&method.receiver)
}

fn deref_path_name(expr: &syn::Expr) -> Option<String> {
    let expr = strip_paren_or_group(expr);
    let syn::Expr::Unary(unary) = expr else {
        return None;
    };
    if !matches!(unary.op, syn::UnOp::Deref(_)) {
        return None;
    }
    super::path_ident_name(strip_paren_or_group(&unary.expr))
}

fn strip_paren_or_group(mut expr: &syn::Expr) -> &syn::Expr {
    loop {
        match expr {
            syn::Expr::Paren(paren) => expr = &paren.expr,
            syn::Expr::Group(group) => expr = &group.expr,
            _ => return expr,
        }
    }
}
