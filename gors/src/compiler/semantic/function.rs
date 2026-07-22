//! Function-local bindings, scopes, and block lowering.

use std::collections::BTreeMap;

use crate::ast;

use super::{ConstantSymbol, FileLowerer, FunctionSymbol};
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::{DefId, LocalId, NodeId, SourceSpan};
use crate::compiler::types::{Signature, Ty};

pub(super) struct FunctionLowerer<'a> {
    pub(super) file: &'a mut FileLowerer,
    pub(super) owner: DefId,
    pub(super) next_node: u32,
    pub(super) functions: BTreeMap<String, FunctionSymbol>,
    pub(super) constants: BTreeMap<String, ConstantSymbol>,
    pub(super) signature: Signature,
    pub(super) locals: Vec<hir::Local>,
    pub(super) scopes: Vec<BTreeMap<String, LocalId>>,
    pub(super) named_results: Vec<Option<LocalId>>,
    pub(super) loop_depth: usize,
}

impl FunctionLowerer<'_> {
    /// Allocate a revision-local HIR node index inside this stable owner.
    ///
    /// Unlike `DefId`, this is not a query key. Allocation restarts for every
    /// function rebuild and therefore cannot be perturbed by another
    /// declaration's insertion or ordering.
    pub(super) fn alloc_node(&mut self) -> Result<NodeId, Diagnostic> {
        let local = self.next_node;
        self.next_node = self
            .next_node
            .checked_add(1)
            .ok_or_else(|| Diagnostic::backend("function exceeds the HIR node ID space"))?;
        Ok(NodeId::owner_local(self.owner, local))
    }

    pub(super) fn alloc_local(
        &mut self,
        name: Option<String>,
        ty: Ty,
        kind: hir::LocalKind,
        span: SourceSpan,
    ) -> Result<LocalId, Diagnostic> {
        let id = LocalId(self.locals.len() as u32);
        if let Some(name) = name.as_ref().filter(|name| name.as_str() != "_") {
            let scope = self
                .scopes
                .last_mut()
                .ok_or_else(|| Diagnostic::backend("function has no lexical scope"))?;
            if scope.insert(name.clone(), id).is_some() {
                return Err(Diagnostic::semantic(
                    format!("{} redeclared in this block", name),
                    span,
                ));
            }
        }
        self.locals.push(hir::Local {
            id,
            name,
            ty,
            kind,
            span,
        });
        Ok(id)
    }

    pub(super) fn declare_field_bindings(
        &mut self,
        fields: &ast::FieldList<'_>,
        types: &[Ty],
        kind: hir::LocalKind,
    ) -> Result<Vec<LocalId>, Diagnostic> {
        let mut result = Vec::new();
        let mut type_index = 0;
        for field in &fields.list {
            let names = field
                .names
                .as_ref()
                .map(|names| names.iter().map(Some).collect::<Vec<_>>())
                .unwrap_or_else(|| vec![None]);
            for name in names {
                let ty = types.get(type_index).cloned().ok_or_else(|| {
                    Diagnostic::semantic("signature field mismatch", SourceSpan::synthetic())
                })?;
                type_index += 1;
                let span = name
                    .map(|name| self.file.span(&name.name_pos))
                    .unwrap_or_else(SourceSpan::synthetic);
                let source_name = name.map(|name| name.name.to_string());
                result.push(self.alloc_local(source_name, ty, kind, span)?);
            }
        }
        Ok(result)
    }

    pub(super) fn declare_result_bindings(
        &mut self,
        fields: &ast::FieldList<'_>,
        types: &[Ty],
    ) -> Result<Vec<Option<LocalId>>, Diagnostic> {
        let mut result = Vec::new();
        let mut type_index = 0;
        for field in &fields.list {
            let names = field
                .names
                .as_ref()
                .map(|names| names.iter().map(Some).collect::<Vec<_>>())
                .unwrap_or_else(|| vec![None]);
            for name in names {
                let ty = types.get(type_index).cloned().ok_or_else(|| {
                    Diagnostic::semantic("result field mismatch", SourceSpan::synthetic())
                })?;
                type_index += 1;
                let Some(name) = name else {
                    result.push(None);
                    continue;
                };
                let id = self.alloc_local(
                    Some(name.name.to_string()),
                    ty,
                    hir::LocalKind::NamedResult,
                    self.file.span(&name.name_pos),
                )?;
                result.push(Some(id));
            }
        }
        Ok(result)
    }

    pub(super) fn lookup_local(&self, name: &str) -> Option<LocalId> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
    }

    pub(super) fn lookup_current_local(&self, name: &str) -> Option<LocalId> {
        self.scopes
            .last()
            .and_then(|scope| scope.get(name).copied())
    }

    pub(super) fn push_scope(&mut self) {
        self.scopes.push(BTreeMap::new());
    }

    pub(super) fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    pub(super) fn lower_block(
        &mut self,
        block: &ast::BlockStmt<'_>,
        introduce_scope: bool,
    ) -> Result<hir::Block, Diagnostic> {
        if introduce_scope {
            self.push_scope();
        }
        let mut stmts = Vec::new();
        for stmt in &block.list {
            if let Some(stmt) = self.lower_stmt(stmt)? {
                stmts.push(stmt);
            }
        }
        if introduce_scope {
            self.pop_scope();
        }
        Ok(hir::Block {
            node: self.alloc_node()?,
            stmts,
            span: self.file.span(&block.lbrace),
        })
    }
}
