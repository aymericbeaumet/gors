//! Typed lowering for fixed-size Go arrays of executable scalar values.

use std::collections::{BTreeMap, BTreeSet};

use super::FunctionLowerer;
use super::expressions::{coerce_expr, expr_constant};
use super::{eval_constant_with_lookup, lower_type_with_constant_lookup};
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::NodeId;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{ExprSyntax, ExprSyntaxKind, SyntaxSource};
use crate::compiler::types::{ConstValue, IntTy, Ty};

const MAX_BOOTSTRAP_ARRAY_LENGTH: u64 = 1_048_576;

pub(super) fn lower_array_type(
    length: &ExprSyntax,
    element: &ExprSyntax,
    type_aliases: &BTreeMap<String, Ty>,
    constant_lookup: &impl Fn(&str) -> Option<(Ty, ConstValue)>,
    source: SourceRef,
) -> Result<Ty, Diagnostic> {
    let length = array_length(length, constant_lookup, source)?;
    let element = lower_type_with_constant_lookup(element, type_aliases, constant_lookup, source)?;
    Ok(Ty::Array(length, Box::new(element)))
}

fn array_length(
    expression: &ExprSyntax,
    constant_lookup: &impl Fn(&str) -> Option<(Ty, ConstValue)>,
    source: SourceRef,
) -> Result<u64, Diagnostic> {
    let (ty, value) = eval_constant_with_lookup(expression, constant_lookup, source, 0)?;
    if !ty.is_integer() && !matches!(ty, Ty::Untyped(_)) {
        return Err(Diagnostic::semantic(
            "array length must be an integer constant",
            source,
        ));
    }
    let value = value
        .exact_integer()
        .ok_or_else(|| Diagnostic::semantic("array length must be an integer constant", source))?;
    let ConstValue::Int(value) = value else {
        return Err(Diagnostic::backend(
            "integer array length normalization produced a non-integer constant",
        ));
    };
    let signed = value
        .parse::<i64>()
        .map_err(|_| Diagnostic::semantic("array length is not representable by Go int", source))?;
    let length = u64::try_from(signed)
        .map_err(|_| Diagnostic::semantic("array length must be non-negative", source))?;
    if length > MAX_BOOTSTRAP_ARRAY_LENGTH {
        return Err(Diagnostic::unsupported(
            format!(
                "array length {length} exceeds the current implementation limit of {MAX_BOOTSTRAP_ARRAY_LENGTH}"
            ),
            source,
        ));
    }
    Ok(length)
}

impl FunctionLowerer {
    pub(super) fn indexed_composite_elements<'syntax>(
        &self,
        elements: &'syntax [ExprSyntax],
        source: SourceRef,
    ) -> Result<Option<Vec<Option<&'syntax ExprSyntax>>>, Diagnostic> {
        if !elements
            .iter()
            .any(|element| matches!(element.kind, ExprSyntaxKind::KeyValue { .. }))
        {
            return Ok(None);
        }
        let mut slots = Vec::new();
        let mut next_index = 0_usize;
        for element in elements {
            let (index, value) = match &element.kind {
                ExprSyntaxKind::KeyValue { key, value } => {
                    let (_, key) = self.eval_constant_expression(key, source, 0)?;
                    let ConstValue::Int(key) = key else {
                        return Err(Diagnostic::semantic(
                            "composite literal index must be an integer constant",
                            source,
                        ));
                    };
                    let index = key.parse::<usize>().map_err(|_| {
                        Diagnostic::semantic(
                            "composite literal index must be a non-negative Go int",
                            source,
                        )
                    })?;
                    (index, value.as_ref())
                }
                _ => (next_index, element),
            };
            next_index = index.checked_add(1).ok_or_else(|| {
                Diagnostic::semantic("composite literal index exceeds the type domain", source)
            })?;
            if slots.len() <= index {
                slots.resize(index.saturating_add(1), None);
            }
            let slot = slots.get_mut(index).ok_or_else(|| {
                Diagnostic::backend("expanded composite literal index disappeared")
            })?;
            if slot.replace(value).is_some() {
                return Err(Diagnostic::semantic(
                    format!("composite literal index {index} is initialized more than once"),
                    source,
                ));
            }
        }
        Ok(Some(slots))
    }

    pub(super) fn infer_array_literal_length(
        &self,
        elements: &[ExprSyntax],
        source: SourceRef,
    ) -> Result<u64, Diagnostic> {
        let mut next_index = 0_u64;
        let mut length = 0_u64;
        for element in elements {
            let index = match &element.kind {
                ExprSyntaxKind::KeyValue { key, .. } => {
                    let (_, key) = self.eval_constant_expression(key, source, 0)?;
                    let ConstValue::Int(key) = key else {
                        return Err(Diagnostic::semantic(
                            "array literal index must be an integer constant",
                            source,
                        ));
                    };
                    key.parse::<u64>().map_err(|_| {
                        Diagnostic::semantic("array literal index is outside u64", source)
                    })?
                }
                _ => next_index,
            };
            next_index = index.checked_add(1).ok_or_else(|| {
                Diagnostic::semantic("array literal index exceeds the type domain", source)
            })?;
            length = length.max(next_index);
        }
        if length > MAX_BOOTSTRAP_ARRAY_LENGTH {
            return Err(Diagnostic::unsupported(
                format!(
                    "array length {length} exceeds the current implementation limit of {MAX_BOOTSTRAP_ARRAY_LENGTH}"
                ),
                source,
            ));
        }
        Ok(length)
    }

    pub(super) fn lower_array_literal(
        &mut self,
        literal_ty: Ty,
        elements: &[ExprSyntax],
        node: NodeId,
        syntax_source: SyntaxSource,
        source: SourceRef,
        expected: Option<&Ty>,
    ) -> Result<hir::Expr, Diagnostic> {
        let (length, element_ty) = match literal_ty.underlying() {
            Ty::Array(length, element_ty) => (*length, element_ty.as_ref().clone()),
            _ => {
                return Err(Diagnostic::backend(
                    "array literal lowering received a non-array type",
                ));
            }
        };
        if !is_executable_array(length, &element_ty) {
            return Err(Diagnostic::unsupported(
                "array literal element type has no executable representation",
                source,
            ));
        }
        let length = usize::try_from(length)
            .map_err(|_| Diagnostic::unsupported("array length does not fit this host", source))?;
        let mut values = Vec::with_capacity(length);
        let mut initialized = BTreeSet::new();
        let mut next_index = 0usize;
        for element in elements {
            let (index, value) = match &element.kind {
                ExprSyntaxKind::KeyValue { key, value } => {
                    let (_, key) = self.eval_constant_expression(key, source, 0)?;
                    let ConstValue::Int(key) = key else {
                        return Err(Diagnostic::semantic(
                            "array literal index must be an integer constant",
                            source,
                        ));
                    };
                    let index = key.parse::<usize>().map_err(|_| {
                        Diagnostic::semantic("array literal index is outside usize", source)
                    })?;
                    (index, value.as_ref())
                }
                _ => (next_index, element),
            };
            if index >= length {
                return Err(Diagnostic::semantic(
                    format!("array literal index {index} is outside length {length}"),
                    source,
                ));
            }
            if !initialized.insert(index) {
                return Err(Diagnostic::semantic(
                    format!("array literal index {index} is initialized more than once"),
                    source,
                ));
            }
            next_index = index.saturating_add(1);
            let index = u64::try_from(index)
                .map_err(|_| Diagnostic::backend("array literal index does not fit u64"))?;
            values.push((index, self.lower_expr(value, Some(&element_ty))?));
        }
        for index in 0..length {
            if initialized.contains(&index) {
                continue;
            }
            let node = self.alloc_node(syntax_source)?;
            let value = self.zero_value_expr(node, SourceRef::node(node), element_ty.clone())?;
            values.push((
                u64::try_from(index)
                    .map_err(|_| Diagnostic::backend("array literal index does not fit u64"))?,
                value,
            ));
        }
        let effects = values
            .iter()
            .fold(hir::Effects::default(), |effects, (_, value)| {
                effects.union(value.effects)
            });
        let integer_constants = if matches!(literal_ty, Ty::Array(_, _))
            && element_ty.underlying() == &Ty::Int(IntTy::Int)
        {
            let mut constants = vec![0_i64; length];
            for (index, value) in &values {
                let Some(ConstValue::Int(value)) = expr_constant(value) else {
                    constants.clear();
                    break;
                };
                let Some(value) = value.parse::<i64>().ok() else {
                    constants.clear();
                    break;
                };
                let index = usize::try_from(*index).map_err(|_| {
                    Diagnostic::backend("verified array literal index does not fit usize")
                })?;
                let slot = constants.get_mut(index).ok_or_else(|| {
                    Diagnostic::backend("verified array literal index is outside its length")
                })?;
                *slot = value;
            }
            (!constants.is_empty() || length == 0).then_some(constants)
        } else {
            None
        };
        let mut result = hir::Expr {
            node,
            kind: integer_constants.map_or_else(
                || hir::ExprKind::ArrayLiteral(values),
                hir::ExprKind::ArrayLiteralI64,
            ),
            ty: literal_ty,
            category: hir::ValueCategory::Value,
            effects,
            source,
        };
        if let Some(expected) = expected {
            coerce_expr(&mut result, expected, source)?;
        }
        Ok(result)
    }

    pub(super) fn lower_array_index(
        &mut self,
        array: hir::Expr,
        index: &ExprSyntax,
        node: NodeId,
        source: SourceRef,
        expected: Option<&Ty>,
    ) -> Result<hir::Expr, Diagnostic> {
        let Ty::Array(_, element) = array.ty.underlying() else {
            return Err(Diagnostic::backend(
                "array index lowering received a non-array value",
            ));
        };
        if !is_executable_array_element(element) {
            return Err(Diagnostic::unsupported(
                "array element type has no executable index representation",
                source,
            ));
        }
        let result_ty = element.as_ref().clone();
        let index = self.lower_expr(index, Some(&Ty::Int(IntTy::Int)))?;
        let effects = array.effects.union(index.effects).union(hir::Effects {
            may_panic: true,
            ..hir::Effects::default()
        });
        let mut result = hir::Expr {
            node,
            kind: if result_ty.underlying() == &Ty::Int(IntTy::Int) {
                hir::ExprKind::ArrayIndexI64 {
                    array: Box::new(array),
                    index: Box::new(index),
                }
            } else {
                hir::ExprKind::ArrayIndex {
                    array: Box::new(array),
                    index: Box::new(index),
                }
            },
            ty: result_ty,
            category: hir::ValueCategory::Value,
            effects,
            source,
        };
        if let Some(expected) = expected {
            coerce_expr(&mut result, expected, source)?;
        }
        Ok(result)
    }

    pub(super) fn lower_array_len(
        &mut self,
        array: hir::Expr,
        length: u64,
        node: NodeId,
        source: SourceRef,
        expected: Option<&Ty>,
    ) -> Result<hir::Expr, Diagnostic> {
        let effects = array.effects;
        let mut result = hir::Expr {
            node,
            kind: hir::ExprKind::ArrayLen {
                array: Box::new(array),
                length,
            },
            ty: Ty::Int(IntTy::Int),
            category: hir::ValueCategory::Value,
            effects,
            source,
        };
        if let Some(expected) = expected {
            coerce_expr(&mut result, expected, source)?;
        }
        Ok(result)
    }
}

pub(super) fn is_scalar_array_element(ty: &Ty) -> bool {
    matches!(
        ty.underlying(),
        Ty::Bool
            | Ty::Int(IntTy::Int)
            | Ty::Uint(crate::compiler::types::UintTy::Uint8)
            | Ty::Float(_)
            | Ty::String
    )
}

pub(super) fn is_executable_array_element(ty: &Ty) -> bool {
    is_scalar_array_element(ty) || ty.bootstrap_i64_struct_pointer_fields().is_some()
}

pub(super) fn is_executable_array(length: u64, element: &Ty) -> bool {
    length == 0 || is_executable_array_element(element)
}
