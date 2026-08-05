//! Typed preparation of assignments with dynamic left-hand-side operands.

use super::FunctionLowerer;
use super::expressions::{default_expr_type, is_assignable};
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{ExprSyntax, ExprSyntaxKind};
use crate::compiler::types::{IntTy, Ty};
use crate::token::Token;

impl FunctionLowerer {
    pub(super) fn try_lower_parallel_index_assignment(
        &mut self,
        left: &[ExprSyntax],
        token: Token,
        right: &[ExprSyntax],
        source: SourceRef,
    ) -> Option<Result<hir::StmtKind, Diagnostic>> {
        if token != Token::ASSIGN
            || left.len() <= 1
            || left.len() != right.len()
            || !left
                .iter()
                .any(|expression| matches!(expression.kind, ExprSyntaxKind::Index { .. }))
        {
            return None;
        }
        Some(self.lower_parallel_index_assignment(left, right, source))
    }

    fn lower_parallel_index_assignment(
        &mut self,
        left: &[ExprSyntax],
        right: &[ExprSyntax],
        source: SourceRef,
    ) -> Result<hir::StmtKind, Diagnostic> {
        let mut destinations = Vec::with_capacity(left.len());
        let mut destination_types = Vec::with_capacity(left.len());
        for expression in left {
            match &expression.kind {
                ExprSyntaxKind::Ident(_) => {
                    let place = self.lower_place(expression, source)?;
                    let ty = match place {
                        hir::Place::Local(_) => Some(self.place_ty(place)?.clone()),
                        hir::Place::Discard => None,
                    };
                    destinations.push(match place {
                        hir::Place::Local(local) => hir::AssignTarget::Local(local),
                        hir::Place::Discard => hir::AssignTarget::Discard,
                    });
                    destination_types.push(ty);
                }
                ExprSyntaxKind::Index { base, index } => {
                    let slice = self.lower_expr(base, None)?;
                    let Ty::Slice(element) = slice.ty.underlying() else {
                        return Err(Diagnostic::semantic(
                            "indexed assignment requires a slice value",
                            source,
                        ));
                    };
                    if element.underlying() != &Ty::Int(IntTy::Int) {
                        return Err(Diagnostic::unsupported(
                            "indexed assignment currently supports []int values",
                            source,
                        ));
                    }
                    let element_ty = element.as_ref().clone();
                    let index = self.lower_expr(index, Some(&Ty::Int(IntTy::Int)))?;
                    destinations.push(hir::AssignTarget::SliceIndex { slice, index });
                    destination_types.push(Some(element_ty));
                }
                _ => {
                    return Err(Diagnostic::unsupported(
                        "this assignment target is not yet implemented",
                        source,
                    ));
                }
            }
        }

        let values = right
            .iter()
            .zip(&destination_types)
            .map(|(expression, expected)| match expected {
                Some(expected) => self.lower_expr(expression, Some(expected)),
                None => self.lower_expr(expression, None).and_then(|expression| {
                    let value_source = expression.source;
                    default_expr_type(expression, value_source)
                }),
            })
            .collect::<Result<Vec<_>, _>>()?;
        for (index, (value, expected)) in values.iter().zip(&destination_types).enumerate() {
            if let Some(expected) = expected
                && !is_assignable(&value.ty, expected)
            {
                return Err(Diagnostic::semantic(
                    format!(
                        "assignment value {index} of type {:?} is not assignable to {expected:?}",
                        value.ty
                    ),
                    source,
                ));
            }
        }
        Ok(hir::StmtKind::ParallelAssign {
            destinations,
            values,
        })
    }
}
