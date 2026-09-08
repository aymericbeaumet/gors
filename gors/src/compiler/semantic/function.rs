//! Function-local bindings, scopes, and block lowering.

use std::collections::{BTreeMap, BTreeSet};

use super::control_targets::ActiveControlTarget;
use super::{
    ConstantSymbol, FunctionSymbol, GenericFunctionSymbol, GenericTypeSymbol, MethodSymbol,
    VariableSymbol,
};
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::{ClosureId, DefId, LocalId, LocalTypeId, NodeId, QualifiedDefId};
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{BlockSyntax, FieldListSyntax, SyntaxSource};
use crate::compiler::types::{ConstValue, Signature, Ty};

#[derive(Clone)]
pub(super) struct LocalConstantSymbol {
    pub(super) ty: Ty,
    pub(super) value: ConstValue,
}

pub(super) struct FunctionLowerer {
    pub(super) owner: DefId,
    pub(super) next_node: u32,
    pub(super) next_local_type: u32,
    pub(super) next_control_target: u32,
    pub(super) functions: BTreeMap<String, FunctionSymbol>,
    pub(super) qualified_functions: BTreeMap<(String, String), FunctionSymbol>,
    pub(super) methods: BTreeMap<(QualifiedDefId, String), MethodSymbol>,
    pub(super) generic_functions: BTreeMap<String, GenericFunctionSymbol>,
    pub(super) generic_methods: BTreeMap<(QualifiedDefId, String), GenericFunctionSymbol>,
    pub(super) generic_types: BTreeMap<String, GenericTypeSymbol>,
    pub(super) constants: BTreeMap<String, ConstantSymbol>,
    pub(super) qualified_constants: BTreeMap<(String, String), ConstantSymbol>,
    pub(super) local_constant_scopes: Vec<BTreeMap<String, LocalConstantSymbol>>,
    pub(super) variables: BTreeMap<String, VariableSymbol>,
    pub(super) qualified_variables: BTreeMap<(String, String), VariableSymbol>,
    pub(super) package_imports: BTreeSet<String>,
    pub(super) intrinsic_packages: BTreeSet<String>,
    pub(super) type_aliases: BTreeMap<String, Ty>,
    pub(super) type_scope_changes: Vec<BTreeMap<String, Option<Ty>>>,
    pub(super) signature: Signature,
    pub(super) locals: Vec<hir::Local>,
    pub(super) scopes: Vec<BTreeMap<String, LocalId>>,
    pub(super) closures: Vec<hir::Closure>,
    pub(super) closure_scopes: Vec<BTreeMap<String, ClosureId>>,
    pub(super) named_results: Vec<Option<LocalId>>,
    pub(super) control_targets: Vec<ActiveControlTarget>,
    /// Exact semantic target of a specialized range-function yield body.
    /// Only branches resolved to this target become iterator callback returns.
    pub(super) range_yield_target: Option<crate::compiler::ids::ControlTargetId>,
    pub(super) iteration_capture_scopes: Vec<BTreeSet<LocalId>>,
    pub(super) declared_labels: std::collections::BTreeSet<String>,
    pub(super) referenced_gotos: BTreeMap<String, SourceRef>,
    pub(super) defer_registration_depth: usize,
    pub(super) inside_deferred_closure: bool,
    pub(super) inside_local_closure: bool,
    pub(super) active_generic_functions: Vec<crate::compiler::ids::QualifiedDefId>,
    pub(super) source_override: Option<SyntaxSource>,
    pub(super) source_plan: Vec<(SourceRef, SyntaxSource)>,
}

impl FunctionLowerer {
    pub(super) fn lower_scoped_type(
        &self,
        expression: &crate::compiler::syntax::ExprSyntax,
        source: SourceRef,
    ) -> Result<Ty, Diagnostic> {
        if super::generics::type_contains_instantiation(expression) {
            return super::generics::lower_type_with_generic_constant_lookup(
                expression,
                &self.type_aliases,
                &self.generic_types,
                self.generic_method_environment(),
                &|name| {
                    self.lookup_local_constant(name)
                        .map(|constant| (constant.ty.clone(), constant.value.clone()))
                        .or_else(|| {
                            self.constants
                                .get(name)
                                .map(|constant| (constant.ty.clone(), constant.value.clone()))
                        })
                },
                source,
            );
        }
        super::lower_type_with_constant_lookup(
            expression,
            &self.type_aliases,
            &|name| {
                self.lookup_local_constant(name)
                    .map(|constant| (constant.ty.clone(), constant.value.clone()))
                    .or_else(|| {
                        self.constants
                            .get(name)
                            .map(|constant| (constant.ty.clone(), constant.value.clone()))
                    })
            },
            source,
        )
    }

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
        self.source_plan.push((
            SourceRef::node(node),
            self.source_override.unwrap_or(source),
        ));
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
                .type_scope_changes
                .last()
                .is_some_and(|scope| scope.contains_key(name))
                || self
                    .local_constant_scopes
                    .last()
                    .is_some_and(|scope| scope.contains_key(name))
            {
                return Err(Diagnostic::semantic(
                    format!("{name} redeclared in this block"),
                    source,
                ));
            }
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
        self.source_plan
            .push((source, self.source_override.unwrap_or(syntax_source)));
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
        for (locals, constants) in self.scopes.iter().zip(&self.local_constant_scopes).rev() {
            if let Some(local) = locals.get(name) {
                return Some(*local);
            }
            if constants.contains_key(name) {
                return None;
            }
        }
        None
    }

    pub(super) fn lookup_local_constant(&self, name: &str) -> Option<&LocalConstantSymbol> {
        for (locals, constants) in self.scopes.iter().zip(&self.local_constant_scopes).rev() {
            if let Some(constant) = constants.get(name) {
                return Some(constant);
            }
            if locals.contains_key(name) {
                return None;
            }
        }
        None
    }

    pub(super) fn eval_constant_expression(
        &self,
        expression: &crate::compiler::syntax::ExprSyntax,
        source: SourceRef,
        iota: Option<u64>,
    ) -> Result<(Ty, ConstValue), Diagnostic> {
        super::constants::eval_constant_with_length_capacity(
            expression,
            &|name| {
                self.lookup_local_constant(name)
                    .map(|constant| (constant.ty.clone(), constant.value.clone()))
                    .or_else(|| {
                        self.constants
                            .get(name)
                            .map(|constant| (constant.ty.clone(), constant.value.clone()))
                    })
            },
            &|name| self.type_aliases.get(name).cloned(),
            &|operation, operand, source| {
                if !self.resolves_to_predeclared(operation.name()) {
                    return Err(Diagnostic::semantic(
                        format!(
                            "{} does not resolve to the predeclared builtin",
                            operation.name()
                        ),
                        source,
                    ));
                }
                let checked = self.check_length_capacity_operand(operand, source, iota)?;
                super::length_capacity::classify(
                    operation,
                    &checked.ty,
                    checked.constant.as_ref(),
                    checked.contains_call_or_receive,
                    source,
                )
            },
            source,
            iota,
        )
    }

    pub(super) fn bind_local_constant(
        &mut self,
        name: String,
        constant: LocalConstantSymbol,
        source: SourceRef,
    ) -> Result<(), Diagnostic> {
        if self.lookup_current_local(&name).is_some()
            || self
                .closure_scopes
                .last()
                .is_some_and(|scope| scope.contains_key(&name))
            || self
                .type_scope_changes
                .last()
                .is_some_and(|scope| scope.contains_key(&name))
        {
            return Err(Diagnostic::semantic(
                format!("{name} redeclared in this block"),
                source,
            ));
        }
        let scope = self
            .local_constant_scopes
            .last_mut()
            .ok_or_else(|| Diagnostic::backend("function has no local constant scope"))?;
        if scope.insert(name.clone(), constant).is_some() {
            return Err(Diagnostic::semantic(
                format!("{name} redeclared in this block"),
                source,
            ));
        }
        Ok(())
    }

    pub(super) fn lookup_current_local(&self, name: &str) -> Option<LocalId> {
        self.scopes
            .last()
            .and_then(|scope| scope.get(name).copied())
    }

    pub(super) fn lookup_closure(&self, name: &str) -> Option<ClosureId> {
        for ((locals, constants), closures) in self
            .scopes
            .iter()
            .zip(&self.local_constant_scopes)
            .zip(&self.closure_scopes)
            .rev()
        {
            if locals.contains_key(name) || constants.contains_key(name) {
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
        if self.lookup_current_local(name).is_some()
            || self
                .type_scope_changes
                .last()
                .is_some_and(|scope| scope.contains_key(name))
            || self
                .local_constant_scopes
                .last()
                .is_some_and(|scope| scope.contains_key(name))
        {
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

    pub(super) fn alloc_local_type_identity(&mut self) -> Result<LocalTypeId, Diagnostic> {
        let local = self.next_local_type;
        self.next_local_type = self
            .next_local_type
            .checked_add(1)
            .ok_or_else(|| Diagnostic::backend("function exceeds the local type ID space"))?;
        Ok(LocalTypeId::owner_local(self.owner, local))
    }

    pub(super) fn expand_named_ref(&self, ty: &Ty) -> Result<Ty, Diagnostic> {
        let Ty::NamedRef { identity } = ty else {
            return Ok(ty.clone());
        };
        self.type_aliases
            .values()
            .find(|candidate| {
                matches!(candidate, Ty::Named { identity: candidate, .. } if candidate == identity)
            })
            .cloned()
            .ok_or_else(|| {
                Diagnostic::backend(format!(
                    "recursive named type {identity:?} is absent from the package type index"
                ))
            })
    }

    pub(super) fn bind_local_type(
        &mut self,
        name: String,
        ty: Ty,
        source: SourceRef,
    ) -> Result<(), Diagnostic> {
        if self.lookup_current_local(&name).is_some()
            || self
                .closure_scopes
                .last()
                .is_some_and(|scope| scope.contains_key(&name))
            || self
                .local_constant_scopes
                .last()
                .is_some_and(|scope| scope.contains_key(&name))
        {
            return Err(Diagnostic::semantic(
                format!("{name} redeclared in this block"),
                source,
            ));
        }
        let scope = self
            .type_scope_changes
            .last_mut()
            .ok_or_else(|| Diagnostic::backend("function has no local type scope"))?;
        if scope.contains_key(&name) {
            return Err(Diagnostic::semantic(
                format!("{name} redeclared in this block"),
                source,
            ));
        }
        let previous = self.type_aliases.insert(name.clone(), ty);
        scope.insert(name, previous);
        Ok(())
    }

    pub(super) fn push_scope(&mut self) {
        self.scopes.push(BTreeMap::new());
        self.closure_scopes.push(BTreeMap::new());
        self.type_scope_changes.push(BTreeMap::new());
        self.local_constant_scopes.push(BTreeMap::new());
    }

    pub(super) fn pop_scope(&mut self) {
        self.scopes.pop();
        self.closure_scopes.pop();
        self.local_constant_scopes.pop();
        if let Some(changes) = self.type_scope_changes.pop() {
            for (name, previous) in changes {
                if let Some(previous) = previous {
                    self.type_aliases.insert(name, previous);
                } else {
                    self.type_aliases.remove(&name);
                }
            }
        }
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
