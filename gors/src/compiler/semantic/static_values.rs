//! Evaluation of immutable package initializers into typed static values.

use std::collections::BTreeMap;

use super::expressions::is_assignable;
use super::{ConstantSymbol, eval_constant, lower_type};
use crate::compiler::Diagnostic;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{ExprSyntax, ExprSyntaxKind};
use crate::compiler::types::{StaticValue, StructField, Ty};

pub(super) fn evaluate_initializer(
    expression: &ExprSyntax,
    constants: &BTreeMap<String, ConstantSymbol>,
    types: &BTreeMap<String, Ty>,
    source: SourceRef,
) -> Result<(Ty, StaticValue), Diagnostic> {
    if let ExprSyntaxKind::CompositeLiteral {
        ty: Some(literal_type),
        elements,
    } = &expression.kind
    {
        let ty = lower_type(literal_type, types, source)?;
        let value = evaluate_struct(&ty, elements, constants, types, source)?;
        return Ok((ty, value));
    }
    let (ty, value) = eval_constant(expression, constants, source, 0)?;
    Ok((ty, StaticValue::Constant(value)))
}

fn evaluate_struct(
    ty: &Ty,
    elements: &[ExprSyntax],
    constants: &BTreeMap<String, ConstantSymbol>,
    types: &BTreeMap<String, Ty>,
    source: SourceRef,
) -> Result<StaticValue, Diagnostic> {
    let Ty::Struct(fields) = ty.underlying() else {
        return Err(Diagnostic::unsupported(
            "package composite initializers currently require a struct type",
            source,
        ));
    };
    let mut initializers = vec![None; fields.len()];
    let keyed = elements
        .first()
        .is_some_and(|element| matches!(element.kind, ExprSyntaxKind::KeyValue { .. }));
    if keyed {
        assign_keyed_initializers(&mut initializers, fields, elements, source)?;
    } else if !elements.is_empty() {
        if elements.len() != fields.len() {
            return Err(Diagnostic::semantic(
                format!(
                    "struct initializer has {} values for {} fields",
                    elements.len(),
                    fields.len()
                ),
                source,
            ));
        }
        for (destination, value) in initializers.iter_mut().zip(elements) {
            *destination = Some(value);
        }
    }

    let values = fields
        .iter()
        .zip(initializers)
        .map(|(field, initializer)| match initializer {
            Some(initializer) => {
                evaluate_typed_value(initializer, &field.ty, constants, types, source)
            }
            None => StaticValue::zero(&field.ty).ok_or_else(|| {
                Diagnostic::unsupported(
                    format!(
                        "zero value for package struct field type {:?} is not implemented",
                        field.ty
                    ),
                    source,
                )
            }),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(StaticValue::Struct(values))
}

fn assign_keyed_initializers<'a>(
    initializers: &mut [Option<&'a ExprSyntax>],
    fields: &[StructField],
    elements: &'a [ExprSyntax],
    source: SourceRef,
) -> Result<(), Diagnostic> {
    for element in elements {
        let ExprSyntaxKind::KeyValue { key, value } = &element.kind else {
            return Err(Diagnostic::semantic(
                "mixture of keyed and unkeyed struct initializer elements",
                source,
            ));
        };
        let ExprSyntaxKind::Ident(key) = &key.kind else {
            return Err(Diagnostic::semantic(
                "struct initializer field key must be an identifier",
                source,
            ));
        };
        let index = fields
            .iter()
            .position(|field| field.name == key.name.as_ref())
            .ok_or_else(|| {
                Diagnostic::semantic(
                    format!("unknown field {} in struct initializer", key.name),
                    source,
                )
            })?;
        let initializer = initializers.get_mut(index).ok_or_else(|| {
            Diagnostic::backend("resolved package struct field index is out of bounds")
        })?;
        if initializer.replace(value).is_some() {
            return Err(Diagnostic::semantic(
                format!("duplicate field {} in struct initializer", key.name),
                source,
            ));
        }
    }
    Ok(())
}

fn evaluate_typed_value(
    expression: &ExprSyntax,
    expected: &Ty,
    constants: &BTreeMap<String, ConstantSymbol>,
    types: &BTreeMap<String, Ty>,
    source: SourceRef,
) -> Result<StaticValue, Diagnostic> {
    if let ExprSyntaxKind::CompositeLiteral {
        ty: literal_type,
        elements,
    } = &expression.kind
    {
        let ty = literal_type
            .as_deref()
            .map(|ty| lower_type(ty, types, source))
            .transpose()?
            .unwrap_or_else(|| expected.clone());
        if !is_assignable(&ty, expected) {
            return Err(Diagnostic::semantic(
                format!("struct initializer type {ty:?} is not assignable to {expected:?}"),
                source,
            ));
        }
        return evaluate_struct(&ty, elements, constants, types, source);
    }
    let (ty, value) = eval_constant(expression, constants, source, 0)?;
    if !is_assignable(&ty, expected) || !value.is_representable_as(expected) {
        return Err(Diagnostic::semantic(
            format!("initializer value of type {ty:?} is not assignable to {expected:?}"),
            source,
        ));
    }
    Ok(StaticValue::Constant(value))
}
