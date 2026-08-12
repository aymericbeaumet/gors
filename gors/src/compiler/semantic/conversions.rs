//! Typed value conversions that select explicit runtime representations.

use super::FunctionLowerer;
use super::expression_lower::slice_runtime_effects;
use super::expressions::{coerce_expr, is_assignable};
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::NodeId;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{ExprSyntax, ExprSyntaxKind};
use crate::compiler::types::{IntTy, Ty, UintTy, UntypedTy};

pub(super) fn is_predeclared_conversion_name(name: &str) -> bool {
    matches!(
        name,
        "bool"
            | "string"
            | "int"
            | "int8"
            | "int16"
            | "int32"
            | "int64"
            | "uint"
            | "uint8"
            | "uint16"
            | "uint32"
            | "uint64"
            | "uintptr"
            | "byte"
            | "rune"
            | "float32"
            | "float64"
            | "complex64"
            | "complex128"
            | "any"
    )
}

impl FunctionLowerer {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn lower_conversion_call(
        &mut self,
        callee: &ExprSyntax,
        arguments: &[ExprSyntax],
        spread: bool,
        node: NodeId,
        source: SourceRef,
        expected: Option<&Ty>,
    ) -> Result<hir::Expr, Diagnostic> {
        if spread {
            return Err(Diagnostic::semantic(
                "conversions do not accept ...",
                source,
            ));
        }
        let [argument] = arguments else {
            return Err(Diagnostic::semantic(
                "a conversion requires exactly one argument",
                source,
            ));
        };
        let target = self.lower_scoped_type(callee, source)?;
        if is_nil_identifier(argument) {
            if !is_nilable_type(&target) {
                return Err(Diagnostic::semantic(
                    format!("cannot convert nil to {target:?}"),
                    source,
                ));
            }
            let mut result = self.zero_value_expr(node, source, target)?;
            if let Some(expected) = expected {
                coerce_expr(&mut result, expected, source)?;
            }
            return Ok(result);
        }
        if matches!(target.underlying(), Ty::Interface(_)) {
            // A conversion to an interface type, including the predeclared
            // `any` alias, boxes the operand exactly like interface
            // assignment.
            let value = self.lower_expr(argument, None)?;
            let mut result = self.coerce_interface_value(value, &target, argument.source)?;
            if let Some(expected) = expected {
                coerce_expr(&mut result, expected, source)?;
            }
            return Ok(result);
        }
        let mut argument = self.lower_expr(argument, None)?;
        let byte_slice = Ty::Slice(Box::new(Ty::Uint(UintTy::Uint8)));
        let mut result = if is_string_type(&target) && argument.ty.is_integer() {
            let default_ty = argument.ty.default_typed();
            if argument.ty != default_ty {
                coerce_expr(&mut argument, &default_ty, source)?;
            }
            let effects = slice_runtime_effects(&[&argument], false, true, false);
            hir::Expr {
                node,
                kind: hir::ExprKind::Call {
                    callee: hir::Callee::Builtin(hir::Builtin::StringFromRune),
                    args: vec![argument],
                },
                ty: target.clone(),
                category: hir::ValueCategory::Value,
                effects,
                source,
            }
        } else if is_string_type(&target) && is_byte_slice_type(&argument.ty) {
            let effects = slice_runtime_effects(&[&argument], false, true, false);
            hir::Expr {
                node,
                kind: hir::ExprKind::Call {
                    callee: hir::Callee::Builtin(hir::Builtin::StringFromSliceU8),
                    args: vec![argument],
                },
                ty: target.clone(),
                category: hir::ValueCategory::Value,
                effects,
                source,
            }
        } else if is_string_type(&target) && is_rune_slice_type(&argument.ty) {
            let effects = slice_runtime_effects(&[&argument], false, true, false);
            hir::Expr {
                node,
                kind: hir::ExprKind::Call {
                    callee: hir::Callee::Builtin(hir::Builtin::StringFromSliceRunes),
                    args: vec![argument],
                },
                ty: target.clone(),
                category: hir::ValueCategory::Value,
                effects,
                source,
            }
        } else if is_rune_slice_type(&target) && is_string_conversion_source(&argument.ty) {
            if argument.ty == Ty::Untyped(UntypedTy::String) {
                coerce_expr(&mut argument, &Ty::String, source)?;
            }
            let effects = slice_runtime_effects(&[&argument], false, true, false);
            hir::Expr {
                node,
                kind: hir::ExprKind::Call {
                    callee: hir::Callee::Builtin(hir::Builtin::StringToSliceRunes),
                    args: vec![argument],
                },
                ty: target.clone(),
                category: hir::ValueCategory::Value,
                effects,
                source,
            }
        } else if target == byte_slice && is_string_conversion_source(&argument.ty) {
            if argument.ty == Ty::Untyped(UntypedTy::String) {
                coerce_expr(&mut argument, &Ty::String, source)?;
            }
            let empty_node = self.alloc_node(callee.source)?;
            let empty = hir::Expr {
                node: empty_node,
                kind: hir::ExprKind::SliceLiteralU8(Vec::new()),
                ty: byte_slice.clone(),
                category: hir::ValueCategory::Value,
                effects: hir::Effects {
                    may_allocate: true,
                    ..hir::Effects::default()
                },
                source: SourceRef::node(empty_node),
            };
            let effects = slice_runtime_effects(&[&empty, &argument], true, true, false);
            hir::Expr {
                node,
                kind: hir::ExprKind::Call {
                    callee: hir::Callee::Builtin(hir::Builtin::SliceU8AppendString),
                    args: vec![empty, argument],
                },
                ty: byte_slice,
                category: hir::ValueCategory::Value,
                effects,
                source,
            }
        } else if let hir::ExprKind::Constant(value) = &mut argument.kind
            && value.is_representable_as(&target)
        {
            *value = value.normalized_for(&target);
            argument.ty = target;
            argument
        } else if is_assignable(&argument.ty, &target) {
            coerce_expr(&mut argument, &target, source)?;
            argument
        } else if argument
            .ty
            .underlying()
            .is_identical_to(target.underlying())
            || is_integer_conversion(&argument.ty, &target)
            || is_numeric_conversion(&argument.ty, &target)
        {
            let effects = argument.effects;
            hir::Expr {
                node,
                kind: hir::ExprKind::Conversion {
                    value: Box::new(argument),
                },
                ty: target,
                category: hir::ValueCategory::Value,
                effects,
                source,
            }
        } else {
            return Err(Diagnostic::unsupported(
                format!(
                    "conversion from {:?} to {target:?} requires a representation change",
                    argument.ty
                ),
                source,
            ));
        };
        if let Some(expected) = expected {
            coerce_expr(&mut result, expected, source)?;
        }
        Ok(result)
    }
}

fn is_string_type(ty: &Ty) -> bool {
    ty.underlying() == &Ty::String
}

fn is_string_conversion_source(ty: &Ty) -> bool {
    is_string_type(ty) || ty == &Ty::Untyped(UntypedTy::String)
}

fn is_byte_slice_type(ty: &Ty) -> bool {
    matches!(
        ty.underlying(),
        Ty::Slice(element) if element.underlying() == &Ty::Uint(UintTy::Uint8)
    )
}

fn is_rune_slice_type(ty: &Ty) -> bool {
    matches!(
        ty.underlying(),
        Ty::Slice(element) if element.underlying() == &Ty::Int(IntTy::Int32)
    )
}

fn is_nil_identifier(expression: &ExprSyntax) -> bool {
    matches!(
        &expression.kind,
        ExprSyntaxKind::Ident(identifier) if identifier.name.as_ref() == "nil"
    )
}

fn is_nilable_type(ty: &Ty) -> bool {
    matches!(
        ty.underlying(),
        Ty::Interface(_)
            | Ty::Function(_)
            | Ty::Pointer(_)
            | Ty::Slice(_)
            | Ty::Map(_, _)
            | Ty::Channel(_, _)
    )
}

fn is_integer_conversion(from: &Ty, to: &Ty) -> bool {
    matches!(
        (from.underlying(), to.underlying()),
        (Ty::Int(_) | Ty::Uint(_), Ty::Int(_) | Ty::Uint(_))
    )
}

fn is_numeric_conversion(from: &Ty, to: &Ty) -> bool {
    matches!(
        (from.underlying(), to.underlying()),
        (
            Ty::Int(_) | Ty::Uint(_) | Ty::Float(_),
            Ty::Int(_) | Ty::Uint(_) | Ty::Float(_)
        ) | (Ty::Complex(_), Ty::Complex(_))
    )
}
