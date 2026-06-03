use syn::{
    Token,
    visit_mut::{self, VisitMut},
};

mod evaluation_order;
mod pointer_cells;
mod static_false;
mod structural_helpers;

pub fn pass(file: &mut syn::File) {
    let tuple_newtypes = collect_tuple_newtypes(file);
    let mutable_ref_call_args = collect_mutable_ref_call_args(file);
    let pointer_cell_statics = collect_pointer_cell_statics(file);
    let structural_helper_metadata = structural_helpers::Metadata::collect(file);
    CoerceTypes {
        mutable_ref_call_args,
        pointer_cell_statics,
        structural_helper_metadata,
        tuple_newtypes,
        ..Default::default()
    }
    .visit_file_mut(file);
}

pub fn pass_after_package_merge(file: &mut syn::File) {
    pointer_cells::pass_after_package_merge(file);
}

pub fn pass_after_structural_helpers(file: &mut syn::File) {
    structural_helpers::pass_after_structural_helpers(file);
}

#[derive(Default)]
struct CoerceTypes {
    mutable_ref_params: Vec<std::collections::HashSet<String>>,
    generic_value_params: Vec<std::collections::HashSet<String>>,
    has_generic_params: Vec<bool>,
    mutable_ref_call_args: std::collections::HashMap<String, std::collections::HashSet<usize>>,
    pointer_cell_statics: std::collections::HashSet<String>,
    structural_helper_metadata: structural_helpers::Metadata,
    tuple_newtypes: std::collections::HashSet<String>,
    impl_self_types: Vec<String>,
}

impl VisitMut for CoerceTypes {
    fn visit_item_impl_mut(&mut self, item_impl: &mut syn::ItemImpl) {
        if let Some(self_ty) = type_path_ident_name(&item_impl.self_ty) {
            self.impl_self_types.push(self_ty);
            visit_mut::visit_item_impl_mut(self, item_impl);
            self.impl_self_types.pop();
        } else {
            visit_mut::visit_item_impl_mut(self, item_impl);
        }
    }

    fn visit_item_fn_mut(&mut self, func: &mut syn::ItemFn) {
        let scope = fn_arg_scope(&func.sig);
        self.mutable_ref_params.push(scope.mutable_refs);
        self.generic_value_params.push(scope.generic_values);
        self.has_generic_params.push(scope.has_generics);
        visit_mut::visit_item_fn_mut(self, func);
        self.has_generic_params.pop();
        self.generic_value_params.pop();
        self.mutable_ref_params.pop();

        static_false::prune_branches(&mut func.block.stmts);
        structural_helpers::prune_reflection_fallback(&mut func.block.stmts, false);
    }

    fn visit_impl_item_fn_mut(&mut self, func: &mut syn::ImplItemFn) {
        let scope = fn_arg_scope(&func.sig);
        self.mutable_ref_params.push(scope.mutable_refs);
        self.generic_value_params.push(scope.generic_values);
        self.has_generic_params.push(scope.has_generics);
        visit_mut::visit_impl_item_fn_mut(self, func);
        self.has_generic_params.pop();
        self.generic_value_params.pop();
        self.mutable_ref_params.pop();

        static_false::prune_branches(&mut func.block.stmts);
        let prune_self_value = self.impl_self_types.last().is_some_and(|ty| {
            self.structural_helper_metadata
                .should_prune_self_value_for_initial_pass(ty, &func.block)
        });
        structural_helpers::prune_reflection_fallback(&mut func.block.stmts, prune_self_value);
    }

    fn visit_block_mut(&mut self, block: &mut syn::Block) {
        let old_stmts = std::mem::take(&mut block.stmts);
        let mut new_stmts = Vec::with_capacity(old_stmts.len());

        for mut stmt in old_stmts {
            visit_mut::visit_stmt_mut(self, &mut stmt);
            new_stmts.extend(evaluation_order::hoist_args_read_after_mut_borrow(
                &mut stmt,
            ));
            new_stmts
                .extend(evaluation_order::hoist_condition_args_read_after_mut_borrow(&mut stmt));
            new_stmts.extend(evaluation_order::hoist_method_args_read_receiver(&mut stmt));
            let needs_flush = self
                .structural_helper_metadata
                .should_flush_after_stmt(&self.impl_self_types, &stmt);
            new_stmts.push(stmt);
            if needs_flush {
                new_stmts.push(syn::parse_quote! {
                    self.__gors_flush_fmt();
                });
            }
        }

        block.stmts = new_stmts;
    }

    fn visit_expr_mut(&mut self, expr: &mut syn::Expr) {
        visit_mut::visit_expr_mut(self, expr);

        // Wrap len()/cap() calls with `as isize` since Go's len() returns int (isize)
        // but Rust's returns usize. This fixes isize/usize arithmetic mismatches.
        if let syn::Expr::Call(call) = expr {
            if is_len_or_cap_call(call) {
                let inner = expr.clone();
                *expr = syn::parse_quote! { (#inner as isize) };
            }
        }
    }

    fn visit_expr_method_call_mut(&mut self, mc: &mut syn::ExprMethodCall) {
        visit_mut::visit_expr_method_call_mut(self, mc);
        coerce_scoped_call_args(
            &mut mc.args,
            self.mutable_ref_params.last(),
            self.generic_value_params.last(),
            self.has_generic_params.last().copied().unwrap_or(false),
        );
    }

    fn visit_expr_binary_mut(&mut self, binary: &mut syn::ExprBinary) {
        visit_mut::visit_expr_binary_mut(self, binary);

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
            arc_mutex_new_call_arg(&binary.left),
            arc_mutex_new_call_arg(&binary.right),
        ) {
            *binary.left = left;
            *binary.right = right;
        }
    }

    fn visit_expr_assign_mut(&mut self, assign: &mut syn::ExprAssign) {
        visit_mut::visit_expr_assign_mut(self, assign);

        if let Some(self_ty) = self.impl_self_types.last()
            && self.tuple_newtypes.contains(self_ty)
            && is_deref_self_expr(&assign.left)
            && rhs_takes_self_underlying(&assign.right)
        {
            let ident = syn::Ident::new(self_ty, proc_macro2::Span::mixed_site());
            let right = assign.right.clone();
            *assign.right = syn::parse_quote! { #ident(#right) };
        }
    }

    fn visit_expr_cast_mut(&mut self, cast: &mut syn::ExprCast) {
        visit_mut::visit_expr_cast_mut(self, cast);

        if let Some(self_ty) = self.impl_self_types.last()
            && self.tuple_newtypes.contains(self_ty)
            && is_self_or_deref_self_expr(&cast.expr)
        {
            *cast.expr = syn::parse_quote! { self.0 };
        }
    }

    fn visit_expr_call_mut(&mut self, call: &mut syn::ExprCall) {
        visit_mut::visit_expr_call_mut(self, call);
        if let Some(self_ty) = self.impl_self_types.last()
            && self.tuple_newtypes.contains(self_ty)
            && is_numeric_from_call(&call.func)
            && call.args.first().is_some_and(is_self_expr)
            && let Some(first) = call.args.first_mut()
        {
            *first = syn::parse_quote! { *self };
        }
        coerce_scoped_call_args(
            &mut call.args,
            self.mutable_ref_params.last(),
            self.generic_value_params.last(),
            self.has_generic_params.last().copied().unwrap_or(false),
        );
        coerce_signature_call_args(
            &call.func,
            &mut call.args,
            &self.mutable_ref_call_args,
            &self.pointer_cell_statics,
        );

        if is_path_call(&call.func, &["Box", "new"]) {
            if let Some(first) = call.args.first_mut() {
                if matches!(first, syn::Expr::Field(_)) {
                    clone_field_or_path(first);
                }
            }
        }

        if is_path_call(&call.func, &["crate", "builtin", "append"])
            || is_path_call(&call.func, &["builtin", "append"])
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

    fn visit_local_mut(&mut self, local: &mut syn::Local) {
        visit_mut::visit_local_mut(self, local);

        if let Some(init) = &mut local.init {
            replace_self_deref_with_take(&mut init.expr);
        }

        // Fix: let mut max: isize = 1e6; → 1e6 is f64, needs cast
        let type_ann = get_type_annotation(local);
        if let Some(init) = &mut local.init {
            if matches!(
                &*init.expr,
                syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Float(_),
                    ..
                })
            ) {
                if let Some(ref pat_type) = type_ann {
                    if is_integer_type(pat_type) {
                        let lit = init.expr.clone();
                        *init.expr = syn::parse_quote! { #lit as #pat_type };
                    }
                }
            }
        }
    }
}

fn collect_tuple_newtypes(file: &syn::File) -> std::collections::HashSet<String> {
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

fn collect_mutable_ref_call_args(
    file: &syn::File,
) -> std::collections::HashMap<String, std::collections::HashSet<usize>> {
    let mut calls = std::collections::HashMap::new();
    for item in &file.items {
        match item {
            syn::Item::Fn(item_fn) => {
                let refs = mutable_ref_arg_indices(&item_fn.sig);
                if !refs.is_empty() {
                    calls.insert(item_fn.sig.ident.to_string(), refs);
                }
            }
            syn::Item::Impl(item_impl) => {
                let Some(self_ty) = type_path_ident_name(&item_impl.self_ty) else {
                    continue;
                };
                for item in &item_impl.items {
                    if let syn::ImplItem::Fn(func) = item {
                        let refs = mutable_ref_arg_indices(&func.sig);
                        if !refs.is_empty() {
                            calls.insert(format!("{self_ty}::{}", func.sig.ident), refs);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    calls
}

fn collect_pointer_cell_statics(file: &syn::File) -> std::collections::HashSet<String> {
    file.items
        .iter()
        .filter_map(|item| {
            let syn::Item::Static(item_static) = item else {
                return None;
            };
            lazylock_contains_arc_mutex(&item_static.ty).then(|| item_static.ident.to_string())
        })
        .collect()
}

fn lazylock_contains_arc_mutex(ty: &syn::Type) -> bool {
    let Some(inner) = first_type_arg_if_path_last_ident(ty, "LazyLock") else {
        return false;
    };
    let Some(mutex) = first_type_arg_if_path_last_ident(inner, "Arc") else {
        return false;
    };
    first_type_arg_if_path_last_ident(mutex, "Mutex").is_some()
}

fn first_type_arg_if_path_last_ident<'a>(ty: &'a syn::Type, ident: &str) -> Option<&'a syn::Type> {
    let syn::Type::Path(path) = ty else {
        return None;
    };
    let segment = path.path.segments.last()?;
    if segment.ident != ident {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
        return None;
    };
    args.args.iter().find_map(|arg| {
        if let syn::GenericArgument::Type(ty) = arg {
            Some(ty)
        } else {
            None
        }
    })
}

fn mutable_ref_arg_indices(sig: &syn::Signature) -> std::collections::HashSet<usize> {
    sig.inputs
        .iter()
        .enumerate()
        .filter_map(|(index, input)| {
            let syn::FnArg::Typed(pat_type) = input else {
                return None;
            };
            matches!(&*pat_type.ty, syn::Type::Reference(reference) if reference.mutability.is_some())
                .then_some(index)
        })
        .collect()
}

fn type_path_ident_name(ty: &syn::Type) -> Option<String> {
    let syn::Type::Path(path) = ty else {
        return None;
    };
    if path.qself.is_some() || path.path.segments.len() != 1 {
        return None;
    }
    path.path
        .segments
        .first()
        .map(|segment| segment.ident.to_string())
}

fn is_deref_self_expr(expr: &syn::Expr) -> bool {
    if let syn::Expr::Paren(paren) = expr {
        return is_deref_self_expr(&paren.expr);
    }
    let syn::Expr::Unary(unary) = expr else {
        return false;
    };
    matches!(unary.op, syn::UnOp::Deref(_)) && is_self_expr(&unary.expr)
}

fn is_self_or_deref_self_expr(expr: &syn::Expr) -> bool {
    if let syn::Expr::Paren(paren) = expr {
        return is_self_or_deref_self_expr(&paren.expr);
    }
    is_self_expr(expr) || is_deref_self_expr(expr)
}

fn rhs_takes_self_underlying(expr: &syn::Expr) -> bool {
    let syn::Expr::Call(call) = expr else {
        return false;
    };
    if is_path_call(&call.func, &["crate", "builtin", "append"])
        || is_path_call(&call.func, &["builtin", "append"])
    {
        return false;
    }
    call.args.first().is_some_and(is_mem_take_self_call)
}

fn is_mem_take_self_call(expr: &syn::Expr) -> bool {
    let syn::Expr::Call(call) = expr else {
        return false;
    };
    if !is_path_call(&call.func, &["std", "mem", "take"]) {
        return false;
    }
    call.args.first().is_some_and(is_self_expr)
}

struct FnArgScope {
    mutable_refs: std::collections::HashSet<String>,
    generic_values: std::collections::HashSet<String>,
    has_generics: bool,
}

fn fn_arg_scope(sig: &syn::Signature) -> FnArgScope {
    let generic_names: std::collections::HashSet<String> = sig
        .generics
        .params
        .iter()
        .filter_map(|param| {
            if let syn::GenericParam::Type(type_param) = param {
                Some(type_param.ident.to_string())
            } else {
                None
            }
        })
        .collect();
    let mut mutable_refs = std::collections::HashSet::new();
    let mut generic_values = std::collections::HashSet::new();

    for input in &sig.inputs {
        let syn::FnArg::Typed(pat_type) = input else {
            continue;
        };
        let Some(name) = pat_ident_name(&pat_type.pat) else {
            continue;
        };
        if matches!(&*pat_type.ty, syn::Type::Reference(reference) if reference.mutability.is_some())
        {
            mutable_refs.insert(name.clone());
        }
        if type_is_generic_param(&pat_type.ty, &generic_names)
            || type_is_cloneable_box(&pat_type.ty)
        {
            generic_values.insert(name);
        }
    }

    FnArgScope {
        mutable_refs,
        generic_values,
        has_generics: !generic_names.is_empty(),
    }
}

fn pat_ident_name(pat: &syn::Pat) -> Option<String> {
    let syn::Pat::Ident(ident) = pat else {
        return None;
    };
    Some(ident.ident.to_string())
}

fn type_is_generic_param(
    ty: &syn::Type,
    generic_names: &std::collections::HashSet<String>,
) -> bool {
    let syn::Type::Path(path) = ty else {
        return false;
    };
    if path.qself.is_some() || path.path.segments.len() != 1 {
        return false;
    }
    path.path
        .segments
        .first()
        .is_some_and(|segment| generic_names.contains(&segment.ident.to_string()))
}

fn type_is_cloneable_box(ty: &syn::Type) -> bool {
    let syn::Type::Path(path) = ty else {
        return false;
    };
    let Some(segment) = path.path.segments.first() else {
        return false;
    };
    if segment.ident != "Box" {
        return false;
    }
    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
        return false;
    };
    !args
        .args
        .iter()
        .any(|arg| matches!(arg, syn::GenericArgument::Type(syn::Type::TraitObject(_))))
}

fn coerce_scoped_call_args(
    args: &mut syn::punctuated::Punctuated<syn::Expr, Token![,]>,
    mutable_refs: Option<&std::collections::HashSet<String>>,
    generic_values: Option<&std::collections::HashSet<String>>,
    has_generic_params: bool,
) {
    for arg in args {
        remove_owned_string_reference(arg);
        if matches!(arg, syn::Expr::Reference(_)) {
            continue;
        }
        if has_generic_params && matches!(arg, syn::Expr::Index(_)) {
            clone_expr(arg);
            continue;
        }
        let Some(name) = path_ident_name(arg) else {
            continue;
        };
        if mutable_refs.is_some_and(|refs| refs.contains(&name)) {
            let ident = syn::Ident::new(&name, proc_macro2::Span::mixed_site());
            *arg = syn::parse_quote! { &mut *#ident };
        } else if generic_values.is_some_and(|values| values.contains(&name)) {
            clone_expr(arg);
        }
    }
}

fn coerce_signature_call_args(
    func: &syn::Expr,
    args: &mut syn::punctuated::Punctuated<syn::Expr, Token![,]>,
    mutable_ref_call_args: &std::collections::HashMap<String, std::collections::HashSet<usize>>,
    pointer_cell_statics: &std::collections::HashSet<String>,
) {
    let Some(name) = call_func_name(func) else {
        return;
    };
    let Some(indices) = mutable_ref_call_args.get(&name) else {
        return;
    };
    for (index, arg) in args.iter_mut().enumerate() {
        if indices.contains(&index) {
            borrow_mut_expr(arg, pointer_cell_statics);
        }
    }
}

fn call_func_name(func: &syn::Expr) -> Option<String> {
    let syn::Expr::Path(path) = func else {
        return None;
    };
    let segments = path
        .path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>();
    match segments.as_slice() {
        [name] => Some(name.clone()),
        [.., ty, method] => Some(format!("{ty}::{method}")),
        [] => None,
    }
}

fn remove_owned_string_reference(expr: &mut syn::Expr) {
    let syn::Expr::Reference(reference) = expr else {
        return;
    };
    if is_owned_to_string_expr(&reference.expr) {
        *expr = (*reference.expr).clone();
    }
}

fn is_owned_to_string_expr(expr: &syn::Expr) -> bool {
    matches!(
        expr,
        syn::Expr::MethodCall(method) if method.method == "to_string"
    ) || matches!(expr, syn::Expr::Paren(paren) if is_owned_to_string_expr(&paren.expr))
        || matches!(expr, syn::Expr::Group(group) if is_owned_to_string_expr(&group.expr))
}

fn path_ident_name(expr: &syn::Expr) -> Option<String> {
    match expr {
        syn::Expr::Path(path) => {
            if path.qself.is_some() || path.path.segments.len() != 1 {
                return None;
            }
            path.path
                .segments
                .first()
                .map(|segment| segment.ident.to_string())
        }
        syn::Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Deref(_)) => {
            path_ident_name(&unary.expr)
        }
        syn::Expr::Paren(paren) => path_ident_name(&paren.expr),
        syn::Expr::Group(group) => path_ident_name(&group.expr),
        _ => None,
    }
}

fn is_path_call(func: &syn::Expr, segments: &[&str]) -> bool {
    let syn::Expr::Path(path) = func else {
        return false;
    };
    path.path.segments.len() == segments.len()
        && path
            .path
            .segments
            .iter()
            .zip(segments)
            .all(|(seg, expected)| seg.ident == *expected)
}

fn borrow_mut_expr(expr: &mut syn::Expr, pointer_cell_statics: &std::collections::HashSet<String>) {
    if matches!(expr, syn::Expr::Reference(_)) {
        return;
    }
    if is_path_ident(expr, "self") {
        return;
    }
    if pointer_cells::borrow_static_expr(expr, pointer_cell_statics) {
        return;
    }
    if let Some(name) = path_ident_name(expr) {
        let ident = syn::Ident::new(&name, proc_macro2::Span::mixed_site());
        *expr = syn::parse_quote! { &mut #ident };
        return;
    }
    let inner = expr.clone();
    *expr = syn::parse_quote! { &mut #inner };
}

fn box_new_call_arg(expr: &syn::Expr) -> Option<syn::Expr> {
    let syn::Expr::Call(call) = expr else {
        return None;
    };
    if !is_path_call(&call.func, &["Box", "new"]) || call.args.len() != 1 {
        return None;
    }
    call.args.first().cloned()
}

fn arc_mutex_new_call_arg(expr: &syn::Expr) -> Option<syn::Expr> {
    let syn::Expr::Call(call) = expr else {
        return None;
    };
    if !is_path_call(&call.func, &["std", "sync", "Arc", "new"]) || call.args.len() != 1 {
        return None;
    }
    let Some(syn::Expr::Call(mutex_call)) = call.args.first() else {
        return None;
    };
    if !is_path_call(&mutex_call.func, &["std", "sync", "Mutex", "new"])
        || mutex_call.args.len() != 1
    {
        return None;
    }
    mutex_call.args.first().cloned()
}

fn replace_self_deref_with_take(expr: &mut syn::Expr) {
    let replacement = match expr {
        syn::Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Deref(_)) => {
            if is_self_expr(&unary.expr) {
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
    if !matches!(expr, syn::Expr::Field(field) if is_self_expr(&field.base)) {
        return;
    }

    let inner = expr.clone();
    *expr = syn::parse_quote! { std::mem::take(&mut #inner) };
}

fn clone_field_or_path(expr: &mut syn::Expr) {
    if !matches!(expr, syn::Expr::Path(_) | syn::Expr::Field(_)) {
        return;
    }
    clone_expr(expr);
}

fn clone_expr(expr: &mut syn::Expr) {
    let inner = expr.clone();
    *expr = syn::parse_quote! { (#inner).clone() };
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

fn is_self_expr(expr: &syn::Expr) -> bool {
    matches!(expr, syn::Expr::Path(path)
        if path.path.leading_colon.is_none()
            && path.path.segments.len() == 1
            && path.path.segments.first().is_some_and(|seg| seg.ident == "self"))
}

fn is_numeric_from_call(func: &syn::Expr) -> bool {
    const NUMERIC_TYPES: &[&str] = &[
        "isize", "i8", "i16", "i32", "i64", "usize", "u8", "u16", "u32", "u64", "f32", "f64",
    ];
    NUMERIC_TYPES
        .iter()
        .any(|ty| is_path_call(func, &[*ty, "from"]))
}

fn is_path_ident(expr: &syn::Expr, name: &str) -> bool {
    matches!(expr, syn::Expr::Path(path)
        if path.path.leading_colon.is_none()
            && path.path.segments.len() == 1
            && path.path.segments.first().is_some_and(|seg| seg.ident == name))
}

fn is_self_field_expr(expr: &syn::Expr) -> bool {
    matches!(expr, syn::Expr::Field(field) if is_self_expr(&field.base))
}

fn is_len_or_cap_call(call: &syn::ExprCall) -> bool {
    if let syn::Expr::Path(path) = &*call.func {
        if let Some(seg) = path.path.segments.last() {
            return matches!(seg.ident.to_string().as_str(), "len" | "cap");
        }
    }
    false
}

fn get_type_annotation(local: &syn::Local) -> Option<syn::Type> {
    if let syn::Pat::Type(pat_type) = &local.pat {
        Some((*pat_type.ty).clone())
    } else {
        None
    }
}

fn is_integer_type(ty: &syn::Type) -> bool {
    if let syn::Type::Path(tp) = ty {
        if let Some(seg) = tp.path.segments.last() {
            return matches!(
                seg.ident.to_string().as_str(),
                "isize" | "i8" | "i16" | "i32" | "i64" | "usize" | "u8" | "u16" | "u32" | "u64"
            );
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_prunes_reflection_fallback_inside_generated_block() {
        let mut stmts: Vec<syn::Stmt> = vec![syn::parse_quote! {
            '__gors_switch: {
                let mut __gors_switch_selected: isize = -1;
                if __gors_switch_selected == -1 {
                    __gors_switch_selected = 0;
                }
                if __gors_switch_selected == 0 {
                    self.fmt.fmtBs(v);
                }
                if __gors_switch_selected == 1 {
                    self.printValue(crate::reflect::ValueOf(v), verb, 0);
                }
            };
        }];

        structural_helpers::prune_reflection_fallback(&mut stmts, false);

        let tokens = quote::quote!(#(#stmts)*).to_string();
        assert!(
            tokens.contains("fmtBs"),
            "expected non-reflection switch case to remain: {tokens}"
        );
        assert!(
            !tokens.contains("printValue"),
            "expected reflection fallback to be pruned: {tokens}"
        );
        assert!(
            !tokens.contains("crate :: reflect"),
            "expected reflect dependency to be pruned: {tokens}"
        );
    }

    #[test]
    fn it_does_not_prune_reflect_mentions_inside_literals() {
        let mut stmts: Vec<syn::Stmt> = vec![syn::parse_quote! {
            let msg = "crate :: reflect :: ValueOf";
        }];

        structural_helpers::prune_reflection_fallback(&mut stmts, false);

        let tokens = quote::quote!(#(#stmts)*).to_string();
        assert!(
            tokens.contains("let msg"),
            "expected string-literal reflect mention to remain: {tokens}"
        );
    }

    #[test]
    fn it_prunes_reflect_type_paths_inside_generated_fallbacks() {
        let mut stmts: Vec<syn::Stmt> = vec![syn::parse_quote! {
            value.is::<crate::reflect::Value>();
        }];

        structural_helpers::prune_reflection_fallback(&mut stmts, false);

        let tokens = quote::quote!(#(#stmts)*).to_string();
        assert!(
            !tokens.contains("crate :: reflect"),
            "expected reflect type-path fallback to be pruned: {tokens}"
        );
    }

    #[test]
    fn it_does_not_replace_named_bodies_from_literal_mentions() {
        let mut file: syn::File = syn::parse_quote! {
            pub struct pp;

            pub fn newPrinter() -> String {
                "ppFree".to_string()
            }

            pub struct Sink;

            impl Sink {
                pub fn fmtString(&mut self, mut v: String) {
                    let marker = "fmtQ";
                    let _ = (marker, v);
                }
            }
        };

        pass(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            tokens.contains("to_string") && tokens.contains("let marker"),
            "expected literal mentions not to trigger body replacements: {tokens}"
        );
        assert!(
            !tokens.contains("pp :: default") && !tokens.contains("fmtS (v)"),
            "expected no token-string-driven body replacement: {tokens}"
        );
    }

    #[test]
    fn it_does_not_replace_pad_string_body_by_name() {
        let mut file: syn::File = syn::parse_quote! {
            pub struct Buffer;

            impl Buffer {
                pub fn writeString(&mut self, mut s: String) {}
            }

            pub struct fmt {
                pub buf: Buffer,
                pub widPresent: bool,
                pub wid: isize,
                pub minus: bool,
            }

            pub fn RuneCountInString(mut s: String) -> isize {
                0
            }

            impl fmt {
                pub fn writePadding(&mut self, mut width: isize) {}

                pub fn padString(&mut self, mut s: String) {
                    if !self.widPresent || self.wid == 0 {
                        self.buf.writeString((s).clone());
                        return;
                    }
                    let width = self.wid - RuneCountInString((s).clone());
                    if !self.minus {
                        self.writePadding(width);
                        self.buf.writeString((s).clone());
                    } else {
                        self.buf.writeString((s).clone());
                        self.writePadding(width);
                    }
                }
            }
        };

        pass(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            tokens.contains("RuneCountInString") && tokens.contains("writePadding"),
            "expected padString body to remain generic lowering output: {tokens}"
        );
        assert!(
            !tokens.contains("lock () . unwrap"),
            "expected no named padString body replacement: {tokens}"
        );
    }

    #[test]
    fn it_does_not_replace_fmt_string_body_by_name() {
        let mut file: syn::File = syn::parse_quote! {
            pub struct rawfmt;

            impl rawfmt {
                pub fn fmtS(&mut self, mut s: String) {}
                pub fn fmtQ(&mut self, mut s: String) {}
                pub fn fmtSx(&mut self, mut s: String, mut digits: String) {}
            }

            pub struct pp {
                pub fmt: rawfmt,
            }

            impl pp {
                pub fn fmtString(&mut self, mut v: String, mut verb: i32) {
                    if verb == 113 {
                        self.fmt.fmtQ((v).clone());
                    } else if verb == 120 {
                        self.fmt.fmtSx((v).clone(), "0123456789abcdefx".to_string());
                    } else {
                        self.fmt.fmtS((v).clone());
                    }
                }
            }
        };

        pass(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            tokens.contains("fmtQ") && tokens.contains("fmtSx"),
            "expected fmtString branch body to remain generic lowering output: {tokens}"
        );
        assert!(
            !tokens.contains("self . fmt . fmtS (v)"),
            "expected no named fmtString body replacement: {tokens}"
        );
    }

    #[test]
    fn it_does_not_prune_non_fmt_self_value_statements() {
        let mut file: syn::File = syn::parse_quote! {
            pub struct Setting {
                pub value: isize,
            }

            impl Setting {
                pub fn Value(&mut self) -> isize {
                    let mut v = self.value;
                    if v > 0 {
                        return v;
                    }
                    v
                }
            }
        };

        pass(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            tokens.contains("let mut v = self . value"),
            "expected ordinary self.value local binding to remain: {tokens}"
        );
    }

    #[test]
    fn it_inserts_flush_for_receivers_with_generated_flush_hook() {
        let mut file: syn::File = syn::parse_quote! {
            pub struct Printer;

            impl Printer {
                pub fn __gors_flush_fmt(&mut self) {}

                pub fn printArg(&mut self, value: isize) {}

                pub fn run(&mut self) {
                    self.printArg(1);
                }
            }
        };

        pass(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            tokens.contains("self . printArg (1) ; self . __gors_flush_fmt ()"),
            "expected generated flush hook after printArg: {tokens}"
        );
    }

    #[test]
    fn it_inserts_flush_after_structural_helpers_are_injected() {
        let mut file: syn::File = syn::parse_quote! {
            pub struct Printer;

            impl Printer {
                pub fn printArg(&mut self, value: isize) {}

                pub fn run(&mut self) {
                    self.printArg(1);
                }
            }

            impl Printer {
                pub fn __gors_flush_fmt(&mut self) {}
            }
        };

        pass_after_structural_helpers(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            tokens.contains("self . printArg (1) ; self . __gors_flush_fmt ()"),
            "expected generated flush hook after structural helpers are injected: {tokens}"
        );
    }

    #[test]
    fn it_does_not_insert_flush_for_receivers_without_generated_flush_hook() {
        let mut file: syn::File = syn::parse_quote! {
            pub struct Printer;

            impl Printer {
                pub fn printArg(&mut self, value: isize) {}

                pub fn run(&mut self) {
                    self.printArg(1);
                }
            }
        };

        pass(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            !tokens.contains("__gors_flush_fmt"),
            "expected no flush without generated flush hook: {tokens}"
        );
    }

    #[test]
    fn it_prunes_self_value_reflection_fallback_from_generated_flush_receivers() {
        let mut file: syn::File = syn::parse_quote! {
            pub struct Printer {
                pub value: isize,
            }

            impl Printer {
                pub fn __gors_flush_fmt(&mut self) {}

                pub fn printValue(&mut self, value: isize) {}

                pub fn run(&mut self) {
                    let mut fallback = self.value;
                    self.printValue(fallback);
                }
            }
        };

        pass(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            !tokens.contains("fallback") && !tokens.contains("self . value"),
            "expected generated self.value reflection fallback to be pruned by receiver metadata: {tokens}"
        );
    }

    #[test]
    fn it_prunes_self_value_reflection_fallback_without_print_call() {
        let mut file: syn::File = syn::parse_quote! {
            pub struct Printer {
                pub arg: Box<dyn std::any::Any>,
                pub value: crate::reflect::Value,
                pub buf: Buffer,
            }

            pub struct Buffer;

            impl Buffer {
                pub fn writeByte(&mut self, value: u8) {}
                pub fn writeString(&mut self, value: String) {}
            }

            pub fn nilAngleString() -> String {
                "nil".to_string()
            }

            impl Printer {
                pub fn badVerb(&mut self) {
                    if !crate::builtin::interface_is_nil(
                        (crate::builtin::clone_any(&self.arg)).as_ref(),
                    ) {
                        self.buf.writeByte(61u8);
                        let __gors_premethod_arg_0 = crate::builtin::clone_any(&self.arg);
                    } else if (self.value).clone().IsValid() {
                        let __gors_premethod_arg_0 = (self.value).clone().Type().String();
                        self.buf.writeString((__gors_premethod_arg_0).clone());
                    } else {
                        self.buf.writeString(nilAngleString());
                    }
                }
            }
        };

        pass(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            !tokens.contains("IsValid") && !tokens.contains("Type"),
            "expected generated self.value reflection fallback to be pruned without a print call: {tokens}"
        );
        assert!(
            tokens.contains("nilAngleString"),
            "expected non-reflection fallback branch to remain: {tokens}"
        );
    }

    #[test]
    fn it_does_not_clone_local_initializers_by_name() {
        let mut file: syn::File = syn::parse_quote! {
            pub struct State {
                pub fmtFlags: isize,
            }

            pub fn use_names(mut value: isize, mut f: isize, mut state: State) -> isize {
                let mut from_value = value;
                let mut from_f = f;
                let mut from_field = state.fmtFlags;
                from_value + from_f + from_field
            }
        };

        pass(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            tokens.contains("let mut from_value = value"),
            "expected local named value not to force cloning: {tokens}"
        );
        assert!(
            tokens.contains("let mut from_f = f"),
            "expected local named f not to force cloning: {tokens}"
        );
        assert!(
            tokens.contains("let mut from_field = state . fmtFlags"),
            "expected field named fmtFlags not to force cloning: {tokens}"
        );
        assert!(
            !tokens.contains("value) . clone") && !tokens.contains("f) . clone"),
            "expected no identifier-name-driven local clones: {tokens}"
        );
    }

    #[test]
    fn it_does_not_coerce_print_value_args_by_name() {
        let mut file: syn::File = syn::parse_quote! {
            pub struct Pair {
                pub Key: isize,
                pub Value: isize,
            }

            pub struct Sink;

            impl Sink {
                pub fn printValue(&mut self, value: isize) {}
            }

            pub fn call(mut sink: Sink, mut pair: Pair) {
                sink.printValue(pair.Key);
            }
        };

        pass(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            tokens.contains("sink . printValue (pair . Key)"),
            "expected method and field names not to force reflect coercion: {tokens}"
        );
        assert!(
            !tokens.contains("reflect :: ValueOf"),
            "expected no method-name-driven reflect coercion: {tokens}"
        );
    }

    #[test]
    fn it_does_not_box_print_arg_err_by_name() {
        let mut file: syn::File = syn::parse_quote! {
            pub struct Sink;

            impl Sink {
                pub fn printArg(&mut self, value: isize) {}
            }

            pub fn call(mut sink: Sink, mut err: isize) {
                sink.printArg(err);
            }
        };

        pass(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            tokens.contains("sink . printArg (err)"),
            "expected local name err not to force boxing: {tokens}"
        );
        assert!(
            !tokens.contains("Box :: new (err)"),
            "expected no method-name-driven err boxing: {tokens}"
        );
    }

    #[test]
    fn it_does_not_replace_print_arg_index_args_by_name() {
        let mut file: syn::File = syn::parse_quote! {
            pub struct Sink;

            impl Sink {
                pub fn printArg(&mut self, value: isize) {}
            }

            pub fn call(mut sink: Sink, mut values: Vec<isize>) {
                sink.printArg(values[0]);
            }
        };

        pass(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            tokens.contains("sink . printArg (values [0])"),
            "expected method name not to force indexed argument replacement: {tokens}"
        );
        assert!(
            !tokens.contains("std :: mem :: replace"),
            "expected no method-name-driven indexed argument replacement: {tokens}"
        );
    }

    #[test]
    fn it_does_not_replace_print_arg_self_arg_by_name() {
        let mut file: syn::File = syn::parse_quote! {
            pub struct Sink {
                arg: isize,
            }

            impl Sink {
                pub fn printArg(&mut self, value: isize) {}

                pub fn call(&mut self) {
                    self.printArg(self.arg);
                }
            }
        };

        pass(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            tokens.contains("let __gors_premethod_arg_0 = self . arg")
                && tokens.contains("self . printArg (__gors_premethod_arg_0)"),
            "expected method and field names not to force empty any replacement: {tokens}"
        );
        assert!(
            !tokens.contains("Box :: new (())"),
            "expected no method-name-driven empty any replacement: {tokens}"
        );
    }

    #[test]
    fn it_hoists_args_that_read_locked_receiver_root() {
        let mut file: syn::File = syn::parse_quote! {
            pub fn call(mut p: P) {
                (|| {
                    (p.lock().unwrap().fmt).init(crate::builtin::GorsPtr::new({
                        let __gors_pointer_field = (p.lock().unwrap().buf).clone();
                        __gors_pointer_field
                    }));
                })();
            }
        };

        pass(&mut file);

        let tokens = quote::quote!(#file).to_string();
        let hoist_pos =
            tokens.find("let __gors_premethod_arg_0 = crate :: builtin :: GorsPtr :: new");
        assert!(
            hoist_pos.is_some(),
            "expected locked receiver argument to be hoisted: {tokens}"
        );
        let hoist_pos = hoist_pos.unwrap_or_default();
        let call_pos =
            tokens.find("(p . lock () . unwrap () . fmt) . init (__gors_premethod_arg_0)");
        assert!(
            call_pos.is_some(),
            "expected method call to use hoisted argument: {tokens}"
        );
        let call_pos = call_pos.unwrap_or_default();
        assert!(
            hoist_pos < call_pos,
            "expected argument to be evaluated before locked receiver call: {tokens}"
        );
    }

    #[test]
    fn it_hoists_condition_args_read_after_mut_borrow() {
        let mut file: syn::File = syn::parse_quote! {
            pub trait Interface {
                fn Len(&mut self) -> isize;
            }

            pub fn down(h: &mut dyn Interface, i: isize, n: isize) -> bool {
                false
            }

            pub fn up(h: &mut dyn Interface, i: isize) {}

            pub fn fix(mut h: &mut dyn Interface, mut i: isize) {
                if !down(&mut *h, i, h.Len()) {
                    up(&mut *h, i);
                }
            }
        };

        pass(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            tokens.contains("let __gors_preborrow_arg_0 = h . Len ()")
                && tokens.contains("down (& mut * h , i , __gors_preborrow_arg_0)"),
            "expected condition argument read after mutable borrow to be hoisted: {tokens}"
        );
    }

    #[test]
    fn it_does_not_rewrite_err_assignment_from_w_by_name() {
        let mut file: syn::File = syn::parse_quote! {
            pub fn call(mut err: isize, mut w: isize) {
                err = w;
            }
        };

        pass(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            tokens.contains("err = w"),
            "expected local names not to force field extraction: {tokens}"
        );
        assert!(
            !tokens.contains("w . lock () . unwrap () . err"),
            "expected no method-name-driven err field extraction: {tokens}"
        );
    }

    #[test]
    fn it_does_not_rewrite_self_arg_assignment_by_name() {
        let mut file: syn::File = syn::parse_quote! {
            pub struct Sink {
                arg: isize,
            }

            impl Sink {
                pub fn save(&mut self, mut arg: isize) {
                    self.arg = arg;
                }
            }
        };

        pass(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            tokens.contains("self . arg = arg"),
            "expected self.arg assignment to remain name independent: {tokens}"
        );
        assert!(
            !tokens.contains("Box :: new (())"),
            "expected no field-name-driven empty any replacement: {tokens}"
        );
    }

    #[test]
    fn it_does_not_clone_self_value_assignment_by_name() {
        let mut file: syn::File = syn::parse_quote! {
            pub struct Sink {
                value: isize,
            }

            impl Sink {
                pub fn save(&mut self, mut value: isize) {
                    self.value = value;
                }
            }
        };

        pass(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            tokens.contains("self . value = value"),
            "expected self.value assignment to remain name independent: {tokens}"
        );
        assert!(
            !tokens.contains("(value) . clone"),
            "expected no field-name-driven value clone: {tokens}"
        );
    }

    #[test]
    fn it_casts_tuple_newtype_self_through_inner_field() {
        let mut file: syn::File = syn::parse_quote! {
            pub struct KeySizeError(pub isize);

            impl KeySizeError {
                pub fn Error(&self) -> String {
                    crate::strconv::Itoa((self as isize))
                }
            }
        };

        pass(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            tokens.contains("self . 0 as isize"),
            "expected tuple newtype receiver casts to use the inner field: {tokens}"
        );
        assert!(
            !tokens.contains("self as isize"),
            "expected borrowed receiver cast to be rewritten: {tokens}"
        );
    }

    #[test]
    fn it_borrows_pointer_cell_static_pointees_for_mut_ref_calls() {
        let mut file: syn::File = syn::parse_quote! {
            pub struct Table {
                pub n: isize,
            }

            pub static Public: std::sync::LazyLock<std::sync::Arc<std::sync::Mutex<Table>>> =
                std::sync::LazyLock::new(|| std::sync::Arc::new(std::sync::Mutex::new(Table { n: 1 })));

            fn check(mut table: &mut Table) -> isize {
                table.n
            }

            pub fn call() -> isize {
                check((*Public).clone())
            }
        };

        pass(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            tokens.contains("check (& mut * ((* Public)) . lock () . unwrap ())"),
            "expected pointer-cell static to be locked for mutable-reference call: {tokens}"
        );
        assert!(
            !tokens.contains("check (& mut (* Public) . clone ())"),
            "expected not to borrow a cloned Arc cell: {tokens}"
        );
    }

    #[test]
    fn it_borrows_pointer_cell_static_pointees_after_package_merge() {
        let mut file: syn::File = syn::parse_quote! {
            pub fn call() -> isize {
                check((*Public).clone())
            }

            fn check(mut table: &mut Table) -> isize {
                table.n
            }

            pub static Public: std::sync::LazyLock<std::sync::Arc<std::sync::Mutex<Table>>> =
                std::sync::LazyLock::new(|| std::sync::Arc::new(std::sync::Mutex::new(Table { n: 1 })));

            pub struct Table {
                pub n: isize,
            }
        };

        pass_after_package_merge(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            tokens.contains("check (& mut * ((* Public)) . lock () . unwrap ())"),
            "expected post-merge pass to lock pointer-cell static: {tokens}"
        );
        assert!(
            !tokens.contains("check (& mut (* Public) . clone ())"),
            "expected post-merge pass not to borrow a cloned Arc cell: {tokens}"
        );
    }

    #[test]
    fn it_borrows_pointer_cell_range_locals_after_package_merge() {
        let mut file: syn::File = syn::parse_quote! {
            pub struct Table {
                pub n: isize,
            }

            pub fn any(mut tables: Vec<std::sync::Arc<std::sync::Mutex<Table>>>) -> bool {
                for (_, mut table) in (tables)
                    .iter()
                    .cloned()
                    .enumerate()
                    .map(|(i, v)| (i as isize, v))
                {
                    if check(&mut table) {
                        return true;
                    }
                }
                false
            }

            fn check(mut table: &mut Table) -> bool {
                table.n > 0
            }
        };

        pass_after_package_merge(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            tokens.contains("check (& mut * table . lock () . unwrap ())"),
            "expected post-merge pass to lock pointer-cell range local: {tokens}"
        );
        assert!(
            !tokens.contains("check (& mut table)"),
            "expected post-merge pass not to pass the pointer cell itself: {tokens}"
        );
    }

    #[test]
    fn it_borrows_pointer_cell_value_params_after_package_merge() {
        let mut file: syn::File = syn::parse_quote! {
            pub struct Table {
                pub n: isize,
            }

            pub fn call(mut table: std::sync::Arc<std::sync::Mutex<Table>>) -> bool {
                check(table)
            }

            fn check(mut table: &mut Table) -> bool {
                table.n > 0
            }
        };

        pass_after_package_merge(&mut file);

        let tokens = quote::quote!(#file).to_string();
        assert!(
            tokens.contains("check (& mut * table . lock () . unwrap ())"),
            "expected post-merge pass to lock pointer-cell value parameter: {tokens}"
        );
        assert!(
            !tokens.contains("check (& mut table)"),
            "expected post-merge pass not to pass the pointer cell itself: {tokens}"
        );
    }
}
