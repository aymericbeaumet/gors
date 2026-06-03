use syn::visit_mut::{self, VisitMut};

#[derive(Default)]
pub(super) struct Metadata {
    fmt_flush_receiver_types: std::collections::HashSet<String>,
    self_value_reflection_receiver_types: std::collections::HashSet<String>,
}

impl Metadata {
    pub(super) fn collect(file: &syn::File) -> Self {
        Self {
            fmt_flush_receiver_types: collect_fmt_flush_receiver_types(file),
            self_value_reflection_receiver_types: collect_self_value_reflection_receiver_types(
                file,
            ),
        }
    }

    fn is_empty(&self) -> bool {
        self.fmt_flush_receiver_types.is_empty()
            && self.self_value_reflection_receiver_types.is_empty()
    }

    pub(super) fn should_flush_after_stmt(
        &self,
        impl_self_types: &[String],
        stmt: &syn::Stmt,
    ) -> bool {
        impl_self_types
            .last()
            .is_some_and(|ty| self.fmt_flush_receiver_types.contains(ty))
            && stmt_needs_fmt_flush(stmt)
    }

    pub(super) fn should_prune_self_value_for_initial_pass(
        &self,
        self_ty: &str,
        block: &syn::Block,
    ) -> bool {
        (self.fmt_flush_receiver_types.contains(self_ty) && should_prune_fmt_self_value(block))
            || (self.self_value_reflection_receiver_types.contains(self_ty)
                && block_has_self_value_reflection_fallback(block))
    }

    fn should_prune_self_value_after_helpers(&self, self_ty: &str, block: &syn::Block) -> bool {
        self.self_value_reflection_receiver_types.contains(self_ty)
            && block_has_self_value_reflection_fallback(block)
    }
}

pub(super) fn pass_after_structural_helpers(file: &mut syn::File) {
    let metadata = Metadata::collect(file);
    if metadata.is_empty() {
        return;
    }
    CoerceStructuralHelpers {
        metadata,
        impl_self_types: Vec::new(),
    }
    .visit_file_mut(file);
}

struct CoerceStructuralHelpers {
    metadata: Metadata,
    impl_self_types: Vec<String>,
}

impl VisitMut for CoerceStructuralHelpers {
    fn visit_item_impl_mut(&mut self, item_impl: &mut syn::ItemImpl) {
        if let Some(self_ty) = super::type_path_ident_name(&item_impl.self_ty) {
            self.impl_self_types.push(self_ty);
            visit_mut::visit_item_impl_mut(self, item_impl);
            self.impl_self_types.pop();
        } else {
            visit_mut::visit_item_impl_mut(self, item_impl);
        }
    }

    fn visit_impl_item_fn_mut(&mut self, func: &mut syn::ImplItemFn) {
        visit_mut::visit_impl_item_fn_mut(self, func);
        let prune_self_value = self.impl_self_types.last().is_some_and(|ty| {
            self.metadata
                .should_prune_self_value_after_helpers(ty, &func.block)
        });
        prune_reflection_fallback(&mut func.block.stmts, prune_self_value);
    }

    fn visit_block_mut(&mut self, block: &mut syn::Block) {
        let old_stmts = std::mem::take(&mut block.stmts);
        let mut new_stmts = Vec::with_capacity(old_stmts.len());

        for mut stmt in old_stmts {
            visit_mut::visit_stmt_mut(self, &mut stmt);
            let needs_flush = self
                .metadata
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
}

fn should_prune_fmt_self_value(block: &syn::Block) -> bool {
    if block_has_self_value_reflection_fallback(block) {
        return true;
    }

    struct Finder {
        found: bool,
    }

    impl syn::visit::Visit<'_> for Finder {
        fn visit_expr_method_call(&mut self, call: &syn::ExprMethodCall) {
            if super::is_self_expr(&call.receiver)
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

fn block_has_self_value_reflection_fallback(block: &syn::Block) -> bool {
    struct Finder {
        found: bool,
    }

    impl syn::visit::Visit<'_> for Finder {
        fn visit_expr_method_call(&mut self, call: &syn::ExprMethodCall) {
            if matches!(call.method.to_string().as_str(), "IsValid" | "Type")
                && expr_mentions_self_field_named(&call.receiver, "value")
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

pub(super) fn prune_reflection_fallback(stmts: &mut Vec<syn::Stmt>, prune_self_value: bool) {
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

fn expr_mentions_self_field_named(expr: &syn::Expr, name: &str) -> bool {
    struct Visitor<'a> {
        name: &'a str,
        found: bool,
    }

    impl syn::visit::Visit<'_> for Visitor<'_> {
        fn visit_expr_field(&mut self, field: &syn::ExprField) {
            if is_self_field_named(field, self.name) {
                self.found = true;
                return;
            }
            syn::visit::visit_expr_field(self, field);
        }
    }

    let mut visitor = Visitor { name, found: false };
    syn::visit::Visit::visit_expr(&mut visitor, expr);
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
        if super::is_self_expr(&call.receiver)
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
    super::is_self_expr(&field.base)
        && member_ident_name(&field.member).is_some_and(|member| member == name)
}

fn collect_fmt_flush_receiver_types(file: &syn::File) -> std::collections::HashSet<String> {
    let mut hook_receivers = std::collections::HashSet::new();
    let mut flushable_receivers = std::collections::HashSet::new();
    for item in &file.items {
        let syn::Item::Impl(item_impl) = item else {
            continue;
        };
        let Some(self_ty) = super::type_path_ident_name(&item_impl.self_ty) else {
            continue;
        };
        if impl_has_method(item_impl, "__gors_flush_fmt") {
            hook_receivers.insert(self_ty.clone());
        }
        if impl_has_method(item_impl, "printArg") || impl_has_method(item_impl, "printValue") {
            flushable_receivers.insert(self_ty);
        }
    }
    hook_receivers
        .intersection(&flushable_receivers)
        .cloned()
        .collect()
}

fn collect_self_value_reflection_receiver_types(
    file: &syn::File,
) -> std::collections::HashSet<String> {
    file.items
        .iter()
        .filter_map(|item| {
            let syn::Item::Impl(item_impl) = item else {
                return None;
            };
            let self_ty = super::type_path_ident_name(&item_impl.self_ty)?;
            impl_has_self_value_reflection_fallback(item_impl).then_some(self_ty)
        })
        .collect()
}

fn impl_has_self_value_reflection_fallback(item_impl: &syn::ItemImpl) -> bool {
    item_impl.items.iter().any(|item| {
        matches!(
            item,
            syn::ImplItem::Fn(func) if block_has_self_value_reflection_fallback(&func.block)
        )
    })
}

fn impl_has_method(item_impl: &syn::ItemImpl, name: &str) -> bool {
    item_impl
        .items
        .iter()
        .any(|item| matches!(item, syn::ImplItem::Fn(func) if func.sig.ident == name))
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
    super::is_self_expr(&call.receiver)
}
