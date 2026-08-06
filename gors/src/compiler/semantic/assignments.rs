//! Typed preparation of assignments with dynamic left-hand-side operands.

use super::FunctionLowerer;
use super::expressions::{default_expr_type, is_assignable};
use super::maps::string_i64_map_ty;
use super::statements::assignment_op;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{ExprSyntax, ExprSyntaxKind};
use crate::compiler::types::{IntTy, Ty};
use crate::token::Token;

impl FunctionLowerer {
    pub(super) fn try_lower_single_index_assignment(
        &mut self,
        left: &[ExprSyntax],
        token: Token,
        right: &[ExprSyntax],
        source: SourceRef,
    ) -> Option<Result<hir::StmtKind, Diagnostic>> {
        let (
            [
                ExprSyntax {
                    kind: ExprSyntaxKind::Index { base, index },
                    ..
                },
            ],
            [value],
        ) = (left, right)
        else {
            return None;
        };
        Some(self.lower_single_index_assignment(base, index, token, value, source))
    }

    fn lower_single_index_assignment(
        &mut self,
        base: &ExprSyntax,
        index: &ExprSyntax,
        token: Token,
        value: &ExprSyntax,
        source: SourceRef,
    ) -> Result<hir::StmtKind, Diagnostic> {
        if token == Token::DEFINE {
            return Err(Diagnostic::semantic(
                "short declaration target must be an identifier",
                source,
            ));
        }
        let container = self.lower_expr(base, None)?;
        match container.ty.underlying() {
            Ty::Slice(element) if element.underlying() == &Ty::Int(IntTy::Int) => {
                let element_ty = element.as_ref().clone();
                let index = self.lower_expr(index, Some(&Ty::Int(IntTy::Int)))?;
                let value = self.lower_expr(value, Some(&element_ty))?;
                let op = assignment_op(token, source)?;
                if op != hir::AssignOp::Set {
                    super::expressions::validate_binary_operator(
                        super::expressions::assignment_binary_op(op),
                        &element_ty,
                        source,
                    )?;
                }
                Ok(hir::StmtKind::SliceAssign {
                    slice: container,
                    index,
                    op,
                    value,
                })
            }
            Ty::Map(key, element)
                if key.underlying() == &Ty::String
                    && element.underlying() == &Ty::Int(IntTy::Int) =>
            {
                if assignment_op(token, source)? != hir::AssignOp::Set {
                    return Err(Diagnostic::unsupported(
                        "compound map assignment is not yet implemented",
                        source,
                    ));
                }
                let key = self.lower_expr(index, Some(&Ty::String))?;
                let value = self.lower_expr(value, Some(&Ty::Int(IntTy::Int)))?;
                Ok(hir::StmtKind::MapAssign {
                    map: container,
                    key,
                    value,
                })
            }
            _ => Err(Diagnostic::semantic(
                "indexed assignment requires []int or map[string]int",
                source,
            )),
        }
    }

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
                    let container = self.lower_expr(base, None)?;
                    match container.ty.underlying() {
                        Ty::Slice(element) if element.underlying() == &Ty::Int(IntTy::Int) => {
                            let element_ty = element.as_ref().clone();
                            let index = self.lower_expr(index, Some(&Ty::Int(IntTy::Int)))?;
                            destinations.push(hir::AssignTarget::SliceIndex {
                                slice: container,
                                index,
                            });
                            destination_types.push(Some(element_ty));
                        }
                        Ty::Map(_, _)
                            if container.ty.underlying() == string_i64_map_ty().underlying() =>
                        {
                            let key = self.lower_expr(index, Some(&Ty::String))?;
                            destinations.push(hir::AssignTarget::MapIndex {
                                map: container,
                                key,
                            });
                            destination_types.push(Some(Ty::Int(IntTy::Int)));
                        }
                        _ => {
                            return Err(Diagnostic::semantic(
                                "indexed assignment requires []int or map[string]int",
                                source,
                            ));
                        }
                    }
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
