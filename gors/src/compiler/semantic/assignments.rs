//! Typed preparation of assignments with dynamic left-hand-side operands.

use std::collections::BTreeSet;

use super::FunctionLowerer;
use super::expressions::{
    assignment_binary_op, coerce_expr, default_expr_type, ensure_bootstrap_value_type,
    is_assignable, validate_binary_operator,
};
use super::maps::string_i64_map_ty;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{ExprSyntax, ExprSyntaxKind};
use crate::compiler::types::{IntTy, Ty, UintTy};
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
        if let Some(assignment) =
            self.try_lower_parallel_dynamic_assignment(left, token, right, source)
        {
            return assignment;
        }
        if let Some(assignment) = self.try_lower_single_index_assignment(left, token, right, source)
        {
            return assignment;
        }
        if let Some(assignment) =
            self.try_lower_single_struct_field_assignment(left, token, right, source)
        {
            return assignment;
        }
        if let Some(assignment) = self.try_lower_pointer_assignment(left, token, right, source) {
            return assignment;
        }
        if left.len() != right.len() {
            if let [value] = right {
                let value = match self.try_lower_channel_comma_ok(value) {
                    Some(value) => value?,
                    None => match self.try_lower_interface_comma_ok(value) {
                        Some(value) => value?,
                        None => match self.try_lower_map_comma_ok(value) {
                            Some(value) => value?,
                            None => self.lower_expr(value, None)?,
                        },
                    },
                };
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
                if !matches!(value.kind, hir::ExprKind::Call { .. }) {
                    return Err(Diagnostic::backend(
                        "tuple-valued non-call reached multi-result assignment",
                    ));
                }
                if token == Token::ASSIGN
                    && left
                        .iter()
                        .any(|expression| !matches!(expression.kind, ExprSyntaxKind::Ident(_)))
                {
                    return self.lower_parallel_tuple_assignment(left, value, source);
                }
                let (destinations, coercions, declares) =
                    self.lower_multi_result_destinations(left, token, component_types, source)?;
                return Ok(if declares {
                    hir::StmtKind::LetTuple {
                        destinations,
                        value,
                        coercions,
                    }
                } else {
                    hir::StmtKind::AssignTuple {
                        destinations,
                        value,
                        coercions,
                    }
                });
            }
            return Err(Diagnostic::unsupported(
                "this multi-result assignment form is not yet supported",
                source,
            ));
        }
        if token == Token::DEFINE {
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
            // Go resolves every right-hand side before adding new bindings.
            let mut values = right
                .iter()
                .map(|expression| self.lower_expr(expression, None))
                .collect::<Result<Vec<_>, _>>()?;
            let mut destinations = Vec::new();
            let mut introduced = false;
            for (expression, value) in left.iter().zip(&mut values) {
                let ExprSyntaxKind::Ident(name) = &expression.kind else {
                    return Err(Diagnostic::semantic(
                        "short declaration target must be an identifier",
                        source,
                    ));
                };
                if name.name.as_ref() == "_" {
                    destinations.push(hir::Place::Discard);
                    continue;
                }
                if let Some(local) = self.lookup_current_local(&name.name) {
                    let ty = self.place_ty(hir::Place::Local(local))?.clone();
                    let value_source = value.source;
                    coerce_expr(value, &ty, value_source)?;
                    destinations.push(hir::Place::Local(local));
                } else {
                    introduced = true;
                    let ty = value.ty.default_typed();
                    let value_source = value.source;
                    ensure_bootstrap_value_type(&ty, value_source)?;
                    coerce_expr(value, &ty, value_source)?;
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
            return Ok(hir::StmtKind::Let {
                destinations,
                values,
            });
        }

        let destinations = left
            .iter()
            .map(|expression| self.lower_place(expression, source))
            .collect::<Result<Vec<_>, _>>()?;
        let destination_types = destinations
            .iter()
            .map(|destination| match destination {
                hir::Place::Local(_) => self.place_ty(*destination).cloned().map(Some),
                hir::Place::Discard => Ok(None),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let values = right
            .iter()
            .zip(&destination_types)
            .map(|(expression, expected)| match expected {
                Some(expected) => self.lower_expr(expression, Some(expected)),
                None => self.lower_expr(expression, None).and_then(|expression| {
                    let source = expression.source;
                    default_expr_type(expression, source)
                }),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let op = assignment_op(token, source)?;
        if op != hir::AssignOp::Set && destinations.len() != 1 {
            return Err(Diagnostic::semantic(
                "compound assignment requires one destination and one value",
                source,
            ));
        }
        if op != hir::AssignOp::Set {
            let ty = destination_types
                .first()
                .and_then(Option::as_ref)
                .ok_or_else(|| {
                    Diagnostic::semantic(
                        "compound assignment requires a non-blank destination",
                        source,
                    )
                })?;
            validate_binary_operator(assignment_binary_op(op), ty, source)?;
        }
        Ok(hir::StmtKind::Assign {
            destinations,
            op,
            values,
        })
    }

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
            for ((expression, ty), index) in left
                .iter()
                .zip(component_types)
                .zip(0..component_types.len())
            {
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
                    let coercion = self
                        .assignment_value_coercion(ty, destination_ty, source)
                        .map_err(|_| {
                            Diagnostic::semantic(
                                format!(
                                    "result {index} of type {ty:?} is not assignable to {destination_ty:?}"
                                ),
                                source,
                            )
                        })?;
                    destinations.push(hir::Place::Local(local));
                    coercions.push(coercion);
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

    pub(super) fn try_lower_single_struct_field_assignment(
        &mut self,
        left: &[ExprSyntax],
        token: Token,
        right: &[ExprSyntax],
        source: SourceRef,
    ) -> Option<Result<hir::StmtKind, Diagnostic>> {
        let (
            [
                ExprSyntax {
                    kind: ExprSyntaxKind::Selector { base, member },
                    ..
                },
            ],
            [value],
        ) = (left, right)
        else {
            return None;
        };
        Some((|| {
            if token == Token::DEFINE {
                return Err(Diagnostic::semantic(
                    "short declaration target must be an identifier",
                    source,
                ));
            }
            let node = self.alloc_node(base.source)?;
            let target = self.lower_selector(base, member, node, source)?;
            let (structure, field) = match target.kind {
                hir::ExprKind::StructField { structure, field } => (*structure, field),
                hir::ExprKind::Call {
                    callee: hir::Callee::Builtin(hir::Builtin::PointerStructI64Get),
                    args,
                } => {
                    let mut args = args.into_iter();
                    let structure = args.next().ok_or_else(|| {
                        Diagnostic::backend("pointer field selector omitted its receiver")
                    })?;
                    let index = args.next().ok_or_else(|| {
                        Diagnostic::backend("pointer field selector omitted its field index")
                    })?;
                    if args.next().is_some() {
                        return Err(Diagnostic::backend(
                            "pointer field selector has excess operands",
                        ));
                    }
                    let hir::ExprKind::Constant(crate::compiler::types::ConstValue::Int(index)) =
                        index.kind
                    else {
                        return Err(Diagnostic::backend(
                            "pointer field selector has a dynamic field index",
                        ));
                    };
                    let field = index.parse::<u32>().map_err(|_| {
                        Diagnostic::backend("pointer field selector index does not fit u32")
                    })?;
                    (structure, field)
                }
                _ => {
                    return Err(Diagnostic::backend(
                        "struct selector assignment did not lower to a field",
                    ));
                }
            };
            let hir::ExprKind::Local(structure) = structure.kind else {
                return Err(Diagnostic::unsupported(
                    "struct field assignment currently requires a local struct or struct pointer",
                    source,
                ));
            };
            let value = self.lower_expr(value, Some(&target.ty))?;
            let op = assignment_op(token, source)?;
            if op != hir::AssignOp::Set {
                super::expressions::validate_binary_operator(
                    super::expressions::assignment_binary_op(op),
                    &target.ty,
                    source,
                )?;
            }
            Ok(hir::StmtKind::StructFieldAssign {
                structure,
                field,
                op,
                value,
            })
        })())
    }

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

    pub(super) fn lower_single_index_assignment(
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
            Ty::Array(_, element) if super::arrays::is_scalar_array_element(element) => {
                self.lower_array_assignment(container, index, token, value, source)
            }
            Ty::Slice(element)
                if matches!(
                    element.underlying(),
                    Ty::Int(IntTy::Int | IntTy::Int32) | Ty::Uint(UintTy::Uint8) | Ty::Bool
                ) =>
            {
                let element_ty = element.as_ref().clone();
                let set = if element.underlying() == &Ty::Bool {
                    hir::Builtin::SliceBoolSet
                } else if element.underlying() == &Ty::Uint(UintTy::Uint8) {
                    hir::Builtin::SliceU8Set
                } else {
                    hir::Builtin::SliceI64Set
                };
                let index = self.lower_expr(index, Some(&Ty::Int(IntTy::Int)))?;
                let value = self.lower_expr(value, Some(&element_ty))?;
                let op = assignment_op(token, source)?;
                if element.underlying() == &Ty::Uint(UintTy::Uint8) && op != hir::AssignOp::Set {
                    return Err(Diagnostic::unsupported(
                        "compound []byte assignment requires uint8 wrapping semantics",
                        source,
                    ));
                }
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
                    set,
                    op,
                    value,
                })
            }
            Ty::Map(key, element)
                if key.underlying() == &Ty::String
                    && element.underlying() == &Ty::Int(IntTy::Int) =>
            {
                let op = assignment_op(token, source)?;
                if op != hir::AssignOp::Set {
                    super::expressions::validate_binary_operator(
                        super::expressions::assignment_binary_op(op),
                        &Ty::Int(IntTy::Int),
                        source,
                    )?;
                }
                let key = self.lower_expr(index, Some(&Ty::String))?;
                let value = self.lower_expr(value, Some(&Ty::Int(IntTy::Int)))?;
                Ok(hir::StmtKind::MapAssign {
                    map: container,
                    key,
                    op,
                    value,
                })
            }
            _ => Err(Diagnostic::semantic(
                "indexed assignment requires []bool, []byte, []int, or map[string]int",
                source,
            )),
        }
    }

    pub(super) fn try_lower_parallel_dynamic_assignment(
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
                .any(|expression| !matches!(expression.kind, ExprSyntaxKind::Ident(_)))
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
        let (destinations, destination_types) =
            self.lower_parallel_assignment_targets(left, source)?;
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

    pub(super) fn lower_parallel_tuple_assignment(
        &mut self,
        left: &[ExprSyntax],
        value: hir::Expr,
        source: SourceRef,
    ) -> Result<hir::StmtKind, Diagnostic> {
        let Ty::Tuple(component_types) = &value.ty else {
            return Err(Diagnostic::backend(
                "parallel tuple assignment value is not a tuple",
            ));
        };
        if component_types.len() != left.len() {
            return Err(Diagnostic::backend(
                "parallel tuple assignment arity changed during semantic lowering",
            ));
        }
        let (destinations, destination_types) =
            self.lower_parallel_assignment_targets(left, source)?;
        let coercions = destination_types
            .iter()
            .zip(component_types)
            .enumerate()
            .map(|(index, (destination_ty, result_ty))| match destination_ty {
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
            .collect::<Result<Vec<_>, _>>()?;
        Ok(hir::StmtKind::ParallelAssignTuple {
            destinations,
            value,
            coercions,
        })
    }

    fn lower_parallel_assignment_targets(
        &mut self,
        left: &[ExprSyntax],
        source: SourceRef,
    ) -> Result<(Vec<hir::AssignTarget>, Vec<Option<Ty>>), Diagnostic> {
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
                        Ty::Slice(element)
                            if matches!(
                                element.underlying(),
                                Ty::Int(IntTy::Int | IntTy::Int32)
                                    | Ty::Uint(UintTy::Uint8)
                                    | Ty::Bool
                            ) =>
                        {
                            let element_ty = element.as_ref().clone();
                            let set = if element.underlying() == &Ty::Bool {
                                hir::Builtin::SliceBoolSet
                            } else if element.underlying() == &Ty::Uint(UintTy::Uint8) {
                                hir::Builtin::SliceU8Set
                            } else {
                                hir::Builtin::SliceI64Set
                            };
                            let index = self.lower_expr(index, Some(&Ty::Int(IntTy::Int)))?;
                            destinations.push(hir::AssignTarget::SliceIndex {
                                slice: container,
                                index,
                                set,
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
                                "indexed assignment requires []bool, []byte, []int, or map[string]int",
                                source,
                            ));
                        }
                    }
                }
                ExprSyntaxKind::Unary {
                    token: Token::MUL,
                    expression: pointer,
                } => {
                    let pointer = self.lower_expr(pointer, None)?;
                    let Ty::Pointer(element) = pointer.ty.underlying() else {
                        return Err(Diagnostic::semantic(
                            "indirect assignment requires a pointer",
                            source,
                        ));
                    };
                    if element.underlying() != &Ty::Int(IntTy::Int) {
                        return Err(Diagnostic::unsupported(
                            "parallel pointer assignment currently supports *int",
                            source,
                        ));
                    }
                    let destination_ty = element.as_ref().clone();
                    destinations.push(hir::AssignTarget::Pointer {
                        pointer,
                        set: hir::Builtin::PointerI64Set,
                    });
                    destination_types.push(Some(destination_ty));
                }
                ExprSyntaxKind::Selector { base, member } => {
                    let node = self.alloc_node(expression.source)?;
                    let target = self.lower_selector(base, member, node, source)?;
                    let destination_ty = target.ty.clone();
                    match target.kind {
                        hir::ExprKind::StructField { structure, field } => {
                            let hir::ExprKind::Local(structure) = structure.kind else {
                                return Err(Diagnostic::unsupported(
                                    "parallel struct field assignment currently requires a local struct",
                                    source,
                                ));
                            };
                            destinations.push(hir::AssignTarget::StructField { structure, field });
                        }
                        hir::ExprKind::Call {
                            callee: hir::Callee::Builtin(hir::Builtin::PointerStructI64Get),
                            args,
                        } => {
                            let mut args = args.into_iter();
                            let pointer = args.next().ok_or_else(|| {
                                Diagnostic::backend("pointer field selector omitted its receiver")
                            })?;
                            let index = args.next().ok_or_else(|| {
                                Diagnostic::backend(
                                    "pointer field selector omitted its field index",
                                )
                            })?;
                            if args.next().is_some() {
                                return Err(Diagnostic::backend(
                                    "pointer field selector has excess operands",
                                ));
                            }
                            let hir::ExprKind::Constant(crate::compiler::types::ConstValue::Int(
                                index,
                            )) = index.kind
                            else {
                                return Err(Diagnostic::backend(
                                    "pointer field selector has a dynamic field index",
                                ));
                            };
                            let field = index.parse::<u32>().map_err(|_| {
                                Diagnostic::backend("pointer field selector index does not fit u32")
                            })?;
                            destinations.push(hir::AssignTarget::PointerStructField {
                                pointer,
                                field,
                                set: hir::Builtin::PointerStructI64Set,
                            });
                        }
                        _ => {
                            return Err(Diagnostic::backend(
                                "struct selector assignment did not lower to a field",
                            ));
                        }
                    }
                    destination_types.push(Some(destination_ty));
                }
                _ => {
                    return Err(Diagnostic::unsupported(
                        "this assignment target is not yet implemented",
                        source,
                    ));
                }
            }
        }
        Ok((destinations, destination_types))
    }

    pub(super) fn lower_place(
        &self,
        expression: &ExprSyntax,
        source: SourceRef,
    ) -> Result<hir::Place, Diagnostic> {
        let ExprSyntaxKind::Ident(ident) = &expression.kind else {
            return Err(Diagnostic::unsupported(
                "only local identifier assignment targets are implemented",
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
