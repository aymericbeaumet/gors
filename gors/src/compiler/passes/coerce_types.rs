use syn::{
    Token,
    visit_mut::{self, VisitMut},
};

pub fn pass(file: &mut syn::File) {
    let tuple_newtypes = collect_tuple_newtypes(file);
    let mutable_ref_call_args = collect_mutable_ref_call_args(file);
    let pointer_cell_statics = collect_pointer_cell_statics(file);
    let fmt_flush_receiver_types = collect_fmt_flush_receiver_types(file);
    CoerceTypes {
        mutable_ref_call_args,
        pointer_cell_statics,
        fmt_flush_receiver_types,
        tuple_newtypes,
        ..Default::default()
    }
    .visit_file_mut(file);
}

pub fn pass_after_package_merge(file: &mut syn::File) {
    let mutable_ref_call_args = collect_mutable_ref_call_args(file);
    let pointer_cell_statics = collect_pointer_cell_statics(file);
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

#[derive(Default)]
struct CoerceTypes {
    mutable_ref_params: Vec<std::collections::HashSet<String>>,
    generic_value_params: Vec<std::collections::HashSet<String>>,
    has_generic_params: Vec<bool>,
    mutable_ref_call_args: std::collections::HashMap<String, std::collections::HashSet<usize>>,
    pointer_cell_statics: std::collections::HashSet<String>,
    fmt_flush_receiver_types: std::collections::HashSet<String>,
    tuple_newtypes: std::collections::HashSet<String>,
    impl_self_types: Vec<String>,
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

        prune_static_false_branches(&mut func.block.stmts);
        prune_print_arg_reflection_fallback(&mut func.block.stmts, false);
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

        prune_static_false_branches(&mut func.block.stmts);
        let prune_self_value = self.impl_self_types.last().is_some_and(|ty| ty == "pp")
            && should_prune_fmt_self_value(&func.block);
        prune_print_arg_reflection_fallback(&mut func.block.stmts, prune_self_value);
    }

    fn visit_block_mut(&mut self, block: &mut syn::Block) {
        let old_stmts = std::mem::take(&mut block.stmts);
        let mut new_stmts = Vec::with_capacity(old_stmts.len());

        for mut stmt in old_stmts {
            visit_mut::visit_stmt_mut(self, &mut stmt);
            new_stmts.extend(hoist_args_read_after_mut_borrow(&mut stmt));
            new_stmts.extend(hoist_condition_args_read_after_mut_borrow(&mut stmt));
            new_stmts.extend(hoist_method_args_read_receiver(&mut stmt));
            let needs_flush = self
                .impl_self_types
                .last()
                .is_some_and(|ty| self.fmt_flush_receiver_types.contains(ty))
                && stmt_needs_fmt_flush(&stmt);
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

fn should_prune_fmt_self_value(block: &syn::Block) -> bool {
    struct Finder {
        found: bool,
    }

    impl syn::visit::Visit<'_> for Finder {
        fn visit_expr_method_call(&mut self, call: &syn::ExprMethodCall) {
            if is_self_expr(&call.receiver)
                && matches!(
                    call.method.to_string().as_str(),
                    "printArg" | "printValue" | "fmtPointer"
                )
            {
                self.found = true;
                return;
            }
            syn::visit::visit_expr_method_call(self, call);
        }
    }

    let mut finder = Finder { found: false };
    syn::visit::Visit::visit_block(&mut finder, block);
    finder.found
}

fn prune_print_arg_reflection_fallback(stmts: &mut Vec<syn::Stmt>, prune_self_value: bool) {
    let old_stmts = std::mem::take(stmts);
    *stmts = old_stmts
        .into_iter()
        .filter_map(|stmt| prune_print_arg_stmt(stmt, prune_self_value))
        .collect();
}

fn prune_print_arg_stmt(stmt: syn::Stmt, prune_self_value: bool) -> Option<syn::Stmt> {
    if print_arg_stmt_needs_reflection(&stmt, prune_self_value) {
        match stmt {
            syn::Stmt::Expr(expr, semi) => {
                prune_print_arg_expr(expr, prune_self_value).map(|expr| syn::Stmt::Expr(expr, semi))
            }
            syn::Stmt::Local(mut local) => {
                if let Some(init) = &mut local.init {
                    let expr = std::mem::replace(
                        &mut init.expr,
                        Box::new(syn::parse_quote! { Default::default() }),
                    );
                    let expr = prune_print_arg_expr(*expr, prune_self_value)?;
                    *init.expr = expr;
                }
                Some(syn::Stmt::Local(local))
            }
            syn::Stmt::Item(_) | syn::Stmt::Macro(_) => None,
        }
    } else {
        Some(stmt)
    }
}

fn prune_print_arg_expr(expr: syn::Expr, prune_self_value: bool) -> Option<syn::Expr> {
    match expr {
        syn::Expr::Block(expr_block) => prune_print_arg_expr_block(expr_block, prune_self_value),
        syn::Expr::If(expr_if) => prune_print_arg_if(expr_if, prune_self_value),
        other if print_arg_expr_needs_reflection(&other, prune_self_value) => None,
        other => Some(other),
    }
}

fn prune_print_arg_expr_block(
    mut expr_block: syn::ExprBlock,
    prune_self_value: bool,
) -> Option<syn::Expr> {
    expr_block.block = prune_print_arg_block(expr_block.block, prune_self_value);
    (!expr_block.block.stmts.is_empty()).then_some(syn::Expr::Block(expr_block))
}

fn prune_print_arg_if(mut expr_if: syn::ExprIf, prune_self_value: bool) -> Option<syn::Expr> {
    if print_arg_expr_needs_reflection(&expr_if.cond, prune_self_value) {
        return expr_if
            .else_branch
            .and_then(|(_, else_expr)| prune_print_arg_expr(*else_expr, prune_self_value));
    }

    let then_had_reflection =
        print_arg_block_needs_reflection(&expr_if.then_branch, prune_self_value);
    expr_if.then_branch = prune_print_arg_block(expr_if.then_branch, prune_self_value);
    let then_is_empty = expr_if.then_branch.stmts.is_empty();

    expr_if.else_branch = expr_if.else_branch.and_then(|(else_token, else_expr)| {
        prune_print_arg_expr(*else_expr, prune_self_value).map(|expr| (else_token, Box::new(expr)))
    });

    if then_had_reflection && then_is_empty {
        return expr_if.else_branch.map(|(_, else_expr)| *else_expr);
    }

    Some(syn::Expr::If(expr_if))
}

fn prune_print_arg_block(mut block: syn::Block, prune_self_value: bool) -> syn::Block {
    let mut dropped_names = std::collections::HashSet::new();
    let mut stmts = vec![];
    for stmt in block.stmts {
        let bound_names = stmt_bound_names(&stmt);
        if stmt_mentions_any_name(&stmt, &dropped_names) {
            dropped_names.extend(bound_names);
            continue;
        }
        if let Some(stmt) = prune_print_arg_stmt(stmt, prune_self_value) {
            stmts.push(stmt);
        } else {
            dropped_names.extend(bound_names);
        }
    }
    block.stmts = stmts;
    block
}

fn prune_static_false_branches(stmts: &mut Vec<syn::Stmt>) {
    let mut false_names = std::collections::HashSet::new();
    prune_static_false_branches_with(stmts, &mut false_names);
}

fn prune_static_false_branches_with(
    stmts: &mut Vec<syn::Stmt>,
    false_names: &mut std::collections::HashSet<String>,
) {
    let old_stmts = std::mem::take(stmts);
    *stmts = old_stmts
        .into_iter()
        .filter_map(|stmt| prune_static_false_stmt(stmt, false_names))
        .collect();
}

fn prune_static_false_stmt(
    stmt: syn::Stmt,
    false_names: &mut std::collections::HashSet<String>,
) -> Option<syn::Stmt> {
    match stmt {
        syn::Stmt::Local(local) => {
            collect_false_local_names(&local, false_names);
            Some(syn::Stmt::Local(local))
        }
        syn::Stmt::Expr(expr, semi) => {
            prune_static_false_expr(expr, false_names).map(|expr| syn::Stmt::Expr(expr, semi))
        }
        syn::Stmt::Item(_) | syn::Stmt::Macro(_) => Some(stmt),
    }
}

fn prune_static_false_expr(
    expr: syn::Expr,
    false_names: &mut std::collections::HashSet<String>,
) -> Option<syn::Expr> {
    match expr {
        syn::Expr::Block(mut expr_block) => {
            let mut scoped_false_names = false_names.clone();
            prune_static_false_branches_with(&mut expr_block.block.stmts, &mut scoped_false_names);
            Some(syn::Expr::Block(expr_block))
        }
        syn::Expr::If(mut expr_if) => {
            if condition_is_static_false(&expr_if.cond, false_names) {
                return expr_if
                    .else_branch
                    .and_then(|(_, else_expr)| prune_static_false_expr(*else_expr, false_names));
            }
            let mut then_false_names = false_names.clone();
            prune_static_false_branches_with(&mut expr_if.then_branch.stmts, &mut then_false_names);
            expr_if.else_branch = expr_if.else_branch.and_then(|(else_token, else_expr)| {
                prune_static_false_expr(*else_expr, false_names)
                    .map(|expr| (else_token, Box::new(expr)))
            });
            Some(syn::Expr::If(expr_if))
        }
        other => Some(other),
    }
}

fn condition_is_static_false(
    expr: &syn::Expr,
    false_names: &std::collections::HashSet<String>,
) -> bool {
    if is_false_lit_expr(expr) {
        return true;
    }
    path_ident_name(expr).is_some_and(|name| false_names.contains(&name))
}

fn collect_false_local_names(
    local: &syn::Local,
    false_names: &mut std::collections::HashSet<String>,
) {
    let Some(init) = &local.init else {
        return;
    };
    collect_false_bindings(&local.pat, &init.expr, false_names);
}

fn collect_false_bindings(
    pat: &syn::Pat,
    expr: &syn::Expr,
    false_names: &mut std::collections::HashSet<String>,
) {
    match (pat, expr) {
        (syn::Pat::Ident(pat_ident), expr) if is_false_lit_expr(expr) => {
            false_names.insert(pat_ident.ident.to_string());
        }
        (syn::Pat::Tuple(pat_tuple), syn::Expr::Tuple(expr_tuple)) => {
            for (pat, expr) in pat_tuple.elems.iter().zip(&expr_tuple.elems) {
                collect_false_bindings(pat, expr, false_names);
            }
        }
        (syn::Pat::Type(pat_type), expr) => {
            collect_false_bindings(&pat_type.pat, expr, false_names);
        }
        _ => {}
    }
}

fn stmt_bound_names(stmt: &syn::Stmt) -> std::collections::HashSet<String> {
    let mut names = std::collections::HashSet::new();
    if let syn::Stmt::Local(local) = stmt {
        collect_pat_names(&local.pat, &mut names);
    }
    names
}

fn collect_pat_names(pat: &syn::Pat, names: &mut std::collections::HashSet<String>) {
    match pat {
        syn::Pat::Ident(pat_ident) => {
            names.insert(pat_ident.ident.to_string());
        }
        syn::Pat::Tuple(tuple) => {
            for elem in &tuple.elems {
                collect_pat_names(elem, names);
            }
        }
        syn::Pat::Type(pat_type) => collect_pat_names(&pat_type.pat, names),
        _ => {}
    }
}

fn stmt_mentions_any_name(stmt: &syn::Stmt, names: &std::collections::HashSet<String>) -> bool {
    if names.is_empty() {
        return false;
    }

    struct Visitor<'a> {
        names: &'a std::collections::HashSet<String>,
        found: bool,
    }

    impl syn::visit::Visit<'_> for Visitor<'_> {
        fn visit_expr_path(&mut self, expr_path: &syn::ExprPath) {
            if expr_path.path.leading_colon.is_none()
                && expr_path.path.segments.len() == 1
                && expr_path
                    .path
                    .segments
                    .first()
                    .is_some_and(|seg| self.names.contains(&seg.ident.to_string()))
            {
                self.found = true;
                return;
            }
            syn::visit::visit_expr_path(self, expr_path);
        }
    }

    let mut visitor = Visitor {
        names,
        found: false,
    };
    syn::visit::Visit::visit_stmt(&mut visitor, stmt);
    visitor.found
}

fn print_arg_stmt_needs_reflection(stmt: &syn::Stmt, prune_self_value: bool) -> bool {
    let mut finder = PrintArgReflectionFinder {
        prune_self_value,
        found: false,
    };
    syn::visit::Visit::visit_stmt(&mut finder, stmt);
    finder.found
}

fn print_arg_expr_needs_reflection(expr: &syn::Expr, prune_self_value: bool) -> bool {
    let mut finder = PrintArgReflectionFinder {
        prune_self_value,
        found: false,
    };
    syn::visit::Visit::visit_expr(&mut finder, expr);
    finder.found
}

fn print_arg_block_needs_reflection(block: &syn::Block, prune_self_value: bool) -> bool {
    let mut finder = PrintArgReflectionFinder {
        prune_self_value,
        found: false,
    };
    syn::visit::Visit::visit_block(&mut finder, block);
    finder.found
}

struct PrintArgReflectionFinder {
    prune_self_value: bool,
    found: bool,
}

impl syn::visit::Visit<'_> for PrintArgReflectionFinder {
    fn visit_expr_path(&mut self, path: &syn::ExprPath) {
        if path
            .path
            .segments
            .iter()
            .any(|segment| segment.ident == "reflect")
        {
            self.found = true;
            return;
        }
        syn::visit::visit_expr_path(self, path);
    }

    fn visit_type_path(&mut self, path: &syn::TypePath) {
        if path
            .path
            .segments
            .iter()
            .any(|segment| segment.ident == "reflect")
        {
            self.found = true;
            return;
        }
        syn::visit::visit_type_path(self, path);
    }

    fn visit_expr_method_call(&mut self, call: &syn::ExprMethodCall) {
        if is_self_expr(&call.receiver)
            && matches!(
                call.method.to_string().as_str(),
                "printValue" | "fmtPointer"
            )
        {
            self.found = true;
            return;
        }
        syn::visit::visit_expr_method_call(self, call);
    }

    fn visit_expr_field(&mut self, field: &syn::ExprField) {
        if self.prune_self_value && is_self_field_named(field, "value") {
            self.found = true;
            return;
        }
        syn::visit::visit_expr_field(self, field);
    }
}

fn member_ident_name(member: &syn::Member) -> Option<&syn::Ident> {
    match member {
        syn::Member::Named(ident) => Some(ident),
        syn::Member::Unnamed(_) => None,
    }
}

fn is_self_field_named(field: &syn::ExprField, name: &str) -> bool {
    is_self_expr(&field.base)
        && member_ident_name(&field.member).is_some_and(|member| member == name)
}

fn is_false_lit_expr(expr: &syn::Expr) -> bool {
    matches!(
        expr,
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Bool(lit),
            ..
        }) if !lit.value
    )
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

fn collect_fmt_flush_receiver_types(file: &syn::File) -> std::collections::HashSet<String> {
    file.items
        .iter()
        .filter_map(|item| {
            let syn::Item::Impl(item_impl) = item else {
                return None;
            };
            let self_ty = type_path_ident_name(&item_impl.self_ty)?;
            let has_flush_hook = impl_has_method(item_impl, "__gors_flush_fmt");
            let has_flushable_method =
                impl_has_method(item_impl, "printArg") || impl_has_method(item_impl, "printValue");
            (has_flush_hook && has_flushable_method).then_some(self_ty)
        })
        .collect()
}

fn impl_has_method(item_impl: &syn::ItemImpl, name: &str) -> bool {
    item_impl
        .items
        .iter()
        .any(|item| matches!(item, syn::ImplItem::Fn(func) if func.sig.ident == name))
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

struct PointerCellArgScope {
    values: std::collections::HashSet<String>,
    iterables: std::collections::HashSet<String>,
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

fn pointer_cell_arg_scope(sig: &syn::Signature) -> PointerCellArgScope {
    let mut values = std::collections::HashSet::new();
    let mut iterables = std::collections::HashSet::new();

    for input in &sig.inputs {
        let syn::FnArg::Typed(pat_type) = input else {
            continue;
        };
        let Some(name) = pat_ident_name(&pat_type.pat) else {
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

fn type_is_pointer_cell(ty: &syn::Type) -> bool {
    let Some(inner) = first_type_arg_if_path_last_ident(ty, "Arc") else {
        return false;
    };
    first_type_arg_if_path_last_ident(inner, "Mutex").is_some()
}

fn type_is_pointer_cell_iterable(ty: &syn::Type) -> bool {
    match ty {
        syn::Type::Reference(reference) => type_is_pointer_cell_iterable(&reference.elem),
        syn::Type::Slice(slice) => type_is_pointer_cell(&slice.elem),
        _ => first_type_arg_if_path_last_ident(ty, "Vec").is_some_and(type_is_pointer_cell),
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
    if iter_names.is_empty() || !expr_contains_any_path_ident(iter, &iter_names) {
        return std::collections::HashSet::new();
    }
    let skip_first = expr_contains_method_call(iter, "enumerate");
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

fn coerce_pointer_cell_call_args(
    func: &syn::Expr,
    args: &mut syn::punctuated::Punctuated<syn::Expr, Token![,]>,
    mutable_ref_call_args: &std::collections::HashMap<String, std::collections::HashSet<usize>>,
    pointer_cell_statics: &std::collections::HashSet<String>,
    pointer_cell_name_scopes: &[std::collections::HashSet<String>],
) {
    let Some(name) = call_func_name(func) else {
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

fn stmt_needs_fmt_flush(stmt: &syn::Stmt) -> bool {
    matches!(stmt, syn::Stmt::Expr(expr, _) if expr_needs_fmt_flush(expr))
}

fn expr_needs_fmt_flush(expr: &syn::Expr) -> bool {
    let syn::Expr::MethodCall(call) = expr else {
        return false;
    };
    if !matches!(call.method.to_string().as_str(), "printArg" | "printValue") {
        return false;
    }
    is_self_expr(&call.receiver)
}

fn hoist_args_read_after_mut_borrow(stmt: &mut syn::Stmt) -> Vec<syn::Stmt> {
    let syn::Stmt::Expr(syn::Expr::Call(call), _) = stmt else {
        return Vec::new();
    };

    let mut hoisted = Vec::new();
    hoist_args_read_after_mut_borrow_in_args(&mut call.args, &mut hoisted);
    hoisted
}

fn hoist_condition_args_read_after_mut_borrow(stmt: &mut syn::Stmt) -> Vec<syn::Stmt> {
    match stmt {
        syn::Stmt::Expr(syn::Expr::If(if_expr), _) => {
            hoist_args_read_after_mut_borrow_in_expr(&mut if_expr.cond)
        }
        syn::Stmt::Expr(syn::Expr::While(while_expr), _) => {
            hoist_args_read_after_mut_borrow_in_expr(&mut while_expr.cond)
        }
        _ => Vec::new(),
    }
}

fn hoist_args_read_after_mut_borrow_in_expr(expr: &mut syn::Expr) -> Vec<syn::Stmt> {
    struct Hoister {
        hoisted: Vec<syn::Stmt>,
    }

    impl VisitMut for Hoister {
        fn visit_expr_call_mut(&mut self, call: &mut syn::ExprCall) {
            visit_mut::visit_expr_call_mut(self, call);
            hoist_args_read_after_mut_borrow_in_args(&mut call.args, &mut self.hoisted);
        }
    }

    let mut hoister = Hoister {
        hoisted: Vec::new(),
    };
    hoister.visit_expr_mut(expr);
    hoister.hoisted
}

fn hoist_args_read_after_mut_borrow_in_args(
    args: &mut syn::punctuated::Punctuated<syn::Expr, syn::token::Comma>,
    hoisted: &mut Vec<syn::Stmt>,
) {
    let borrowed: Vec<(usize, String)> = args
        .iter()
        .enumerate()
        .filter_map(|(index, arg)| mut_borrowed_path_name(arg).map(|name| (index, name)))
        .collect();
    if borrowed.is_empty() {
        return;
    }

    for (borrow_index, name) in borrowed {
        for (arg_index, arg) in args.iter_mut().enumerate() {
            if arg_index <= borrow_index || !expr_contains_path_ident(arg, &name) {
                continue;
            }
            let temp = quote::format_ident!("__gors_preborrow_arg_{}", hoisted.len());
            let value = arg.clone();
            *arg = syn::parse_quote! { #temp };
            hoisted.push(syn::parse_quote! {
                let #temp = #value;
            });
        }
    }
}

fn hoist_method_args_read_receiver(stmt: &mut syn::Stmt) -> Vec<syn::Stmt> {
    let syn::Stmt::Expr(syn::Expr::MethodCall(call), _) = stmt else {
        return Vec::new();
    };
    let Some(receiver_name) = receiver_root_ident_name(&call.receiver) else {
        return Vec::new();
    };

    let mut hoisted = Vec::new();
    for arg in &mut call.args {
        if !expr_contains_path_ident(arg, &receiver_name) {
            continue;
        }
        let temp = quote::format_ident!("__gors_premethod_arg_{}", hoisted.len());
        let value = arg.clone();
        *arg = syn::parse_quote! { #temp };
        hoisted.push(syn::parse_quote! {
            let #temp = #value;
        });
    }
    hoisted
}

fn receiver_root_ident_name(expr: &syn::Expr) -> Option<String> {
    match expr {
        syn::Expr::Path(_) => path_ident_name(expr),
        syn::Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Deref(_)) => {
            receiver_root_ident_name(&unary.expr)
        }
        syn::Expr::Paren(paren) => receiver_root_ident_name(&paren.expr),
        syn::Expr::Group(group) => receiver_root_ident_name(&group.expr),
        syn::Expr::Field(field) => receiver_root_ident_name(&field.base),
        syn::Expr::Reference(reference) => receiver_root_ident_name(&reference.expr),
        syn::Expr::MethodCall(method)
            if method.args.is_empty() && is_transparent_receiver_method(&method.method) =>
        {
            receiver_root_ident_name(&method.receiver)
        }
        _ => None,
    }
}

fn is_transparent_receiver_method(method: &syn::Ident) -> bool {
    matches!(
        method.to_string().as_str(),
        "as_mut" | "as_ref" | "clone" | "lock" | "unwrap"
    )
}

fn mut_borrowed_path_name(expr: &syn::Expr) -> Option<String> {
    let syn::Expr::Reference(reference) = expr else {
        return None;
    };
    reference.mutability.as_ref()?;
    path_ident_name(&reference.expr)
}

fn expr_contains_path_ident(expr: &syn::Expr, name: &str) -> bool {
    struct Finder<'a> {
        name: &'a str,
        found: bool,
    }

    impl syn::visit::Visit<'_> for Finder<'_> {
        fn visit_expr_path(&mut self, path: &syn::ExprPath) {
            if path.path.leading_colon.is_none()
                && path.path.segments.len() == 1
                && path
                    .path
                    .segments
                    .first()
                    .is_some_and(|segment| segment.ident == self.name)
            {
                self.found = true;
            }
            syn::visit::visit_expr_path(self, path);
        }
    }

    let mut finder = Finder { name, found: false };
    syn::visit::Visit::visit_expr(&mut finder, expr);
    finder.found
}

fn expr_contains_any_path_ident(
    expr: &syn::Expr,
    names: &std::collections::HashSet<String>,
) -> bool {
    names
        .iter()
        .any(|name| expr_contains_path_ident(expr, name))
}

fn expr_contains_method_call(expr: &syn::Expr, method: &str) -> bool {
    struct Finder<'a> {
        method: &'a str,
        found: bool,
    }

    impl syn::visit::Visit<'_> for Finder<'_> {
        fn visit_expr_method_call(&mut self, call: &syn::ExprMethodCall) {
            if call.method == self.method {
                self.found = true;
            }
            syn::visit::visit_expr_method_call(self, call);
        }
    }

    let mut finder = Finder {
        method,
        found: false,
    };
    syn::visit::Visit::visit_expr(&mut finder, expr);
    finder.found
}

fn borrow_mut_expr(expr: &mut syn::Expr, pointer_cell_statics: &std::collections::HashSet<String>) {
    if matches!(expr, syn::Expr::Reference(_)) {
        return;
    }
    if is_path_ident(expr, "self") {
        return;
    }
    if borrow_pointer_cell_static_expr(expr, pointer_cell_statics) {
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

fn borrow_pointer_cell_static_expr(
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
    if borrow_pointer_cell_static_expr(expr, pointer_cell_statics) {
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
        .or_else(|| path_ident_name(expr))
        .or_else(|| strip_clone_method_call(expr).and_then(|expr| path_ident_name(&expr)))
}

fn mut_reference_path_name(expr: &syn::Expr) -> Option<String> {
    let expr = strip_paren_or_group(expr);
    let syn::Expr::Reference(reference) = expr else {
        return None;
    };
    reference.mutability.as_ref()?;
    path_ident_name(&reference.expr)
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
    path_ident_name(strip_paren_or_group(&unary.expr))
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

        prune_print_arg_reflection_fallback(&mut stmts, false);

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

        prune_print_arg_reflection_fallback(&mut stmts, false);

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

        prune_print_arg_reflection_fallback(&mut stmts, false);

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
