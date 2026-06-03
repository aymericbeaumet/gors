use syn::visit_mut::{self, VisitMut};

mod fmt_flush;

type NameSet = std::collections::HashSet<String>;
type ReceiverNameMap = std::collections::BTreeMap<String, NameSet>;

#[derive(Default)]
pub(super) struct Metadata {
    fmt_flush: fmt_flush::Metadata,
    reflection_fallback: ReflectionFallbackMetadata,
}

impl Metadata {
    pub(super) fn collect(file: &syn::File) -> Self {
        Self {
            fmt_flush: fmt_flush::Metadata::collect(file),
            reflection_fallback: ReflectionFallbackMetadata::collect(file),
        }
    }

    fn is_empty(&self) -> bool {
        self.fmt_flush.is_empty() && self.reflection_fallback.is_empty()
    }

    pub(super) fn push_stmt_with_flush(
        &self,
        impl_self_types: &[String],
        stmt: syn::Stmt,
        stmts: &mut Vec<syn::Stmt>,
    ) {
        self.fmt_flush
            .push_stmt_with_flush(impl_self_types, stmt, stmts);
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

#[cfg(test)]
mod tests {
    use super::*;

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
