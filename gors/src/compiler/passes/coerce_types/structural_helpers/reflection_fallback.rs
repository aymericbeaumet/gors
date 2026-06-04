pub(super) type FieldSet = super::local_names::NameSet;
use crate::compiler::{reflect_semantics, syn_inspect};

type ReceiverFieldMap = std::collections::BTreeMap<String, FieldSet>;
type ReceiverMethodMap = std::collections::BTreeMap<String, FieldSet>;

#[derive(Default)]
pub(super) struct Metadata {
    self_reflect_value_fields: ReceiverFieldMap,
    reflect_value_methods: ReceiverMethodMap,
}

impl Metadata {
    pub(super) fn collect(file: &syn::File) -> Self {
        Self {
            self_reflect_value_fields: collect_self_reflect_value_fields(file),
            reflect_value_methods: collect_reflect_value_methods(file),
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.self_reflect_value_fields.is_empty()
    }

    pub(super) fn fields_for_initial_pass(
        &self,
        self_ty: &str,
        block: &syn::Block,
    ) -> Option<&FieldSet> {
        let fields = self.fields_for_receiver(self_ty)?;
        let methods = self.reflect_value_methods.get(self_ty);
        (block_has_self_reflect_field_runtime_fallback(block, fields)
            || methods.is_some_and(|methods| {
                block_passes_self_reflect_field_to_method(block, fields, methods)
            }))
        .then_some(fields)
    }

    pub(super) fn fields_after_helpers(
        &self,
        self_ty: &str,
        block: &syn::Block,
    ) -> Option<&FieldSet> {
        let fields = self.fields_for_receiver(self_ty)?;
        block_has_self_reflect_field_runtime_fallback(block, fields).then_some(fields)
    }

    fn fields_for_receiver(&self, self_ty: &str) -> Option<&FieldSet> {
        self.self_reflect_value_fields.get(self_ty)
    }
}

fn block_has_self_reflect_field_runtime_fallback(block: &syn::Block, fields: &FieldSet) -> bool {
    struct Finder<'a> {
        fields: &'a FieldSet,
        found: bool,
    }

    impl syn::visit::Visit<'_> for Finder<'_> {
        fn visit_expr_method_call(&mut self, call: &syn::ExprMethodCall) {
            if syn_inspect::ident_matches_any(&call.method, &["IsValid", "Type"])
                && super::self_fields::expr_mentions(&call.receiver, self.fields)
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

fn block_passes_self_reflect_field_to_method(
    block: &syn::Block,
    fields: &FieldSet,
    methods: &FieldSet,
) -> bool {
    let mut reflect_field_locals = FieldSet::new();
    for stmt in &block.stmts {
        if stmt_calls_method_with_reflect_source(stmt, fields, &reflect_field_locals, methods) {
            return true;
        }
        let bound_names = super::local_names::stmt_bound_names(stmt);
        if !bound_names.is_empty()
            && stmt_mentions_reflect_source(stmt, fields, &reflect_field_locals)
        {
            reflect_field_locals.extend(bound_names);
        }
    }
    false
}

fn stmt_calls_method_with_reflect_source(
    stmt: &syn::Stmt,
    fields: &FieldSet,
    local_names: &FieldSet,
    methods: &FieldSet,
) -> bool {
    struct Finder<'a> {
        fields: &'a FieldSet,
        local_names: &'a FieldSet,
        methods: &'a FieldSet,
        found: bool,
    }

    impl syn::visit::Visit<'_> for Finder<'_> {
        fn visit_expr_method_call(&mut self, call: &syn::ExprMethodCall) {
            if syn_inspect::is_self_expr(&call.receiver)
                && self.methods.contains(&call.method.to_string())
                && call
                    .args
                    .iter()
                    .any(|arg| expr_mentions_reflect_source(arg, self.fields, self.local_names))
            {
                self.found = true;
                return;
            }
            syn::visit::visit_expr_method_call(self, call);
        }
    }

    let mut finder = Finder {
        fields,
        local_names,
        methods,
        found: false,
    };
    syn::visit::Visit::visit_stmt(&mut finder, stmt);
    finder.found
}

fn stmt_mentions_reflect_source(
    stmt: &syn::Stmt,
    fields: &FieldSet,
    local_names: &FieldSet,
) -> bool {
    match stmt {
        syn::Stmt::Expr(expr, _) => expr_mentions_reflect_source(expr, fields, local_names),
        syn::Stmt::Local(local) => local
            .init
            .as_ref()
            .is_some_and(|init| expr_mentions_reflect_source(&init.expr, fields, local_names)),
        syn::Stmt::Item(_) | syn::Stmt::Macro(_) => false,
    }
}

fn expr_mentions_reflect_source(
    expr: &syn::Expr,
    fields: &FieldSet,
    local_names: &FieldSet,
) -> bool {
    super::self_fields::expr_mentions(expr, fields)
        || super::local_names::expr_mentions_any_name(expr, local_names)
}

pub(super) fn prune(stmts: &mut Vec<syn::Stmt>, self_reflect_fields: Option<&FieldSet>) {
    let old_stmts = std::mem::take(stmts);
    let block = prune_block(
        syn::Block {
            brace_token: syn::token::Brace::default(),
            stmts: old_stmts,
        },
        self_reflect_fields,
    );
    *stmts = block.stmts;
}

fn prune_stmt(stmt: syn::Stmt, self_reflect_fields: Option<&FieldSet>) -> Option<syn::Stmt> {
    if stmt_needs_reflection(&stmt, self_reflect_fields) {
        match stmt {
            syn::Stmt::Expr(expr, semi) => {
                prune_expr(expr, self_reflect_fields).map(|expr| syn::Stmt::Expr(expr, semi))
            }
            syn::Stmt::Local(mut local) => {
                if let Some(init) = &mut local.init {
                    let expr = std::mem::replace(
                        &mut init.expr,
                        Box::new(syn::parse_quote! { Default::default() }),
                    );
                    let expr = prune_expr(*expr, self_reflect_fields)?;
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

fn prune_expr(expr: syn::Expr, self_reflect_fields: Option<&FieldSet>) -> Option<syn::Expr> {
    match expr {
        syn::Expr::Block(expr_block) => prune_expr_block(expr_block, self_reflect_fields),
        syn::Expr::If(expr_if) => prune_if(expr_if, self_reflect_fields),
        other if expr_needs_reflection(&other, self_reflect_fields) => None,
        other => Some(other),
    }
}

fn prune_expr_block(
    mut expr_block: syn::ExprBlock,
    self_reflect_fields: Option<&FieldSet>,
) -> Option<syn::Expr> {
    expr_block.block = prune_block(expr_block.block, self_reflect_fields);
    (!expr_block.block.stmts.is_empty()).then_some(syn::Expr::Block(expr_block))
}

fn prune_if(mut expr_if: syn::ExprIf, self_reflect_fields: Option<&FieldSet>) -> Option<syn::Expr> {
    if expr_needs_reflection(&expr_if.cond, self_reflect_fields) {
        return expr_if
            .else_branch
            .and_then(|(_, else_expr)| prune_expr(*else_expr, self_reflect_fields));
    }

    let then_had_reflection = block_needs_reflection(&expr_if.then_branch, self_reflect_fields);
    expr_if.then_branch = prune_block(expr_if.then_branch, self_reflect_fields);
    let then_is_empty = expr_if.then_branch.stmts.is_empty();

    expr_if.else_branch = expr_if.else_branch.and_then(|(else_token, else_expr)| {
        prune_expr(*else_expr, self_reflect_fields).map(|expr| (else_token, Box::new(expr)))
    });

    if then_had_reflection && then_is_empty {
        return expr_if.else_branch.map(|(_, else_expr)| *else_expr);
    }

    Some(syn::Expr::If(expr_if))
}

fn prune_block(mut block: syn::Block, self_reflect_fields: Option<&FieldSet>) -> syn::Block {
    let mut dropped_names = std::collections::HashSet::new();
    let mut stmts = vec![];
    for stmt in block.stmts {
        let bound_names = super::local_names::stmt_bound_names(&stmt);
        if super::local_names::stmt_mentions_any_name(&stmt, &dropped_names) {
            dropped_names.extend(bound_names);
            continue;
        }
        if let Some(stmt) = prune_stmt(stmt, self_reflect_fields) {
            stmts.push(stmt);
        } else {
            dropped_names.extend(bound_names);
        }
    }
    block.stmts = stmts;
    block
}

fn stmt_needs_reflection(stmt: &syn::Stmt, self_reflect_fields: Option<&FieldSet>) -> bool {
    let mut finder = Finder {
        self_reflect_fields,
        found: false,
    };
    syn::visit::Visit::visit_stmt(&mut finder, stmt);
    finder.found
}

fn expr_needs_reflection(expr: &syn::Expr, self_reflect_fields: Option<&FieldSet>) -> bool {
    let mut finder = Finder {
        self_reflect_fields,
        found: false,
    };
    syn::visit::Visit::visit_expr(&mut finder, expr);
    finder.found
}

fn block_needs_reflection(block: &syn::Block, self_reflect_fields: Option<&FieldSet>) -> bool {
    let mut finder = Finder {
        self_reflect_fields,
        found: false,
    };
    syn::visit::Visit::visit_block(&mut finder, block);
    finder.found
}

struct Finder<'a> {
    self_reflect_fields: Option<&'a FieldSet>,
    found: bool,
}

impl syn::visit::Visit<'_> for Finder<'_> {
    fn visit_expr_path(&mut self, path: &syn::ExprPath) {
        if is_reflect_fallback_expr_path(&path.path) {
            self.found = true;
            return;
        }
        syn::visit::visit_expr_path(self, path);
    }

    fn visit_type_path(&mut self, path: &syn::TypePath) {
        if is_reflect_value_type_path(&path.path) {
            self.found = true;
            return;
        }
        syn::visit::visit_type_path(self, path);
    }

    fn visit_expr_field(&mut self, field: &syn::ExprField) {
        if self
            .self_reflect_fields
            .is_some_and(|fields| super::self_fields::is_self_field_in(field, fields))
        {
            self.found = true;
            return;
        }
        syn::visit::visit_expr_field(self, field);
    }
}

fn is_reflect_fallback_expr_path(path: &syn::Path) -> bool {
    matches!(
        reflect_semantics::syn_path_member(path),
        Some(member)
            if member == reflect_semantics::VALUE_OF_FUNC
                || member == reflect_semantics::TYPE_OF_FUNC
    )
}

fn is_reflect_value_type_path(path: &syn::Path) -> bool {
    reflect_semantics::syn_path_member(path)
        .is_some_and(|member| member == reflect_semantics::VALUE_TYPE)
}

fn is_reflect_value_type(ty: &syn::Type) -> bool {
    let syn::Type::Path(path) = ty else {
        return false;
    };
    is_reflect_value_type_path(&path.path)
}

fn collect_self_reflect_value_fields(file: &syn::File) -> ReceiverFieldMap {
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
                .collect::<FieldSet>();
            (!fields.is_empty()).then(|| (item_struct.ident.to_string(), fields))
        })
        .collect()
}

fn collect_reflect_value_methods(file: &syn::File) -> ReceiverMethodMap {
    let mut methods = ReceiverMethodMap::new();
    for item in &file.items {
        let syn::Item::Impl(item_impl) = item else {
            continue;
        };
        let Some(self_ty) = syn_inspect::type_path_ident_name(&item_impl.self_ty) else {
            continue;
        };
        for func in item_impl.items.iter().filter_map(|item| {
            let syn::ImplItem::Fn(func) = item else {
                return None;
            };
            method_accepts_reflect_value(func).then_some(func)
        }) {
            methods
                .entry(self_ty.clone())
                .or_default()
                .insert(func.sig.ident.to_string());
        }
    }
    methods
}

fn method_accepts_reflect_value(func: &syn::ImplItemFn) -> bool {
    func.sig.inputs.iter().any(|input| match input {
        syn::FnArg::Receiver(_) => false,
        syn::FnArg::Typed(pat_type) => is_reflect_value_type(&pat_type.ty),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn impl_method<'a>(file: &'a syn::File, name: &str) -> Option<&'a syn::ImplItemFn> {
        file.items.iter().find_map(|item| {
            let syn::Item::Impl(item_impl) = item else {
                return None;
            };
            item_impl.items.iter().find_map(|item| {
                let syn::ImplItem::Fn(func) = item else {
                    return None;
                };
                (func.sig.ident == name).then_some(func)
            })
        })
    }

    #[test]
    fn metadata_collects_only_reflect_value_fields() {
        let file: syn::File = syn::parse_quote! {
            struct Printer {
                value: crate::reflect::Value,
                local_value: Value,
                buf: Buffer,
            }
        };

        let metadata = Metadata::collect(&file);
        let fields = metadata.fields_for_receiver("Printer");
        assert!(fields.is_some(), "expected reflect field metadata");
        let Some(fields) = fields else {
            return;
        };

        assert!(fields.contains("value"));
        assert!(!fields.contains("local_value"));
        assert!(!fields.contains("buf"));
    }

    #[test]
    fn metadata_uses_reflect_value_method_parameters_to_activate_pruning() {
        let file: syn::File = syn::parse_quote! {
            struct Printer {
                value: crate::reflect::Value,
            }

            impl Printer {
                fn printValue(&mut self, value: crate::reflect::Value) {}

                fn run(&mut self) {
                    let fallback = self.value;
                    self.printValue(fallback);
                }
            }
        };

        let metadata = Metadata::collect(&file);
        let run = impl_method(&file, "run");
        assert!(run.is_some(), "expected run method");
        let Some(run) = run else {
            return;
        };

        assert!(
            metadata
                .fields_for_initial_pass("Printer", &run.block)
                .is_some(),
            "expected reflect.Value method argument to activate fallback pruning"
        );
    }

    #[test]
    fn metadata_ignores_similar_method_names_without_reflect_value_params() {
        let file: syn::File = syn::parse_quote! {
            struct Printer {
                value: crate::reflect::Value,
            }

            impl Printer {
                fn printValue(&mut self, value: isize) {}

                fn run(&mut self) {
                    let fallback = self.value;
                    self.printValue(1);
                }
            }
        };

        let metadata = Metadata::collect(&file);
        let run = impl_method(&file, "run");
        assert!(run.is_some(), "expected run method");
        let Some(run) = run else {
            return;
        };

        assert!(
            metadata
                .fields_for_initial_pass("Printer", &run.block)
                .is_none(),
            "expected method name alone not to activate fallback pruning"
        );
    }
}
