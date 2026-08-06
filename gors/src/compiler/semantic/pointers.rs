//! Typed lowering for Go pointers to executable integer values and structs.

use super::FunctionLowerer;
use super::channels::int_channel_parts;
use super::expressions::coerce_expr;
use super::lower_type;
use super::maps::string_i64_map_ty;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::NodeId;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{ExprSyntax, ExprSyntaxKind};
use crate::compiler::types::{IntTy, Ty};
use crate::token::Token;

pub(super) fn int_pointer_ty() -> Ty {
    Ty::Pointer(Box::new(Ty::Int(IntTy::Int)))
}

impl FunctionLowerer {
    pub(super) fn lower_struct_pointer_comparison(
        &mut self,
        mut left: hir::Expr,
        mut right: hir::Expr,
        equal: bool,
        node: NodeId,
        source: SourceRef,
        expected: Option<&Ty>,
    ) -> Result<hir::Expr, Diagnostic> {
        if left.ty.bootstrap_i64_struct_pointer_fields().is_none()
            || right.ty.bootstrap_i64_struct_pointer_fields().is_none()
            || left.ty != right.ty
        {
            return Err(Diagnostic::semantic(
                format!(
                    "incompatible pointer comparison operands {:?} and {:?}",
                    left.ty, right.ty
                ),
                source,
            ));
        }
        let pointer_ty = left.ty.clone();
        coerce_expr(&mut left, &pointer_ty, source)?;
        coerce_expr(&mut right, &pointer_ty, source)?;
        let effects = pointer_effects(&[&left, &right], false, false, false);
        let call = hir::Expr {
            node,
            kind: hir::ExprKind::Call {
                callee: hir::Callee::Builtin(hir::Builtin::PointerStructI64Equal),
                args: vec![left, right],
            },
            ty: Ty::Bool,
            category: hir::ValueCategory::Value,
            effects,
            source,
        };
        let mut result = if equal {
            call
        } else {
            hir::Expr {
                node,
                kind: hir::ExprKind::Unary {
                    op: hir::UnaryOp::Not,
                    operand: Box::new(call),
                },
                ty: Ty::Bool,
                category: hir::ValueCategory::Value,
                effects,
                source,
            }
        };
        if let Some(expected) = expected {
            coerce_expr(&mut result, expected, source)?;
        }
        Ok(result)
    }

    pub(super) fn lower_address_of_local(
        &mut self,
        expression: &ExprSyntax,
        node: NodeId,
        source: SourceRef,
        expected: Option<&Ty>,
    ) -> Result<hir::Expr, Diagnostic> {
        if self.defer_registration_depth != 0
            || self.inside_deferred_closure
            || self.inside_local_closure
        {
            return Err(Diagnostic::unsupported(
                "address-taking in nested control flow or function literals is not yet represented",
                source,
            ));
        }
        if matches!(expression.kind, ExprSyntaxKind::CompositeLiteral { .. }) {
            let value = self.lower_expr(expression, None)?;
            if value.ty.bootstrap_i64_struct_fields().is_none()
                && !value.ty.uses_interface_aggregate_pointer_representation()
            {
                return Err(Diagnostic::unsupported(
                    "address-taking currently supports integer struct composite literals",
                    source,
                ));
            }
            let ty = Ty::Pointer(Box::new(value.ty.clone()));
            let effects = pointer_effects(&[&value], true, true, false);
            let mut result = hir::Expr {
                node,
                kind: hir::ExprKind::AddressOfValue(Box::new(value)),
                ty,
                category: hir::ValueCategory::Value,
                effects,
                source,
            };
            if let Some(expected) = expected {
                coerce_expr(&mut result, expected, source)?;
            }
            return Ok(result);
        }
        let ExprSyntaxKind::Ident(identifier) = &expression.kind else {
            return Err(Diagnostic::unsupported(
                "address-taking currently requires a local identifier or struct composite literal",
                source,
            ));
        };
        let Some(local) = self.lookup_local(&identifier.name) else {
            if self.variables.contains_key(identifier.name.as_ref()) {
                return Err(Diagnostic::unsupported(
                    "taking the address of a package variable requires global storage lowering",
                    source,
                ));
            }
            return Err(Diagnostic::semantic(
                format!("undefined variable {}", identifier.name),
                source,
            ));
        };
        let element = self
            .locals
            .get(local.0 as usize)
            .map(|local| local.ty.clone())
            .ok_or_else(|| Diagnostic::backend(format!("invalid local id {}", local.0)))?;
        if element.underlying() != &Ty::Int(IntTy::Int)
            && element.bootstrap_i64_struct_fields().is_none()
            && !element.uses_interface_aggregate_pointer_representation()
        {
            return Err(Diagnostic::unsupported(
                "address-taking currently supports integer and integer-struct locals",
                source,
            ));
        }
        let mut result = hir::Expr {
            node,
            kind: hir::ExprKind::AddressOfLocal(local),
            ty: Ty::Pointer(Box::new(element)),
            category: hir::ValueCategory::Value,
            effects: hir::Effects {
                may_allocate: true,
                may_read: true,
                ..hir::Effects::default()
            },
            source,
        };
        if let Some(expected) = expected {
            coerce_expr(&mut result, expected, source)?;
        }
        Ok(result)
    }

    pub(super) fn lower_new_builtin_call(
        &mut self,
        arguments: &[ExprSyntax],
        spread: bool,
        node: NodeId,
        source: SourceRef,
        expected: Option<&Ty>,
    ) -> Result<hir::Expr, Diagnostic> {
        if spread {
            return Err(Diagnostic::semantic(
                "... is not valid in a call to new",
                source,
            ));
        }
        let [element] = arguments else {
            return Err(Diagnostic::semantic(
                "call to new requires exactly one type argument",
                source,
            ));
        };
        let element_source = element.source;
        let element = lower_type(element, &self.type_aliases, source)?;
        let (builtin, args) = if element.underlying() == &Ty::Int(IntTy::Int) {
            (hir::Builtin::PointerI64New, Vec::new())
        } else if let Some(fields) = element.bootstrap_i64_struct_fields() {
            let field_count = self.alloc_node(element_source)?;
            (
                hir::Builtin::PointerStructI64New,
                vec![hir::Expr {
                    node: field_count,
                    kind: hir::ExprKind::Constant(crate::compiler::types::ConstValue::Int(
                        fields.len().to_string(),
                    )),
                    ty: Ty::Int(IntTy::Int),
                    category: hir::ValueCategory::Constant,
                    effects: hir::Effects::default(),
                    source: SourceRef::node(field_count),
                }],
            )
        } else {
            return Err(Diagnostic::unsupported(
                "new currently supports int values and integer structs",
                source,
            ));
        };
        let mut result = hir::Expr {
            node,
            kind: hir::ExprKind::Call {
                callee: hir::Callee::Builtin(builtin),
                args,
            },
            ty: Ty::Pointer(Box::new(element)),
            category: hir::ValueCategory::Value,
            effects: pointer_effects(&[], false, true, false),
            source,
        };
        if let Some(expected) = expected {
            coerce_expr(&mut result, expected, source)?;
        }
        Ok(result)
    }

    pub(super) fn lower_pointer_deref(
        &mut self,
        expression: &ExprSyntax,
        node: NodeId,
        source: SourceRef,
        expected: Option<&Ty>,
    ) -> Result<hir::Expr, Diagnostic> {
        if let Some(result) =
            self.try_lower_unsafe_pointer_roundtrip(expression, node, source, expected)?
        {
            return Ok(result);
        }
        let pointer = self.lower_expr(expression, None)?;
        let Ty::Pointer(element) = pointer.ty.underlying() else {
            return Err(Diagnostic::semantic(
                format!("cannot dereference non-pointer value {:?}", pointer.ty),
                source,
            ));
        };
        if element.bootstrap_i64_struct_fields().is_some()
            || element.uses_interface_aggregate_pointer_representation()
        {
            let ty = element.as_ref().clone();
            let effects = pointer_effects(&[&pointer], false, false, true);
            let mut result = hir::Expr {
                node,
                kind: hir::ExprKind::PointerStructValue(Box::new(pointer)),
                ty,
                category: hir::ValueCategory::Value,
                effects,
                source,
            };
            if let Some(expected) = expected {
                coerce_expr(&mut result, expected, source)?;
            }
            return Ok(result);
        }
        if element.underlying() != &Ty::Int(IntTy::Int) {
            return Err(Diagnostic::unsupported(
                "pointer dereference currently supports *int and integer structs",
                source,
            ));
        }
        let ty = element.as_ref().clone();
        let effects = pointer_effects(&[&pointer], false, false, true);
        let mut result = hir::Expr {
            node,
            kind: hir::ExprKind::Call {
                callee: hir::Callee::Builtin(hir::Builtin::PointerI64Get),
                args: vec![pointer],
            },
            ty,
            category: hir::ValueCategory::Value,
            effects,
            source,
        };
        if let Some(expected) = expected {
            coerce_expr(&mut result, expected, source)?;
        }
        Ok(result)
    }

    pub(super) fn try_lower_pointer_assignment(
        &mut self,
        left: &[ExprSyntax],
        token: Token,
        right: &[ExprSyntax],
        source: SourceRef,
    ) -> Option<Result<hir::StmtKind, Diagnostic>> {
        let (
            [
                ExprSyntax {
                    kind:
                        ExprSyntaxKind::Unary {
                            token: Token::MUL,
                            expression: pointer,
                        },
                    ..
                },
            ],
            [value],
        ) = (left, right)
        else {
            return None;
        };
        Some((|| {
            if token != Token::ASSIGN {
                return Err(Diagnostic::unsupported(
                    "pointer assignment currently requires =",
                    source,
                ));
            }
            let pointer = self.lower_expr(pointer, None)?;
            let Ty::Pointer(element) = pointer.ty.underlying() else {
                return Err(Diagnostic::semantic(
                    "indirect assignment requires a pointer",
                    source,
                ));
            };
            if element.underlying() != &Ty::Int(IntTy::Int) {
                return Err(Diagnostic::unsupported(
                    "pointer assignment currently supports *int",
                    source,
                ));
            }
            let value = self.lower_expr(value, Some(element))?;
            let effects = pointer_effects(&[&pointer, &value], true, false, true);
            Ok(hir::StmtKind::Expr(hir::Expr {
                node: pointer.node,
                kind: hir::ExprKind::Call {
                    callee: hir::Callee::Builtin(hir::Builtin::PointerI64Set),
                    args: vec![pointer, value],
                },
                ty: Ty::Unit,
                category: hir::ValueCategory::Value,
                effects,
                source,
            }))
        })())
    }

    pub(super) fn lower_nil_comparison(
        &mut self,
        expression: &ExprSyntax,
        equal: bool,
        node: NodeId,
        source: SourceRef,
        expected: Option<&Ty>,
    ) -> Result<hir::Expr, Diagnostic> {
        let value = self.lower_expr(expression, None)?;
        let builtin = if let Ty::Slice(element) = value.ty.underlying() {
            if matches!(element.underlying(), Ty::Int(IntTy::Int | IntTy::Int32))
                || element.snapshot_function_result().is_some()
            {
                hir::Builtin::SliceI64IsNil
            } else if element.underlying() == &Ty::Uint(crate::compiler::types::UintTy::Uint8) {
                hir::Builtin::SliceU8IsNil
            } else if element.underlying() == &Ty::Bool {
                hir::Builtin::SliceBoolIsNil
            } else if element.uses_interface_aggregate_representation() {
                hir::Builtin::AggregateSliceIsNil
            } else {
                return Err(Diagnostic::semantic(
                    "nil comparison requires a represented slice type",
                    source,
                ));
            }
        } else if value.ty.underlying() == string_i64_map_ty().underlying() {
            hir::Builtin::MapStringI64IsNil
        } else if matches!(value.ty.underlying(), Ty::Interface(_)) {
            hir::Builtin::InterfaceIsNil
        } else if matches!(value.ty.underlying(), Ty::Function(_)) {
            hir::Builtin::FunctionIsNil
        } else if value.ty.underlying() == int_pointer_ty().underlying() {
            hir::Builtin::PointerI64IsNil
        } else if value.ty.bootstrap_i64_struct_pointer_fields().is_some() {
            hir::Builtin::PointerStructI64IsNil
        } else if matches!(
            value.ty.underlying(),
            Ty::Pointer(element) if element.uses_interface_aggregate_pointer_representation()
        ) {
            hir::Builtin::AggregatePointerIsNil
        } else if int_channel_parts(&value.ty).is_some() {
            hir::Builtin::ChannelI64IsNil
        } else {
            return Err(Diagnostic::semantic(
                "nil comparison requires a nil-capable value",
                source,
            ));
        };
        let effects = pointer_effects(&[&value], false, false, false);
        let call = hir::Expr {
            node,
            kind: hir::ExprKind::Call {
                callee: hir::Callee::Builtin(builtin),
                args: vec![value],
            },
            ty: Ty::Bool,
            category: hir::ValueCategory::Value,
            effects,
            source,
        };
        let mut result = if equal {
            call
        } else {
            hir::Expr {
                node,
                kind: hir::ExprKind::Unary {
                    op: hir::UnaryOp::Not,
                    operand: Box::new(call),
                },
                ty: Ty::Bool,
                category: hir::ValueCategory::Value,
                effects,
                source,
            }
        };
        if let Some(expected) = expected {
            coerce_expr(&mut result, expected, source)?;
        }
        Ok(result)
    }
}

pub(super) fn pointer_effects(
    arguments: &[&hir::Expr],
    writes: bool,
    allocates: bool,
    panics: bool,
) -> hir::Effects {
    arguments.iter().fold(
        hir::Effects {
            may_call: true,
            may_allocate: allocates,
            may_panic: panics,
            may_write: writes,
            ..hir::Effects::default()
        },
        |effects, argument| effects.union(argument.effects),
    )
}
