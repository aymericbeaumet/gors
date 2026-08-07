//! Function-local constant, type, and variable declaration lowering.

use crate::token::Token;

use super::FunctionLowerer;
use super::expressions::*;
use super::function::LocalConstantSymbol;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{
    DeclSyntax, ExprSyntax, LocalTypeSyntax, SyntaxSource, ValueSpecSyntax,
};
use crate::compiler::types::Ty;

impl FunctionLowerer {
    pub(super) fn lower_local_decl(
        &mut self,
        declaration: &DeclSyntax,
        source: SourceRef,
    ) -> Result<hir::StmtKind, Diagnostic> {
        if declaration.token == Token::CONST {
            return self.lower_local_const_declaration(declaration, source);
        }
        if declaration.token == Token::TYPE {
            return self.lower_local_type_declaration(declaration, source);
        }
        if declaration.token != Token::VAR {
            return Err(Diagnostic::unsupported(
                "local import declarations are not implemented",
                source,
            ));
        }
        if declaration.contains_import_spec || !declaration.type_specs.is_empty() {
            return Err(Diagnostic::semantic(
                "value declaration contains a non-value specification",
                source,
            ));
        }
        let statements = declaration
            .specs
            .iter()
            .map(|spec| self.lower_value_spec(spec, declaration.source, source))
            .collect::<Result<Vec<_>, _>>()?;
        let node = self.alloc_node(declaration.source)?;
        Ok(hir::StmtKind::Block(hir::Block {
            node,
            stmts: statements,
            source: SourceRef::node(node),
        }))
    }

    fn lower_local_const_declaration(
        &mut self,
        declaration: &DeclSyntax,
        source: SourceRef,
    ) -> Result<hir::StmtKind, Diagnostic> {
        if declaration.contains_import_spec || !declaration.type_specs.is_empty() {
            return Err(Diagnostic::semantic(
                "constant declaration contains a non-value specification",
                source,
            ));
        }
        let mut previous_explicit_type: Option<ExprSyntax> = None;
        let mut previous_values: Option<std::sync::Arc<[ExprSyntax]>> = None;
        for spec in &*declaration.specs {
            let (explicit_type, values) = if let Some(values) = &spec.values {
                previous_explicit_type = spec.explicit_type.clone();
                previous_values = Some(values.clone());
                (spec.explicit_type.as_ref(), values.as_ref())
            } else {
                let values = previous_values.as_deref().ok_or_else(|| {
                    Diagnostic::semantic(
                        "first local constant specification requires an expression",
                        source,
                    )
                })?;
                (previous_explicit_type.as_ref(), values)
            };
            if spec.names.len() != values.len() {
                return Err(Diagnostic::semantic(
                    format!(
                        "constant declaration has {} names and {} values",
                        spec.names.len(),
                        values.len()
                    ),
                    source,
                ));
            }
            let declared_type = explicit_type
                .map(|ty| self.lower_scoped_type(ty, source))
                .transpose()?;
            let evaluated = values
                .iter()
                .map(|value| self.eval_constant_expression(value, source, spec.iota))
                .collect::<Result<Vec<_>, _>>()?;
            for (name, (raw_ty, mut value)) in spec.names.iter().zip(evaluated) {
                let ty = declared_type.clone().unwrap_or_else(|| raw_ty.clone());
                ensure_bootstrap_value_type(&ty.default_typed(), source)?;
                if !is_assignable(&raw_ty, &ty) {
                    return Err(Diagnostic::semantic(
                        format!("constant {} is not assignable to {ty:?}", name.name),
                        source,
                    ));
                }
                if !value.is_representable_as(&ty) {
                    return Err(Diagnostic::semantic(
                        format!("constant {} is not representable as {ty:?}", name.name),
                        source,
                    ));
                }
                value = value.normalized_for(&ty);
                let node = self.alloc_node(name.source)?;
                if name.name.as_ref() != "_" {
                    self.bind_local_constant(
                        name.name.to_string(),
                        LocalConstantSymbol { ty, value },
                        SourceRef::node(node),
                    )?;
                }
            }
        }
        let node = self.alloc_node(declaration.source)?;
        Ok(hir::StmtKind::Block(hir::Block {
            node,
            stmts: Vec::new(),
            source: SourceRef::node(node),
        }))
    }

    fn lower_local_type_declaration(
        &mut self,
        declaration: &DeclSyntax,
        source: SourceRef,
    ) -> Result<hir::StmtKind, Diagnostic> {
        if declaration.contains_import_spec || !declaration.specs.is_empty() {
            return Err(Diagnostic::semantic(
                "type declaration contains a non-type specification",
                source,
            ));
        }
        for spec in &*declaration.type_specs {
            self.lower_local_type_spec(spec, source)?;
        }
        let node = self.alloc_node(declaration.source)?;
        Ok(hir::StmtKind::Block(hir::Block {
            node,
            stmts: Vec::new(),
            source: SourceRef::node(node),
        }))
    }

    fn lower_local_type_spec(
        &mut self,
        spec: &LocalTypeSyntax,
        declaration_source: SourceRef,
    ) -> Result<(), Diagnostic> {
        if spec.has_type_parameters {
            return Err(Diagnostic::unsupported(
                "generic local type declarations are not yet implemented",
                declaration_source,
            ));
        }
        let target = self.lower_scoped_type(&spec.target, declaration_source)?;
        ensure_bootstrap_value_type(&target, declaration_source)?;
        let ty = if spec.alias {
            target
        } else {
            Ty::LocalNamed {
                identity: self.alloc_local_type_identity()?,
                underlying: Box::new(target.underlying().clone()),
            }
        };
        let node = self.alloc_node(spec.name.source)?;
        self.bind_local_type(spec.name.name.to_string(), ty, SourceRef::node(node))
    }

    fn lower_value_spec(
        &mut self,
        spec: &ValueSpecSyntax,
        declaration_syntax_source: SyntaxSource,
        declaration_source: SourceRef,
    ) -> Result<hir::Stmt, Diagnostic> {
        let explicit_ty = spec
            .explicit_type
            .as_ref()
            .map(|ty| self.lower_scoped_type(ty, declaration_source))
            .transpose()?;
        let raw_values = spec.values.as_deref().unwrap_or_default();
        if !raw_values.is_empty() && raw_values.len() != spec.names.len() {
            return self.lower_multi_value_spec(
                spec,
                explicit_ty,
                raw_values,
                declaration_syntax_source,
                declaration_source,
            );
        }
        // Go evaluates every RHS in one ValueSpec before any of that
        // spec's names enter scope. A previous ValueSpec in the same
        // declaration is already initialized and visible.
        let mut values = if raw_values.is_empty() {
            let ty = explicit_ty.clone().ok_or_else(|| {
                Diagnostic::semantic(
                    "declaration without initializer requires a type",
                    declaration_source,
                )
            })?;
            spec.names
                .iter()
                .map(|name| {
                    let node = self.alloc_node(name.source)?;
                    let source = SourceRef::node(node);
                    self.zero_value_expr(node, source, ty.clone())
                })
                .collect::<Result<Vec<_>, Diagnostic>>()?
        } else {
            raw_values
                .iter()
                .map(|value| self.lower_expr(value, explicit_ty.as_ref()))
                .collect::<Result<Vec<_>, _>>()?
        };

        let mut destinations = Vec::with_capacity(spec.names.len());
        for (name, value) in spec.names.iter().zip(&mut values) {
            let ty = explicit_ty
                .clone()
                .unwrap_or_else(|| value.ty.default_typed());
            let value_source = value.source;
            ensure_bootstrap_value_type(&ty, value_source)?;
            coerce_expr(value, &ty, value_source)?;
            if name.name.as_ref() == "_" {
                destinations.push(hir::Place::Discard);
            } else {
                let id = self.alloc_local(
                    Some(name.name.to_string()),
                    ty,
                    hir::LocalKind::Variable,
                    name.source,
                )?;
                destinations.push(hir::Place::Local(id));
            }
        }
        let node = self.alloc_node(declaration_syntax_source)?;
        Ok(hir::Stmt {
            node,
            kind: hir::StmtKind::Let {
                destinations,
                values,
            },
            source: SourceRef::node(node),
        })
    }

    fn lower_multi_value_spec(
        &mut self,
        spec: &ValueSpecSyntax,
        explicit_ty: Option<Ty>,
        raw_values: &[ExprSyntax],
        declaration_syntax_source: SyntaxSource,
        declaration_source: SourceRef,
    ) -> Result<hir::Stmt, Diagnostic> {
        let [raw_value] = raw_values else {
            return Err(Diagnostic::semantic(
                format!(
                    "variable declaration has {} names and {} values",
                    spec.names.len(),
                    raw_values.len()
                ),
                declaration_source,
            ));
        };
        // The complete RHS is resolved before any name from this ValueSpec is
        // installed in the lexical scope.
        let value = self.lower_multi_result_expression(raw_value)?;
        let Ty::Tuple(component_types) = &value.ty else {
            return Err(Diagnostic::semantic(
                format!(
                    "variable declaration has {} names and {} values",
                    spec.names.len(),
                    raw_values.len()
                ),
                declaration_source,
            ));
        };
        let component_types = component_types.clone();
        if component_types.len() != spec.names.len() {
            return Err(Diagnostic::semantic(
                format!(
                    "variable declaration has {} names and {} result values",
                    spec.names.len(),
                    component_types.len()
                ),
                declaration_source,
            ));
        }
        if !matches!(
            value.kind,
            hir::ExprKind::Call { .. } | hir::ExprKind::ForwardedCall { .. }
        ) {
            return Err(Diagnostic::backend(
                "tuple-valued non-call reached multi-valued variable declaration",
            ));
        }

        let mut destination_types = Vec::with_capacity(component_types.len());
        let mut coercions = Vec::with_capacity(component_types.len());
        for component_ty in &component_types {
            let destination_ty = explicit_ty
                .clone()
                .unwrap_or_else(|| component_ty.default_typed());
            ensure_bootstrap_value_type(&destination_ty, value.source)?;
            coercions.push(self.assignment_value_coercion(
                component_ty,
                &destination_ty,
                value.source,
            )?);
            destination_types.push(destination_ty);
        }

        let mut destinations = Vec::with_capacity(spec.names.len());
        for (name, ty) in spec.names.iter().zip(destination_types) {
            if name.name.as_ref() == "_" {
                destinations.push(hir::Place::Discard);
            } else {
                let local = self.alloc_local(
                    Some(name.name.to_string()),
                    ty,
                    hir::LocalKind::Variable,
                    name.source,
                )?;
                destinations.push(hir::Place::Local(local));
            }
        }

        let node = self.alloc_node(declaration_syntax_source)?;
        Ok(hir::Stmt {
            node,
            kind: hir::StmtKind::LetTuple {
                destinations,
                value,
                coercions,
            },
            source: SourceRef::node(node),
        })
    }
}
