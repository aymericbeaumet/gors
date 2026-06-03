use syn::visit_mut::{self, VisitMut};

type NameSet = std::collections::HashSet<String>;
type ReceiverNameMap = std::collections::BTreeMap<String, NameSet>;

#[derive(Default)]
pub(super) struct Metadata {
    fmt_flush: FmtFlushMetadata,
    reflection_fallback: ReflectionFallbackMetadata,
}

impl Metadata {
    pub(super) fn collect(file: &syn::File) -> Self {
        Self {
            fmt_flush: FmtFlushMetadata::collect(file),
            reflection_fallback: ReflectionFallbackMetadata::collect(file),
        }
    }

    fn is_empty(&self) -> bool {
        self.fmt_flush.is_empty() && self.reflection_fallback.is_empty()
    }

    pub(super) fn should_flush_after_stmt(
        &self,
        impl_self_types: &[String],
        stmt: &syn::Stmt,
    ) -> bool {
        self.fmt_flush
            .should_flush_after_stmt(impl_self_types, stmt)
    }

    pub(super) fn push_stmt_with_flush(
        &self,
        impl_self_types: &[String],
        stmt: syn::Stmt,
        stmts: &mut Vec<syn::Stmt>,
    ) {
        let needs_flush = self.should_flush_after_stmt(impl_self_types, &stmt);
        stmts.push(stmt);
        if needs_flush {
            stmts.push(syn::parse_quote! {
                self.__gors_flush_fmt();
            });
        }
    }

    pub(super) fn self_reflect_fields_for_initial_pass(
        &self,
        self_ty: &str,
        block: &syn::Block,
    ) -> Option<&NameSet> {
        let fields = self.reflection_fallback.fields_for_receiver(self_ty)?;
        (block_has_self_reflect_field_runtime_fallback(block, fields)
            || (self.fmt_flush.has_receiver(self_ty)
                && block_mentions_self_reflect_field(block, fields)))
        .then_some(fields)
    }

    fn self_reflect_fields_after_helpers(
        &self,
        self_ty: &str,
        block: &syn::Block,
    ) -> Option<&NameSet> {
        let fields = self.reflection_fallback.fields_for_receiver(self_ty)?;
        block_has_self_reflect_field_runtime_fallback(block, fields).then_some(fields)
    }
}

#[derive(Default)]
struct FmtFlushMetadata {
    methods_by_receiver: ReceiverNameMap,
}

impl FmtFlushMetadata {
    fn collect(file: &syn::File) -> Self {
        Self {
            methods_by_receiver: collect_fmt_flush_methods_by_receiver(file),
        }
    }

    fn is_empty(&self) -> bool {
        self.methods_by_receiver.is_empty()
    }

    fn has_receiver(&self, self_ty: &str) -> bool {
        self.methods_by_receiver.contains_key(self_ty)
    }

    fn should_flush_after_stmt(&self, impl_self_types: &[String], stmt: &syn::Stmt) -> bool {
        let Some(methods) = impl_self_types
            .last()
            .and_then(|ty| self.methods_by_receiver.get(ty))
        else {
            return false;
        };
        stmt_needs_fmt_flush(stmt, methods)
    }
}

#[derive(Default)]
struct ReflectionFallbackMetadata {
    self_reflect_value_fields: ReceiverNameMap,
}

impl ReflectionFallbackMetadata {
    fn collect(file: &syn::File) -> Self {
        Self {
            self_reflect_value_fields: collect_self_reflect_value_fields(file),
        }
    }

    fn is_empty(&self) -> bool {
        self.self_reflect_value_fields.is_empty()
    }

    fn fields_for_receiver(&self, self_ty: &str) -> Option<&NameSet> {
        self.self_reflect_value_fields.get(self_ty)
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
        if let Some(self_ty) = super::syntax::type_path_ident_name(&item_impl.self_ty) {
            self.impl_self_types.push(self_ty);
            visit_mut::visit_item_impl_mut(self, item_impl);
            self.impl_self_types.pop();
        } else {
            visit_mut::visit_item_impl_mut(self, item_impl);
        }
    }

    fn visit_impl_item_fn_mut(&mut self, func: &mut syn::ImplItemFn) {
        visit_mut::visit_impl_item_fn_mut(self, func);
        let self_reflect_fields = self.impl_self_types.last().and_then(|ty| {
            self.metadata
                .self_reflect_fields_after_helpers(ty, &func.block)
        });
        prune_reflection_fallback(&mut func.block.stmts, self_reflect_fields);
    }

    fn visit_block_mut(&mut self, block: &mut syn::Block) {
        let old_stmts = std::mem::take(&mut block.stmts);
        let mut new_stmts = Vec::with_capacity(old_stmts.len());

        for mut stmt in old_stmts {
            visit_mut::visit_stmt_mut(self, &mut stmt);
            self.metadata
                .push_stmt_with_flush(&self.impl_self_types, stmt, &mut new_stmts);
        }

        block.stmts = new_stmts;
    }
}

fn block_mentions_self_reflect_field(block: &syn::Block, fields: &NameSet) -> bool {
    SelfFieldFinder::block_contains(block, fields)
}

fn block_has_self_reflect_field_runtime_fallback(block: &syn::Block, fields: &NameSet) -> bool {
    struct Finder<'a> {
        fields: &'a NameSet,
        found: bool,
    }

    impl syn::visit::Visit<'_> for Finder<'_> {
        fn visit_expr_method_call(&mut self, call: &syn::ExprMethodCall) {
            if matches!(call.method.to_string().as_str(), "IsValid" | "Type")
                && expr_mentions_self_field_in(&call.receiver, self.fields)
            {
                self.found = true;
                return;
            }
            syn::visit::visit_expr_method_call(self, call);
        }
    }

    let mut finder = Finder {
        fields,
        found: false,
    };
    syn::visit::Visit::visit_block(&mut finder, block);
    finder.found
}

pub(super) fn prune_reflection_fallback(
    stmts: &mut Vec<syn::Stmt>,
    self_reflect_fields: Option<&NameSet>,
) {
    let old_stmts = std::mem::take(stmts);
    let block = prune_reflection_block(
        syn::Block {
            brace_token: syn::token::Brace::default(),
            stmts: old_stmts,
        },
        self_reflect_fields,
    );
    *stmts = block.stmts;
}

fn prune_reflection_stmt(
    stmt: syn::Stmt,
    self_reflect_fields: Option<&NameSet>,
) -> Option<syn::Stmt> {
    if stmt_needs_reflection(&stmt, self_reflect_fields) {
        match stmt {
            syn::Stmt::Expr(expr, semi) => prune_reflection_expr(expr, self_reflect_fields)
                .map(|expr| syn::Stmt::Expr(expr, semi)),
            syn::Stmt::Local(mut local) => {
                if let Some(init) = &mut local.init {
                    let expr = std::mem::replace(
                        &mut init.expr,
                        Box::new(syn::parse_quote! { Default::default() }),
                    );
                    let expr = prune_reflection_expr(*expr, self_reflect_fields)?;
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

fn prune_reflection_expr(
    expr: syn::Expr,
    self_reflect_fields: Option<&NameSet>,
) -> Option<syn::Expr> {
    match expr {
        syn::Expr::Block(expr_block) => {
            prune_reflection_expr_block(expr_block, self_reflect_fields)
        }
        syn::Expr::If(expr_if) => prune_reflection_if(expr_if, self_reflect_fields),
        other if expr_needs_reflection(&other, self_reflect_fields) => None,
        other => Some(other),
    }
}

fn prune_reflection_expr_block(
    mut expr_block: syn::ExprBlock,
    self_reflect_fields: Option<&NameSet>,
) -> Option<syn::Expr> {
    expr_block.block = prune_reflection_block(expr_block.block, self_reflect_fields);
    (!expr_block.block.stmts.is_empty()).then_some(syn::Expr::Block(expr_block))
}

fn prune_reflection_if(
    mut expr_if: syn::ExprIf,
    self_reflect_fields: Option<&NameSet>,
) -> Option<syn::Expr> {
    if expr_needs_reflection(&expr_if.cond, self_reflect_fields) {
        return expr_if
            .else_branch
            .and_then(|(_, else_expr)| prune_reflection_expr(*else_expr, self_reflect_fields));
    }

    let then_had_reflection = block_needs_reflection(&expr_if.then_branch, self_reflect_fields);
    expr_if.then_branch = prune_reflection_block(expr_if.then_branch, self_reflect_fields);
    let then_is_empty = expr_if.then_branch.stmts.is_empty();

    expr_if.else_branch = expr_if.else_branch.and_then(|(else_token, else_expr)| {
        prune_reflection_expr(*else_expr, self_reflect_fields)
            .map(|expr| (else_token, Box::new(expr)))
    });

    if then_had_reflection && then_is_empty {
        return expr_if.else_branch.map(|(_, else_expr)| *else_expr);
    }

    Some(syn::Expr::If(expr_if))
}

fn prune_reflection_block(
    mut block: syn::Block,
    self_reflect_fields: Option<&NameSet>,
) -> syn::Block {
    let mut dropped_names = std::collections::HashSet::new();
    let mut stmts = vec![];
    for stmt in block.stmts {
        let bound_names = stmt_bound_names(&stmt);
        if stmt_mentions_any_name(&stmt, &dropped_names) {
            dropped_names.extend(bound_names);
            continue;
        }
        if let Some(stmt) = prune_reflection_stmt(stmt, self_reflect_fields) {
            stmts.push(stmt);
        } else {
            dropped_names.extend(bound_names);
        }
    }
    block.stmts = stmts;
    block
}

fn stmt_bound_names(stmt: &syn::Stmt) -> NameSet {
    let mut names = NameSet::new();
    if let syn::Stmt::Local(local) = stmt {
        collect_pat_names(&local.pat, &mut names);
    }
    names
}

fn collect_pat_names(pat: &syn::Pat, names: &mut NameSet) {
    match pat {
        syn::Pat::Ident(pat_ident) => {
            names.insert(pat_ident.ident.to_string());
        }
        syn::Pat::Or(pat_or) => {
            for case in &pat_or.cases {
                collect_pat_names(case, names);
            }
        }
        syn::Pat::Paren(paren) => collect_pat_names(&paren.pat, names),
        syn::Pat::Reference(reference) => collect_pat_names(&reference.pat, names),
        syn::Pat::Rest(_) => {}
        syn::Pat::Slice(slice) => {
            for elem in &slice.elems {
                collect_pat_names(elem, names);
            }
        }
        syn::Pat::Struct(pat_struct) => {
            for field in &pat_struct.fields {
                collect_pat_names(&field.pat, names);
            }
        }
        syn::Pat::Tuple(tuple) => {
            for elem in &tuple.elems {
                collect_pat_names(elem, names);
            }
        }
        syn::Pat::TupleStruct(tuple_struct) => {
            for elem in &tuple_struct.elems {
                collect_pat_names(elem, names);
            }
        }
        syn::Pat::Type(pat_type) => collect_pat_names(&pat_type.pat, names),
        syn::Pat::Wild(_) => {}
        _ => {}
    }
}

fn stmt_mentions_any_name(stmt: &syn::Stmt, names: &NameSet) -> bool {
    if names.is_empty() {
        return false;
    }
    match stmt {
        syn::Stmt::Expr(expr, _) => expr_mentions_any_name(expr, names),
        syn::Stmt::Local(local) => local
            .init
            .as_ref()
            .is_some_and(|init| expr_mentions_any_name(&init.expr, names)),
        syn::Stmt::Item(_) | syn::Stmt::Macro(_) => false,
    }
}

fn expr_mentions_any_name(expr: &syn::Expr, names: &NameSet) -> bool {
    if names.is_empty() {
        return false;
    }

    struct Visitor<'a> {
        names: &'a NameSet,
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
    syn::visit::Visit::visit_expr(&mut visitor, expr);
    visitor.found
}

fn expr_mentions_self_field_in(expr: &syn::Expr, fields: &NameSet) -> bool {
    SelfFieldFinder::expr_contains(expr, fields)
}

struct SelfFieldFinder<'a> {
    fields: &'a NameSet,
    found: bool,
}

impl SelfFieldFinder<'_> {
    fn block_contains(block: &syn::Block, fields: &NameSet) -> bool {
        let mut finder = SelfFieldFinder {
            fields,
            found: false,
        };
        syn::visit::Visit::visit_block(&mut finder, block);
        finder.found
    }

    fn expr_contains(expr: &syn::Expr, fields: &NameSet) -> bool {
        let mut finder = SelfFieldFinder {
            fields,
            found: false,
        };
        syn::visit::Visit::visit_expr(&mut finder, expr);
        finder.found
    }
}

impl syn::visit::Visit<'_> for SelfFieldFinder<'_> {
    fn visit_expr_field(&mut self, field: &syn::ExprField) {
        if is_self_field_in(field, self.fields) {
            self.found = true;
            return;
        }
        syn::visit::visit_expr_field(self, field);
    }
}

fn stmt_needs_reflection(stmt: &syn::Stmt, self_reflect_fields: Option<&NameSet>) -> bool {
    let mut finder = ReflectionFallbackFinder {
        self_reflect_fields,
        found: false,
    };
    syn::visit::Visit::visit_stmt(&mut finder, stmt);
    finder.found
}

fn expr_needs_reflection(expr: &syn::Expr, self_reflect_fields: Option<&NameSet>) -> bool {
    let mut finder = ReflectionFallbackFinder {
        self_reflect_fields,
        found: false,
    };
    syn::visit::Visit::visit_expr(&mut finder, expr);
    finder.found
}

fn block_needs_reflection(block: &syn::Block, self_reflect_fields: Option<&NameSet>) -> bool {
    let mut finder = ReflectionFallbackFinder {
        self_reflect_fields,
        found: false,
    };
    syn::visit::Visit::visit_block(&mut finder, block);
    finder.found
}

struct ReflectionFallbackFinder<'a> {
    self_reflect_fields: Option<&'a NameSet>,
    found: bool,
}

impl syn::visit::Visit<'_> for ReflectionFallbackFinder<'_> {
    fn visit_expr_path(&mut self, path: &syn::ExprPath) {
        if is_reflect_module_path(&path.path) {
            self.found = true;
            return;
        }
        syn::visit::visit_expr_path(self, path);
    }

    fn visit_type_path(&mut self, path: &syn::TypePath) {
        if is_reflect_module_path(&path.path) {
            self.found = true;
            return;
        }
        syn::visit::visit_type_path(self, path);
    }

    fn visit_expr_field(&mut self, field: &syn::ExprField) {
        if self
            .self_reflect_fields
            .is_some_and(|fields| is_self_field_in(field, fields))
        {
            self.found = true;
            return;
        }
        syn::visit::visit_expr_field(self, field);
    }
}

fn is_reflect_module_path(path: &syn::Path) -> bool {
    let segments = path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>();
    matches!(
        segments.as_slice(),
        [first, second, ..] if first == "crate" && second == "reflect"
    ) || matches!(segments.as_slice(), [first, _, ..] if first == "reflect")
}

fn is_reflect_value_type(ty: &syn::Type) -> bool {
    let syn::Type::Path(path) = ty else {
        return false;
    };
    path.path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "Value")
        && is_reflect_module_path(&path.path)
}

fn member_ident_name(member: &syn::Member) -> Option<&syn::Ident> {
    match member {
        syn::Member::Named(ident) => Some(ident),
        syn::Member::Unnamed(_) => None,
    }
}

fn is_self_field_in(field: &syn::ExprField, fields: &NameSet) -> bool {
    super::syntax::is_self_expr(&field.base)
        && member_ident_name(&field.member)
            .is_some_and(|member| fields.contains(&member.to_string()))
}

fn collect_fmt_flush_methods_by_receiver(file: &syn::File) -> ReceiverNameMap {
    let flush_source_fields = collect_fmt_flush_source_fields_by_receiver(file);
    let mut methods_by_receiver = ReceiverNameMap::new();

    for (self_ty, source_fields) in flush_source_fields {
        let receiver_methods = collect_receiver_flush_methods(file, &self_ty, &source_fields);
        if !receiver_methods.is_empty() {
            methods_by_receiver.insert(self_ty, receiver_methods);
        }
    }
    methods_by_receiver
}

fn collect_fmt_flush_source_fields_by_receiver(file: &syn::File) -> ReceiverNameMap {
    let mut fields_by_receiver = ReceiverNameMap::new();

    for item in &file.items {
        let syn::Item::Impl(item_impl) = item else {
            continue;
        };
        let Some(self_ty) = super::syntax::type_path_ident_name(&item_impl.self_ty) else {
            continue;
        };
        for func in item_impl.items.iter().filter_map(|item| {
            let syn::ImplItem::Fn(func) = item else {
                return None;
            };
            (func.sig.ident == "__gors_flush_fmt").then_some(func)
        }) {
            let fields = fmt_flush_source_fields(func);
            if !fields.is_empty() {
                fields_by_receiver
                    .entry(self_ty.clone())
                    .or_default()
                    .extend(fields);
            }
        }
    }

    fields_by_receiver
}

fn collect_receiver_flush_methods(
    file: &syn::File,
    self_ty: &str,
    source_fields: &NameSet,
) -> NameSet {
    let mut direct_methods = NameSet::new();
    let mut calls_by_method = std::collections::BTreeMap::<String, NameSet>::new();

    for item in &file.items {
        let syn::Item::Impl(item_impl) = item else {
            continue;
        };
        if super::syntax::type_path_ident_name(&item_impl.self_ty).as_deref() != Some(self_ty) {
            continue;
        }
        for func in item_impl.items.iter().filter_map(|item| {
            let syn::ImplItem::Fn(func) = item else {
                return None;
            };
            (func.sig.ident != "__gors_flush_fmt").then_some(func)
        }) {
            let name = func.sig.ident.to_string();
            calls_by_method
                .entry(name.clone())
                .or_default()
                .extend(self_method_calls(func));
            if method_calls_flush_source_field(func, source_fields) {
                direct_methods.insert(name);
            }
        }
    }

    expand_transitive_flush_methods(direct_methods, &calls_by_method)
}

fn expand_transitive_flush_methods(
    mut methods: NameSet,
    calls_by_method: &std::collections::BTreeMap<String, NameSet>,
) -> NameSet {
    loop {
        let mut changed = false;
        for (method, callees) in calls_by_method {
            if methods.contains(method) || !callees.iter().any(|callee| methods.contains(callee)) {
                continue;
            }
            methods.insert(method.clone());
            changed = true;
        }
        if !changed {
            break;
        }
    }
    methods
}

fn fmt_flush_source_fields(func: &syn::ImplItemFn) -> NameSet {
    struct Finder {
        fields: NameSet,
    }

    impl syn::visit::Visit<'_> for Finder {
        fn visit_expr_call(&mut self, call: &syn::ExprCall) {
            if expr_call_path_ends_with(call, "take") {
                for arg in &call.args {
                    collect_direct_self_fields(arg, &mut self.fields);
                }
                return;
            }
            syn::visit::visit_expr_call(self, call);
        }
    }

    let mut finder = Finder {
        fields: NameSet::new(),
    };
    syn::visit::Visit::visit_block(&mut finder, &func.block);
    finder.fields
}

fn self_method_calls(func: &syn::ImplItemFn) -> NameSet {
    struct Finder {
        calls: NameSet,
    }

    impl syn::visit::Visit<'_> for Finder {
        fn visit_expr_method_call(&mut self, call: &syn::ExprMethodCall) {
            if super::syntax::is_self_expr(&call.receiver) {
                self.calls.insert(call.method.to_string());
            }
            syn::visit::visit_expr_method_call(self, call);
        }
    }

    let mut finder = Finder {
        calls: NameSet::new(),
    };
    syn::visit::Visit::visit_block(&mut finder, &func.block);
    finder.calls
}

fn method_calls_flush_source_field(func: &syn::ImplItemFn, source_fields: &NameSet) -> bool {
    struct Finder<'a> {
        source_fields: &'a NameSet,
        found: bool,
    }

    impl syn::visit::Visit<'_> for Finder<'_> {
        fn visit_expr_method_call(&mut self, call: &syn::ExprMethodCall) {
            if expr_mentions_self_field_in(&call.receiver, self.source_fields) {
                self.found = true;
                return;
            }
            syn::visit::visit_expr_method_call(self, call);
        }
    }

    let mut finder = Finder {
        source_fields,
        found: false,
    };
    syn::visit::Visit::visit_block(&mut finder, &func.block);
    finder.found
}

fn expr_call_path_ends_with(call: &syn::ExprCall, name: &str) -> bool {
    let syn::Expr::Path(path) = &*call.func else {
        return false;
    };
    path.path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == name)
}

fn collect_direct_self_fields(expr: &syn::Expr, fields: &mut NameSet) {
    struct Collector<'a> {
        fields: &'a mut NameSet,
    }

    impl syn::visit::Visit<'_> for Collector<'_> {
        fn visit_expr_field(&mut self, field: &syn::ExprField) {
            if super::syntax::is_self_expr(&field.base)
                && let Some(ident) = member_ident_name(&field.member)
            {
                self.fields.insert(ident.to_string());
                return;
            }
            syn::visit::visit_expr_field(self, field);
        }
    }

    let mut collector = Collector { fields };
    syn::visit::Visit::visit_expr(&mut collector, expr);
}

fn collect_self_reflect_value_fields(file: &syn::File) -> ReceiverNameMap {
    file.items
        .iter()
        .filter_map(|item| {
            let syn::Item::Struct(item_struct) = item else {
                return None;
            };
            let fields = item_struct
                .fields
                .iter()
                .filter_map(|field| {
                    is_reflect_value_type(&field.ty)
                        .then(|| field.ident.as_ref().map(|ident| ident.to_string()))
                        .flatten()
                })
                .collect::<NameSet>();
            (!fields.is_empty()).then(|| (item_struct.ident.to_string(), fields))
        })
        .collect()
}

fn stmt_needs_fmt_flush(stmt: &syn::Stmt, methods: &NameSet) -> bool {
    matches!(stmt, syn::Stmt::Expr(expr, _) if expr_needs_fmt_flush(expr, methods))
}

fn expr_needs_fmt_flush(expr: &syn::Expr, methods: &NameSet) -> bool {
    let syn::Expr::MethodCall(call) = expr else {
        return false;
    };
    if !methods.contains(&call.method.to_string()) {
        return false;
    }
    super::syntax::is_self_expr(&call.receiver)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_flush_metadata_requires_hook_on_same_receiver() {
        let file: syn::File = syn::parse_quote! {
            struct Printer;
            struct Other;

            impl Printer {
                fn printArg(&mut self) {}
            }

            impl Other {
                fn __gors_flush_fmt(&mut self) {}
            }
        };

        let metadata = FmtFlushMetadata::collect(&file);
        assert!(!metadata.has_receiver("Printer"));
        assert!(!metadata.has_receiver("Other"));
    }

    #[test]
    fn fmt_flush_metadata_derives_methods_from_hook_source_field() {
        let file: syn::File = syn::parse_quote! {
            struct Printer {
                inner: Inner,
                buf: Buffer,
            }

            struct Inner {
                buf: Buffer,
            }

            struct Buffer(Vec<u8>);

            impl Inner {
                fn write(&mut self, value: isize) {}
            }

            impl Printer {
                fn __gors_flush_fmt(&mut self) {
                    let bytes = std::mem::take(&mut self.inner.buf.0);
                    self.buf.0.extend(bytes);
                }

                fn emit(&mut self, value: isize) {
                    self.inner.write(value);
                }

                fn run(&mut self) {
                    self.emit(1);
                }
            }
        };

        let metadata = FmtFlushMetadata::collect(&file);
        let receiver = ["Printer".to_string()];
        let emit_stmt: syn::Stmt = syn::parse_quote! {
            self.emit(1);
        };
        let run_stmt: syn::Stmt = syn::parse_quote! {
            self.run();
        };

        assert!(metadata.should_flush_after_stmt(&receiver, &emit_stmt));
        assert!(metadata.should_flush_after_stmt(&receiver, &run_stmt));
    }

    #[test]
    fn fmt_flush_metadata_ignores_method_names_without_source_field_use() {
        let file: syn::File = syn::parse_quote! {
            struct Printer {
                inner: Inner,
                buf: Buffer,
            }

            struct Inner {
                buf: Buffer,
            }

            struct Buffer(Vec<u8>);

            impl Printer {
                fn __gors_flush_fmt(&mut self) {
                    let bytes = std::mem::take(&mut self.inner.buf.0);
                    self.buf.0.extend(bytes);
                }

                fn printArg(&mut self, value: isize) {}

                fn run(&mut self) {
                    self.printArg(1);
                }
            }
        };

        let metadata = FmtFlushMetadata::collect(&file);
        let receiver = ["Printer".to_string()];
        let stmt: syn::Stmt = syn::parse_quote! {
            self.printArg(1);
        };

        assert!(!metadata.should_flush_after_stmt(&receiver, &stmt));
    }

    #[test]
    fn reflection_metadata_collects_only_reflect_value_fields() {
        let file: syn::File = syn::parse_quote! {
            struct Printer {
                value: crate::reflect::Value,
                local_value: Value,
                buf: Buffer,
            }
        };

        let metadata = ReflectionFallbackMetadata::collect(&file);
        let fields = metadata
            .fields_for_receiver("Printer")
            .expect("expected reflect field metadata");

        assert!(fields.contains("value"));
        assert!(!fields.contains("local_value"));
        assert!(!fields.contains("buf"));
    }

    #[test]
    fn dependency_pruning_tracks_local_method_call_arguments() {
        let local: syn::Stmt = syn::parse_quote! {
            let mut fallback = self.value;
        };
        let names = stmt_bound_names(&local);
        assert!(names.contains("fallback"), "expected local name: {names:?}");

        let call: syn::Stmt = syn::parse_quote! {
            self.printValue(fallback);
        };
        assert!(
            stmt_mentions_any_name(&call, &names),
            "expected method-call argument to mention dropped local"
        );
    }
}
