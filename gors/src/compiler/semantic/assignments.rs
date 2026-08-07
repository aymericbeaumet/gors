//! Typed assignment preparation over semantic assignment targets.

use std::collections::BTreeSet;

use super::FunctionLowerer;
use super::expressions::{
    assignment_binary_op, coerce_expr, default_expr_type, ensure_bootstrap_value_type,
    validate_binary_operator,
};
use super::shifts::prepare_shift_count;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{ExprSyntax, ExprSyntaxKind};
use crate::compiler::types::{ConstValue, Ty};
use crate::token::Token;

impl FunctionLowerer {
    pub(super) fn lower_assignment(
        &mut self,
        left: &[ExprSyntax],
        token: Token,
        right: &[ExprSyntax],
        source: SourceRef,
    ) -> Result<hir::StmtKind, Diagnostic> {
        if let Some(binding) = self.try_lower_closure_binding(left, token, right, source) {
            return binding;
        }
        if token == Token::DEFINE {
            return self.lower_short_declaration(left, right, source);
        }
        if left.len() != right.len() {
            return self.lower_tuple_result_assignment(left, token, right, source);
        }

        let op = assignment_op(token, source)?;
        if op != hir::AssignOp::Set && left.len() != 1 {
            return Err(Diagnostic::semantic(
                "compound assignment requires one destination and one value",
                source,
            ));
        }
        let destinations = self.lower_assignment_targets(left, source)?;
        if op != hir::AssignOp::Set {
            let ty = destinations
                .first()
                .and_then(|target| target.ty.as_ref())
                .ok_or_else(|| {
                    Diagnostic::semantic(
                        "compound assignment requires a non-blank destination",
                        source,
                    )
                })?;
            validate_binary_operator(assignment_binary_op(op), ty, source)?;
        }
        let values = right
            .iter()
            .zip(&destinations)
            .map(|(expression, target)| match &target.ty {
                Some(expected) => self.lower_assignment_operand(expression, expected, op, source),
                None => self.lower_expr(expression, None).and_then(|expression| {
                    let value_source = expression.source;
                    default_expr_type(expression, value_source)
                }),
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(hir::StmtKind::Assign {
            destinations,
            op,
            values,
        })
    }

    pub(super) fn lower_inc_dec(
        &mut self,
        expression: &ExprSyntax,
        token: Token,
        source: SourceRef,
    ) -> Result<hir::StmtKind, Diagnostic> {
        let op = match token {
            Token::INC => hir::AssignOp::Add,
            Token::DEC => hir::AssignOp::Sub,
            _ => {
                return Err(Diagnostic::semantic(
                    "invalid increment/decrement token",
                    source,
                ));
            }
        };
        let target = self.lower_assignment_target(expression, source)?;
        let ty = target.ty.clone().ok_or_else(|| {
            Diagnostic::semantic(
                "increment and decrement require a non-blank operand",
                source,
            )
        })?;
        if !matches!(ty.underlying(), Ty::Int(_) | Ty::Uint(_)) {
            return Err(Diagnostic::semantic(
                "increment and decrement require an integer operand",
                source,
            ));
        }
        validate_binary_operator(assignment_binary_op(op), &ty, source)?;
        let one_node = self.alloc_node(expression.source)?;
        let one = hir::Expr {
            node: one_node,
            kind: hir::ExprKind::Constant(ConstValue::Int("1".into())),
            ty,
            category: hir::ValueCategory::Constant,
            effects: hir::Effects::default(),
            source: SourceRef::node(one_node),
        };
        Ok(hir::StmtKind::Assign {
            destinations: vec![target],
            op,
            values: vec![one],
        })
    }

    fn lower_short_declaration(
        &mut self,
        left: &[ExprSyntax],
        right: &[ExprSyntax],
        source: SourceRef,
    ) -> Result<hir::StmtKind, Diagnostic> {
        if left.len() != right.len() {
            if let [value] = right {
                let value = self.lower_multi_result_expression(value)?;
                let Ty::Tuple(component_types) = &value.ty else {
                    return Err(Diagnostic::semantic(
                        format!(
                            "assignment has {} destinations and {} values",
                            left.len(),
                            right.len()
                        ),
                        source,
                    ));
                };
                let component_types = component_types.clone();
                let (destinations, coercions, declares) = self.lower_multi_result_destinations(
                    left,
                    Token::DEFINE,
                    &component_types,
                    source,
                )?;
                debug_assert!(declares);
                return Ok(hir::StmtKind::LetTuple {
                    destinations,
                    value,
                    coercions,
                });
            }
            return Err(Diagnostic::unsupported(
                "this multi-result short declaration form is not yet supported",
                source,
            ));
        }

        let mut names = BTreeSet::new();
        for expression in left {
            let ExprSyntaxKind::Ident(name) = &expression.kind else {
                return Err(Diagnostic::semantic(
                    "short declaration target must be an identifier",
                    source,
                ));
            };
            if name.name.as_ref() != "_" && !names.insert(name.name.as_ref()) {
                return Err(Diagnostic::semantic(
                    format!("{} appears more than once on the left of :=", name.name),
                    source,
                ));
            }
        }
        // Resolve all right-hand sides before any new binding enters scope.
        let mut values = right
            .iter()
            .map(|expression| self.lower_expr(expression, None))
            .collect::<Result<Vec<_>, _>>()?;
        let mut destinations = Vec::new();
        let mut introduced = false;
        for (expression, value) in left.iter().zip(&mut values) {
            let ExprSyntaxKind::Ident(name) = &expression.kind else {
                return Err(Diagnostic::backend(
                    "validated short declaration target changed shape",
                ));
            };
            if name.name.as_ref() == "_" {
                destinations.push(hir::Place::Discard);
                continue;
            }
            if let Some(local) = self.lookup_current_local(&name.name) {
                let ty = self.place_ty(hir::Place::Local(local))?.clone();
                coerce_expr(value, &ty, value.source)?;
                destinations.push(hir::Place::Local(local));
            } else {
                introduced = true;
                let ty = value.ty.default_typed();
                ensure_bootstrap_value_type(&ty, value.source)?;
                coerce_expr(value, &ty, value.source)?;
                let local = self.alloc_local(
                    Some(name.name.to_string()),
                    ty,
                    hir::LocalKind::Variable,
                    name.source,
                )?;
                destinations.push(hir::Place::Local(local));
            }
        }
        if !introduced {
            return Err(Diagnostic::semantic(
                "short declaration introduces no new variables",
                source,
            ));
        }
        Ok(hir::StmtKind::Let {
            destinations,
            values,
        })
    }

    fn lower_tuple_result_assignment(
        &mut self,
        left: &[ExprSyntax],
        token: Token,
        right: &[ExprSyntax],
        source: SourceRef,
    ) -> Result<hir::StmtKind, Diagnostic> {
        if token != Token::ASSIGN {
            return Err(Diagnostic::semantic(
                "multi-result assignment requires = or :=",
                source,
            ));
        }
        let [value] = right else {
            return Err(Diagnostic::unsupported(
                "this multi-result assignment form is not yet supported",
                source,
            ));
        };
        let value = self.lower_multi_result_expression(value)?;
        let Ty::Tuple(component_types) = &value.ty else {
            return Err(Diagnostic::semantic(
                format!(
                    "assignment has {} destinations and {} values",
                    left.len(),
                    right.len()
                ),
                source,
            ));
        };
        if component_types.len() != left.len() {
            return Err(Diagnostic::semantic(
                format!(
                    "assignment has {} destinations and {} result values",
                    left.len(),
                    component_types.len()
                ),
                source,
            ));
        }
        if !matches!(
            value.kind,
            hir::ExprKind::Call { .. } | hir::ExprKind::ForwardedCall { .. }
        ) {
            return Err(Diagnostic::backend(
                "tuple-valued non-call reached multi-result assignment",
            ));
        }
        let destinations = self.lower_assignment_targets(left, source)?;
        let coercions = self.assignment_target_coercions(&destinations, component_types, source)?;
        Ok(hir::StmtKind::AssignTuple {
            destinations,
            value,
            coercions,
        })
    }

    /// Lower forms that acquire multiple values specifically in assignment
    /// and variable-declaration contexts. The tuple remains one HIR expression.
    pub(super) fn lower_multi_result_expression(
        &mut self,
        expression: &ExprSyntax,
    ) -> Result<hir::Expr, Diagnostic> {
        match self.try_lower_channel_comma_ok(expression) {
            Some(value) => value,
            None => match self.try_lower_interface_comma_ok(expression) {
                Some(value) => value,
                None => match self.try_lower_map_comma_ok(expression) {
                    Some(value) => value,
                    None => self.lower_expr(expression, None),
                },
            },
        }
    }

    /// Identifier-only destination lowering retained for declarations and the
    /// current select receive surface. General assignment uses `AssignTarget`.
    pub(super) fn lower_multi_result_destinations(
        &mut self,
        left: &[ExprSyntax],
        token: Token,
        component_types: &[Ty],
        source: SourceRef,
    ) -> Result<(Vec<hir::Place>, Vec<hir::ValueCoercion>, bool), Diagnostic> {
        if component_types.len() != left.len() {
            return Err(Diagnostic::semantic(
                format!(
                    "assignment has {} destinations and {} result values",
                    left.len(),
                    component_types.len()
                ),
                source,
            ));
        }
        if token == Token::DEFINE {
            let mut names = BTreeSet::new();
            let mut destinations = Vec::with_capacity(left.len());
            let mut coercions = Vec::with_capacity(left.len());
            let mut introduced = false;
            for ((expression, ty), index) in left.iter().zip(component_types).zip(0..) {
                let ExprSyntaxKind::Ident(name) = &expression.kind else {
                    return Err(Diagnostic::semantic(
                        "short declaration target must be an identifier",
                        source,
                    ));
                };
                if name.name.as_ref() != "_" && !names.insert(name.name.as_ref()) {
                    return Err(Diagnostic::semantic(
                        format!("{} appears more than once on the left of :=", name.name),
                        source,
                    ));
                }
                if name.name.as_ref() == "_" {
                    destinations.push(hir::Place::Discard);
                    coercions.push(hir::ValueCoercion::Identity);
                } else if let Some(local) = self.lookup_current_local(&name.name) {
                    let destination_ty = self.place_ty(hir::Place::Local(local))?;
                    coercions.push(self.assignment_value_coercion(ty, destination_ty, source).map_err(
                        |_| {
                            Diagnostic::semantic(
                                format!(
                                    "result {index} of type {ty:?} is not assignable to {destination_ty:?}"
                                ),
                                source,
                            )
                        },
                    )?);
                    destinations.push(hir::Place::Local(local));
                } else {
                    introduced = true;
                    ensure_bootstrap_value_type(ty, source)?;
                    let local = self.alloc_local(
                        Some(name.name.to_string()),
                        ty.clone(),
                        hir::LocalKind::Variable,
                        name.source,
                    )?;
                    destinations.push(hir::Place::Local(local));
                    coercions.push(hir::ValueCoercion::Identity);
                }
            }
            if !introduced {
                return Err(Diagnostic::semantic(
                    "short declaration introduces no new variables",
                    source,
                ));
            }
            return Ok((destinations, coercions, true));
        }
        if token != Token::ASSIGN {
            return Err(Diagnostic::semantic(
                "multi-result assignment requires = or :=",
                source,
            ));
        }
        let destinations = left
            .iter()
            .map(|expression| self.lower_place(expression, source))
            .collect::<Result<Vec<_>, _>>()?;
        let coercions = destinations
            .iter()
            .zip(component_types)
            .enumerate()
            .map(|(index, (destination, result_ty))| match destination {
                hir::Place::Discard => Ok(hir::ValueCoercion::Identity),
                hir::Place::Local(_) => {
                    let destination_ty = self.place_ty(*destination)?;
                    self.assignment_value_coercion(result_ty, destination_ty, source)
                        .map_err(|_| {
                            Diagnostic::semantic(
                                format!(
                                    "result {index} of type {result_ty:?} is not assignable to {destination_ty:?}"
                                ),
                                source,
                            )
                        })
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok((destinations, coercions, false))
    }

    pub(super) fn assignment_target_coercions(
        &self,
        destinations: &[hir::AssignTarget],
        component_types: &[Ty],
        source: SourceRef,
    ) -> Result<Vec<hir::ValueCoercion>, Diagnostic> {
        if destinations.len() != component_types.len() {
            return Err(Diagnostic::semantic(
                format!(
                    "assignment has {} destinations and {} result values",
                    destinations.len(),
                    component_types.len()
                ),
                source,
            ));
        }
        destinations
            .iter()
            .zip(component_types)
            .enumerate()
            .map(|(index, (destination, result_ty))| match &destination.ty {
                None => Ok(hir::ValueCoercion::Identity),
                Some(destination_ty) => self
                    .assignment_value_coercion(result_ty, destination_ty, source)
                    .map_err(|_| {
                        Diagnostic::semantic(
                            format!(
                                "result {index} of type {result_ty:?} is not assignable to {destination_ty:?}"
                            ),
                            source,
                        )
                    }),
            })
            .collect()
    }

    pub(super) fn lower_place(
        &self,
        expression: &ExprSyntax,
        source: SourceRef,
    ) -> Result<hir::Place, Diagnostic> {
        let ExprSyntaxKind::Ident(ident) = &expression.kind else {
            return Err(Diagnostic::unsupported(
                "only local identifier declaration targets are implemented",
                source,
            ));
        };
        if ident.name.as_ref() == "_" {
            return Ok(hir::Place::Discard);
        }
        if self.variables.contains_key(ident.name.as_ref()) {
            return Err(Diagnostic::unsupported(
                "package variable mutation requires global storage lowering",
                source,
            ));
        }
        if self.lookup_local_constant(&ident.name).is_some()
            || self.constants.contains_key(ident.name.as_ref())
        {
            return Err(Diagnostic::semantic(
                format!("cannot assign to constant {}", ident.name),
                source,
            ));
        }
        self.lookup_local(&ident.name)
            .map(hir::Place::Local)
            .ok_or_else(|| {
                Diagnostic::semantic(format!("undefined variable {}", ident.name), source)
            })
    }

    pub(super) fn place_ty(&self, place: hir::Place) -> Result<&Ty, Diagnostic> {
        match place {
            hir::Place::Local(id) => self
                .locals
                .get(id.0 as usize)
                .map(|local| &local.ty)
                .ok_or_else(|| Diagnostic::backend(format!("invalid local id {}", id.0))),
            hir::Place::Discard => Err(Diagnostic::backend(
                "blank identifier unexpectedly required an inferred type",
            )),
        }
    }

    pub(super) fn lower_assignment_operand(
        &mut self,
        expression: &ExprSyntax,
        expected: &Ty,
        op: hir::AssignOp,
        source: SourceRef,
    ) -> Result<hir::Expr, Diagnostic> {
        if matches!(op, hir::AssignOp::Shl | hir::AssignOp::Shr) {
            let mut value = self.lower_expr(expression, None)?;
            prepare_shift_count(&mut value, source)?;
            Ok(value)
        } else {
            self.lower_expr(expression, Some(expected))
        }
    }
}

pub(super) fn assignment_op(token: Token, source: SourceRef) -> Result<hir::AssignOp, Diagnostic> {
    match token {
        Token::ASSIGN => Ok(hir::AssignOp::Set),
        Token::ADD_ASSIGN => Ok(hir::AssignOp::Add),
        Token::SUB_ASSIGN => Ok(hir::AssignOp::Sub),
        Token::MUL_ASSIGN => Ok(hir::AssignOp::Mul),
        Token::QUO_ASSIGN => Ok(hir::AssignOp::Div),
        Token::REM_ASSIGN => Ok(hir::AssignOp::Rem),
        Token::AND_ASSIGN => Ok(hir::AssignOp::BitAnd),
        Token::OR_ASSIGN => Ok(hir::AssignOp::BitOr),
        Token::XOR_ASSIGN => Ok(hir::AssignOp::BitXor),
        Token::SHL_ASSIGN => Ok(hir::AssignOp::Shl),
        Token::SHR_ASSIGN => Ok(hir::AssignOp::Shr),
        Token::AND_NOT_ASSIGN => Ok(hir::AssignOp::AndNot),
        _ => Err(Diagnostic::semantic(
            format!("invalid assignment operator {token:?}"),
            source,
        )),
    }
}
