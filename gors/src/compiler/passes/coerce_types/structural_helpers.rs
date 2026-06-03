use syn::visit_mut::{self, VisitMut};

mod fmt_flush;
mod reflection_fallback;
mod self_fields;

#[derive(Default)]
pub(super) struct Metadata {
    fmt_flush: fmt_flush::Metadata,
    reflection_fallback: reflection_fallback::Metadata,
}

impl Metadata {
    pub(super) fn collect(file: &syn::File) -> Self {
        Self {
            fmt_flush: fmt_flush::Metadata::collect(file),
            reflection_fallback: reflection_fallback::Metadata::collect(file),
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
    ) -> Option<&reflection_fallback::FieldSet> {
        self.reflection_fallback.fields_for_initial_pass(
            self_ty,
            block,
            self.fmt_flush.has_receiver(self_ty),
        )
    }

    fn self_reflect_fields_after_helpers(
        &self,
        self_ty: &str,
        block: &syn::Block,
    ) -> Option<&reflection_fallback::FieldSet> {
        self.reflection_fallback
            .fields_after_helpers(self_ty, block)
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

pub(super) fn prune_reflection_fallback(
    stmts: &mut Vec<syn::Stmt>,
    self_reflect_fields: Option<&reflection_fallback::FieldSet>,
) {
    reflection_fallback::prune(stmts, self_reflect_fields);
}
