//! Typed lowering for slice range statements.

use std::collections::BTreeSet;

use super::FunctionLowerer;
use super::expressions::is_assignable;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{BlockSyntax, ExprSyntax, ExprSyntaxKind, IdentSyntax};
use crate::compiler::types::{IntTy, Ty};
use crate::token::Token;

impl FunctionLowerer {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn lower_range(
        &mut self,
        label: Option<&IdentSyntax>,
        key: Option<&ExprSyntax>,
        value: Option<&ExprSyntax>,
        token: Option<Token>,
        range_expression: &ExprSyntax,
        body: &BlockSyntax,
        source: SourceRef,
    ) -> Result<hir::StmtKind, Diagnostic> {
        let expression = self.lower_expr(range_expression, None)?;
        let Ty::Slice(element) = expression.ty.underlying() else {
            return Err(Diagnostic::unsupported(
                "range currently supports slice values",
                source,
            ));
        };
        if element.underlying() != &Ty::Int(IntTy::Int) {
            return Err(Diagnostic::unsupported(
                "range currently supports []int values",
                source,
            ));
        }
        let element_ty = element.as_ref().clone();
        let label = label.map(|label| label.name.to_string());
        if self.inside_local_closure && label.is_some() {
            return Err(Diagnostic::unsupported(
                "labeled loops in local function literals are not yet implemented",
                source,
            ));
        }
        if let Some(label) = &label
            && !self.declared_labels.insert(label.clone())
        {
            return Err(Diagnostic::semantic(
                format!("label {label} already defined"),
                source,
            ));
        }

        self.push_scope();
        let lowered = (|| {
            let (key, value) = match token {
                None if key.is_none() && value.is_none() => (None, None),
                Some(Token::DEFINE) => {
                    self.declare_range_bindings(key, value, &element_ty, source)?
                }
                Some(Token::ASSIGN) => {
                    self.resolve_range_bindings(key, value, &element_ty, source)?
                }
                _ => {
                    return Err(Diagnostic::semantic(
                        "invalid range assignment clause",
                        source,
                    ));
                }
            };
            self.loop_labels.push(label.clone());
            let body = self.lower_block(body, true);
            self.loop_labels.pop();
            Ok::<_, Diagnostic>((key, value, body?))
        })();
        self.pop_scope();
        let (key, value, body) = lowered?;
        Ok(hir::StmtKind::Range {
            label,
            key,
            value,
            expression,
            body,
        })
    }

    fn declare_range_bindings(
        &mut self,
        key: Option<&ExprSyntax>,
        value: Option<&ExprSyntax>,
        element_ty: &Ty,
        source: SourceRef,
    ) -> Result<(Option<hir::Place>, Option<hir::Place>), Diagnostic> {
        let mut names = BTreeSet::new();
        let key = key
            .map(|target| {
                self.declare_range_binding(target, &Ty::Int(IntTy::Int), &mut names, source)
            })
            .transpose()?;
        let value = value
            .map(|target| self.declare_range_binding(target, element_ty, &mut names, source))
            .transpose()?;
        if !matches!(key, Some(hir::Place::Local(_)))
            && !matches!(value, Some(hir::Place::Local(_)))
        {
            return Err(Diagnostic::semantic(
                "range short declaration introduces no variables",
                source,
            ));
        }
        Ok((key, value))
    }

    fn declare_range_binding(
        &mut self,
        target: &ExprSyntax,
        ty: &Ty,
        names: &mut BTreeSet<String>,
        source: SourceRef,
    ) -> Result<hir::Place, Diagnostic> {
        let ExprSyntaxKind::Ident(name) = &target.kind else {
            return Err(Diagnostic::semantic(
                "range short declaration target must be an identifier",
                source,
            ));
        };
        if name.name.as_ref() == "_" {
            return Ok(hir::Place::Discard);
        }
        if !names.insert(name.name.to_string()) {
            return Err(Diagnostic::semantic(
                format!("{} appears more than once on the left of :=", name.name),
                source,
            ));
        }
        let local = self.alloc_local(
            Some(name.name.to_string()),
            ty.clone(),
            hir::LocalKind::Variable,
            name.source,
        )?;
        Ok(hir::Place::Local(local))
    }

    fn resolve_range_bindings(
        &self,
        key: Option<&ExprSyntax>,
        value: Option<&ExprSyntax>,
        element_ty: &Ty,
        source: SourceRef,
    ) -> Result<(Option<hir::Place>, Option<hir::Place>), Diagnostic> {
        let key = key
            .map(|target| self.resolve_range_binding(target, &Ty::Int(IntTy::Int), source))
            .transpose()?;
        let value = value
            .map(|target| self.resolve_range_binding(target, element_ty, source))
            .transpose()?;
        Ok((key, value))
    }

    fn resolve_range_binding(
        &self,
        target: &ExprSyntax,
        expected: &Ty,
        source: SourceRef,
    ) -> Result<hir::Place, Diagnostic> {
        let place = self.lower_place(target, source)?;
        if let hir::Place::Local(_) = place {
            let actual = self.place_ty(place)?;
            if !is_assignable(expected, actual) {
                return Err(Diagnostic::semantic(
                    format!("range value {expected:?} is not assignable to {actual:?}"),
                    source,
                ));
            }
        }
        Ok(place)
    }
}
