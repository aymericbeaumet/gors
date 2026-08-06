//! Function-local bindings, scopes, and block lowering.

use std::collections::BTreeMap;

use super::{ConstantSymbol, FunctionSymbol};
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::{ClosureId, DefId, LocalId, NodeId};
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{BlockSyntax, FieldListSyntax, SyntaxSource};
use crate::compiler::types::{Signature, Ty};

pub(super) struct FunctionLowerer {
    pub(super) owner: DefId,
    pub(super) next_node: u32,
    pub(super) functions: BTreeMap<String, FunctionSymbol>,
    pub(super) qualified_functions: BTreeMap<(String, String), FunctionSymbol>,
    pub(super) constants: BTreeMap<String, ConstantSymbol>,
    pub(super) qualified_constants: BTreeMap<(String, String), ConstantSymbol>,
    pub(super) type_aliases: BTreeMap<String, Ty>,
    pub(super) signature: Signature,
    pub(super) locals: Vec<hir::Local>,
    pub(super) scopes: Vec<BTreeMap<String, LocalId>>,
    pub(super) closures: Vec<hir::Closure>,
    pub(super) closure_scopes: Vec<BTreeMap<String, ClosureId>>,
    pub(super) named_results: Vec<Option<LocalId>>,
    pub(super) loop_labels: Vec<Option<String>>,
    pub(super) declared_labels: std::collections::BTreeSet<String>,
    pub(super) referenced_gotos: BTreeMap<String, SourceRef>,
    pub(super) defer_registration_depth: usize,
    pub(super) inside_deferred_closure: bool,
    pub(super) inside_local_closure: bool,
    pub(super) source_plan: Vec<(SourceRef, SyntaxSource)>,
}

impl FunctionLowerer {
    /// Allocate a revision-local HIR node index inside this stable owner.
    ///
    /// Unlike `DefId`, this is not a query key. Allocation restarts for every
    /// function rebuild and therefore cannot be perturbed by another
    /// declaration's insertion or ordering.
    pub(super) fn alloc_node(&mut self, source: SyntaxSource) -> Result<NodeId, Diagnostic> {
        let local = self.next_node;
        self.next_node = self
            .next_node
            .checked_add(1)
            .ok_or_else(|| Diagnostic::backend("function exceeds the HIR node ID space"))?;
        let node = NodeId::owner_local(self.owner, local);
        self.source_plan.push((SourceRef::node(node), source));
        Ok(node)
    }

    pub(super) fn alloc_local(
        &mut self,
        name: Option<String>,
        ty: Ty,
        kind: hir::LocalKind,
        syntax_source: SyntaxSource,
    ) -> Result<LocalId, Diagnostic> {
        let index = u32::try_from(self.locals.len())
            .map_err(|_| Diagnostic::backend("function exceeds the HIR local ID space"))?;
        let id = LocalId(index);
        let source = SourceRef::local(self.owner, id);
        if let Some(name) = name.as_ref().filter(|name| name.as_str() != "_") {
            if self
                .closure_scopes
                .last()
                .is_some_and(|scope| scope.contains_key(name))
            {
                return Err(Diagnostic::semantic(
                    format!("{name} redeclared in this block"),
                    source,
                ));
            }
            let scope = self
                .scopes
                .last_mut()
                .ok_or_else(|| Diagnostic::backend("function has no lexical scope"))?;
            if scope.insert(name.clone(), id).is_some() {
                return Err(Diagnostic::semantic(
                    format!("{name} redeclared in this block"),
                    source,
                ));
            }
        }
        self.locals.push(hir::Local {
            id,
            name,
            ty,
            kind,
            source,
        });
        self.source_plan.push((source, syntax_source));
        Ok(id)
    }

    pub(super) fn declare_field_bindings(
        &mut self,
        fields: &FieldListSyntax,
        types: &[Ty],
        kind: hir::LocalKind,
    ) -> Result<Vec<LocalId>, Diagnostic> {
        let mut result = Vec::new();
        let mut type_index = 0;
        for field in &*fields.fields {
            if let Some(names) = &field.names {
                for name in &**names {
                    let ty = types
                        .get(type_index)
                        .cloned()
                        .ok_or_else(|| Diagnostic::backend("signature field mismatch"))?;
                    type_index += 1;
                    result.push(self.alloc_local(
                        Some(name.name.to_string()),
                        ty,
                        kind,
                        name.source,
                    )?);
                }
            } else {
                let ty = types
                    .get(type_index)
                    .cloned()
                    .ok_or_else(|| Diagnostic::backend("signature field mismatch"))?;
                type_index += 1;
                let syntax_source = field
                    .ty
                    .as_ref()
                    .ok_or_else(|| Diagnostic::backend("unnamed signature field has no type"))?
                    .source;
                result.push(self.alloc_local(None, ty, kind, syntax_source)?);
            }
        }
        Ok(result)
    }

    pub(super) fn declare_result_bindings(
        &mut self,
        fields: &FieldListSyntax,
        types: &[Ty],
    ) -> Result<Vec<Option<LocalId>>, Diagnostic> {
        let mut result = Vec::new();
        let mut type_index = 0;
        for field in &*fields.fields {
            if let Some(names) = &field.names {
                for name in &**names {
                    let ty = types
                        .get(type_index)
                        .cloned()
                        .ok_or_else(|| Diagnostic::backend("result field mismatch"))?;
                    type_index += 1;
                    result.push(Some(self.alloc_local(
                        Some(name.name.to_string()),
                        ty,
                        hir::LocalKind::NamedResult,
                        name.source,
                    )?));
                }
            } else {
                types
                    .get(type_index)
                    .ok_or_else(|| Diagnostic::backend("result field mismatch"))?;
                type_index += 1;
                result.push(None);
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

    pub(super) fn lookup_closure(&self, name: &str) -> Option<ClosureId> {
        for (locals, closures) in self.scopes.iter().zip(&self.closure_scopes).rev() {
            if locals.contains_key(name) {
                return None;
            }
            if let Some(id) = closures.get(name) {
                return Some(*id);
            }
        }
        None
    }

    pub(super) fn bind_closure(
        &mut self,
        name: &str,
        id: ClosureId,
        source: SourceRef,
    ) -> Result<(), Diagnostic> {
        if self.lookup_current_local(name).is_some() {
            return Err(Diagnostic::semantic(
                format!("{name} redeclared in this block"),
                source,
            ));
        }
        let scope = self
            .closure_scopes
            .last_mut()
            .ok_or_else(|| Diagnostic::backend("function has no closure scope"))?;
        if scope.insert(name.to_owned(), id).is_some() {
            return Err(Diagnostic::semantic(
                format!("{name} redeclared in this block"),
                source,
            ));
        }
        Ok(())
    }

    pub(super) fn push_scope(&mut self) {
        self.scopes.push(BTreeMap::new());
        self.closure_scopes.push(BTreeMap::new());
    }

    pub(super) fn pop_scope(&mut self) {
        self.scopes.pop();
        self.closure_scopes.pop();
    }

    pub(super) fn lower_block(
        &mut self,
        block: &BlockSyntax,
        introduce_scope: bool,
    ) -> Result<hir::Block, Diagnostic> {
        if introduce_scope {
            self.push_scope();
            self.defer_registration_depth += 1;
        }
        let mut stmts = Vec::new();
        for stmt in &*block.statements {
            if let Some(stmt) = self.lower_stmt(stmt)? {
                stmts.push(stmt);
            }
        }
        if introduce_scope {
            self.defer_registration_depth -= 1;
            self.pop_scope();
        }
        let node = self.alloc_node(block.source)?;
        Ok(hir::Block {
            node,
            stmts,
            source: SourceRef::node(node),
        })
    }
}
