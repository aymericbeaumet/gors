//! Evaluation of immutable package initializers into typed static values.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::expressions::is_assignable;
use super::{ConstantSymbol, eval_constant, lower_type_with_constants};
use crate::compiler::Diagnostic;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{
    ExprSyntax, ExprSyntaxKind, FunctionBodySyntax, StmtSyntax, StmtSyntaxKind,
};
use crate::compiler::types::{StaticValue, StructField, Ty};
use crate::token::Token;

pub(super) fn evaluate_initializer(
    expression: &ExprSyntax,
    constants: &BTreeMap<String, ConstantSymbol>,
    types: &BTreeMap<String, Ty>,
    functions: &BTreeMap<String, Arc<FunctionBodySyntax>>,
    source: SourceRef,
) -> Result<(Ty, StaticValue), Diagnostic> {
    evaluate_expression(expression, constants, types, functions, source, 0)
}

fn evaluate_expression(
    expression: &ExprSyntax,
    constants: &BTreeMap<String, ConstantSymbol>,
    types: &BTreeMap<String, Ty>,
    functions: &BTreeMap<String, Arc<FunctionBodySyntax>>,
    source: SourceRef,
    depth: usize,
) -> Result<(Ty, StaticValue), Diagnostic> {
    if depth >= 64 {
        return Err(Diagnostic::semantic(
            "package initializer call depth exceeds 64",
            source,
        ));
    }
    if let ExprSyntaxKind::CompositeLiteral {
        ty: Some(literal_type),
        elements,
    } = &expression.kind
    {
        let ty = lower_type_with_constants(literal_type, types, constants, source)?;
        let value = match ty.underlying() {
            Ty::Array(_, _) => {
                evaluate_array(&ty, elements, constants, types, functions, source, depth)?
            }
            Ty::Struct(_) => {
                evaluate_struct(&ty, elements, constants, types, functions, source, depth)?
            }
            Ty::Slice(element) => StaticValue::Slice(
                elements
                    .iter()
                    .map(|element_syntax| {
                        evaluate_typed_value(
                            element_syntax,
                            element,
                            constants,
                            types,
                            functions,
                            source,
                            depth,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            _ => {
                return Err(Diagnostic::unsupported(
                    "package composite initializer type is not implemented",
                    source,
                ));
            }
        };
        return Ok((ty, value));
    }
    if let ExprSyntaxKind::Call {
        callee,
        arguments,
        spread: false,
    } = &expression.kind
        && arguments.is_empty()
        && let ExprSyntaxKind::Ident(name) = &callee.kind
        && let Some(body) = functions.get(name.name.as_ref())
    {
        return evaluate_static_function(body, constants, types, functions, source, depth + 1);
    }
    let shadowed_predeclared = constants
        .keys()
        .chain(types.keys())
        .chain(functions.keys())
        .cloned()
        .collect();
    let (ty, value) = eval_constant(
        expression,
        constants,
        types,
        &shadowed_predeclared,
        source,
        None,
    )?;
    Ok((ty, StaticValue::Constant(value)))
}

#[allow(clippy::too_many_arguments)]
fn evaluate_array(
    ty: &Ty,
    elements: &[ExprSyntax],
    constants: &BTreeMap<String, ConstantSymbol>,
    types: &BTreeMap<String, Ty>,
    functions: &BTreeMap<String, Arc<FunctionBodySyntax>>,
    source: SourceRef,
    depth: usize,
) -> Result<StaticValue, Diagnostic> {
    let Ty::Array(length, element) = ty.underlying() else {
        return Err(Diagnostic::backend(
            "package array initializer has a non-array type",
        ));
    };
    let length = usize::try_from(*length)
        .map_err(|_| Diagnostic::semantic("package array length is outside usize", source))?;
    let mut initializers = vec![None; length];
    let mut next_index = 0_usize;
    for syntax in elements {
        let (index, value) = match &syntax.kind {
            ExprSyntaxKind::KeyValue { key, value } => (
                evaluate_array_index(key, constants, types, functions, source)?,
                value.as_ref(),
            ),
            _ => (next_index, syntax),
        };
        let destination = initializers.get_mut(index).ok_or_else(|| {
            Diagnostic::semantic(
                format!("array literal index {index} is outside length {length}"),
                source,
            )
        })?;
        if destination.replace(value).is_some() {
            return Err(Diagnostic::semantic(
                format!("array literal index {index} is initialized more than once"),
                source,
            ));
        }
        next_index = index
            .checked_add(1)
            .ok_or_else(|| Diagnostic::semantic("array literal index overflow", source))?;
    }

    initializers
        .into_iter()
        .map(|initializer| match initializer {
            Some(initializer) => evaluate_typed_value(
                initializer,
                element,
                constants,
                types,
                functions,
                source,
                depth,
            ),
            None => StaticValue::zero(element).ok_or_else(|| {
                Diagnostic::unsupported(
                    format!(
                        "zero value for package array element type {element:?} is not implemented"
                    ),
                    source,
                )
            }),
        })
        .collect::<Result<Vec<_>, _>>()
        .map(StaticValue::Array)
}

fn evaluate_array_index(
    expression: &ExprSyntax,
    constants: &BTreeMap<String, ConstantSymbol>,
    types: &BTreeMap<String, Ty>,
    functions: &BTreeMap<String, Arc<FunctionBodySyntax>>,
    source: SourceRef,
) -> Result<usize, Diagnostic> {
    let shadowed_predeclared = constants
        .keys()
        .chain(types.keys())
        .chain(functions.keys())
        .cloned()
        .collect();
    let (_, value) = eval_constant(
        expression,
        constants,
        types,
        &shadowed_predeclared,
        source,
        None,
    )?;
    let crate::compiler::types::ConstValue::Int(value) = value else {
        return Err(Diagnostic::semantic(
            "array literal index must be an integer constant",
            source,
        ));
    };
    value
        .parse::<usize>()
        .map_err(|_| Diagnostic::semantic("array literal index is outside usize", source))
}

fn evaluate_static_function(
    body: &FunctionBodySyntax,
    constants: &BTreeMap<String, ConstantSymbol>,
    types: &BTreeMap<String, Ty>,
    functions: &BTreeMap<String, Arc<FunctionBodySyntax>>,
    source: SourceRef,
    depth: usize,
) -> Result<(Ty, StaticValue), Diagnostic> {
    let block = body.block.as_ref().ok_or_else(|| {
        Diagnostic::unsupported("package initializer function has no body", source)
    })?;
    let statements = block
        .statements
        .iter()
        .filter(|statement| !matches!(statement.kind, StmtSyntaxKind::Empty))
        .collect::<Vec<_>>();
    let [statement] = statements.as_slice() else {
        return Err(Diagnostic::unsupported(
            "package initializer functions must contain one return statement",
            source,
        ));
    };
    let StmtSyntaxKind::Return(values) = &statement.kind else {
        return Err(Diagnostic::unsupported(
            "package initializer functions must contain one return statement",
            source,
        ));
    };
    let [value] = values.as_ref() else {
        return Err(Diagnostic::unsupported(
            "package initializer functions must return one value",
            source,
        ));
    };
    evaluate_expression(value, constants, types, functions, source, depth)
}

fn evaluate_struct(
    ty: &Ty,
    elements: &[ExprSyntax],
    constants: &BTreeMap<String, ConstantSymbol>,
    types: &BTreeMap<String, Ty>,
    functions: &BTreeMap<String, Arc<FunctionBodySyntax>>,
    source: SourceRef,
    depth: usize,
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
            Some(initializer) => evaluate_typed_value(
                initializer,
                &field.ty,
                constants,
                types,
                functions,
                source,
                depth,
            ),
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
    functions: &BTreeMap<String, Arc<FunctionBodySyntax>>,
    source: SourceRef,
    depth: usize,
) -> Result<StaticValue, Diagnostic> {
    if let ExprSyntaxKind::CompositeLiteral {
        ty: literal_type,
        elements,
    } = &expression.kind
    {
        let ty = literal_type
            .as_deref()
            .map(|ty| lower_type_with_constants(ty, types, constants, source))
            .transpose()?
            .unwrap_or_else(|| expected.clone());
        if !is_assignable(&ty, expected) {
            return Err(Diagnostic::semantic(
                format!("struct initializer type {ty:?} is not assignable to {expected:?}"),
                source,
            ));
        }
        return match ty.underlying() {
            Ty::Array(_, _) => {
                evaluate_array(&ty, elements, constants, types, functions, source, depth)
            }
            Ty::Struct(_) => {
                evaluate_struct(&ty, elements, constants, types, functions, source, depth)
            }
            Ty::Slice(element) => elements
                .iter()
                .map(|element_syntax| {
                    evaluate_typed_value(
                        element_syntax,
                        element,
                        constants,
                        types,
                        functions,
                        source,
                        depth,
                    )
                })
                .collect::<Result<Vec<_>, _>>()
                .map(StaticValue::Slice),
            _ => Err(Diagnostic::unsupported(
                "nested package composite initializer type is not implemented",
                source,
            )),
        };
    }
    let (ty, value) = evaluate_expression(expression, constants, types, functions, source, depth)?;
    if !is_assignable(&ty, expected) || !value.is_representable_as(expected) {
        return Err(Diagnostic::semantic(
            format!("initializer value of type {ty:?} is not assignable to {expected:?}"),
            source,
        ));
    }
    Ok(value)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn apply_package_initializers(
    name: &str,
    ty: &Ty,
    value: &mut StaticValue,
    initializers: &[Arc<FunctionBodySyntax>],
    constants: &BTreeMap<String, ConstantSymbol>,
    types: &BTreeMap<String, Ty>,
    functions: &BTreeMap<String, Arc<FunctionBodySyntax>>,
    source: SourceRef,
) -> Result<(), Diagnostic> {
    for initializer in initializers {
        let block = initializer.block.as_ref().ok_or_else(|| {
            Diagnostic::unsupported("package init declaration has no body", source)
        })?;
        for statement in block.statements.iter() {
            apply_initializer_statement(
                statement, name, ty, value, constants, types, functions, source,
            )?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn apply_initializer_statement(
    statement: &StmtSyntax,
    name: &str,
    ty: &Ty,
    value: &mut StaticValue,
    constants: &BTreeMap<String, ConstantSymbol>,
    types: &BTreeMap<String, Ty>,
    functions: &BTreeMap<String, Arc<FunctionBodySyntax>>,
    source: SourceRef,
) -> Result<(), Diagnostic> {
    match &statement.kind {
        StmtSyntaxKind::Empty => Ok(()),
        StmtSyntaxKind::IncDec { expression, token }
            if is_identifier(expression, name) && matches!(token, Token::INC | Token::DEC) =>
        {
            let StaticValue::Constant(crate::compiler::types::ConstValue::Int(integer)) = value
            else {
                return Err(Diagnostic::unsupported(
                    "package init increment requires an integer variable",
                    source,
                ));
            };
            let parsed = integer.parse::<i64>().map_err(|_| {
                Diagnostic::semantic("package init integer is outside Go int", source)
            })?;
            let updated = if *token == Token::INC {
                parsed.checked_add(1)
            } else {
                parsed.checked_sub(1)
            }
            .ok_or_else(|| Diagnostic::semantic("package init integer overflow", source))?;
            *integer = updated.to_string();
            Ok(())
        }
        StmtSyntaxKind::Assign { left, token, right } => {
            let [target] = left.as_ref() else {
                return Ok(());
            };
            if !is_identifier(target, name) {
                return Ok(());
            }
            let [expression] = right.as_ref() else {
                return Err(Diagnostic::unsupported(
                    "package init assignments require one = expression",
                    source,
                ));
            };
            if *token != Token::ASSIGN {
                return Err(Diagnostic::unsupported(
                    "package init assignments require one = expression",
                    source,
                ));
            }
            if append_static_slice(
                name, ty, value, expression, constants, types, functions, source,
            )? {
                return Ok(());
            }
            *value = evaluate_typed_value(expression, ty, constants, types, functions, source, 0)?;
            Ok(())
        }
        StmtSyntaxKind::IncDec { .. } => Ok(()),
        _ => Err(Diagnostic::unsupported(
            "package init statement is not statically executable",
            source,
        )),
    }
}

#[allow(clippy::too_many_arguments)]
fn append_static_slice(
    name: &str,
    ty: &Ty,
    value: &mut StaticValue,
    expression: &ExprSyntax,
    constants: &BTreeMap<String, ConstantSymbol>,
    types: &BTreeMap<String, Ty>,
    functions: &BTreeMap<String, Arc<FunctionBodySyntax>>,
    source: SourceRef,
) -> Result<bool, Diagnostic> {
    let ExprSyntaxKind::Call {
        callee,
        arguments,
        spread: false,
    } = &expression.kind
    else {
        return Ok(false);
    };
    let ExprSyntaxKind::Ident(callee) = &callee.kind else {
        return Ok(false);
    };
    let Some((base, additions)) = arguments.split_first() else {
        return Ok(false);
    };
    if callee.name.as_ref() != "append" || !is_identifier(base, name) {
        return Ok(false);
    }
    let Ty::Slice(element) = ty.underlying() else {
        return Err(Diagnostic::semantic(
            "append in package init requires a slice variable",
            source,
        ));
    };
    let StaticValue::Slice(values) = value else {
        return Err(Diagnostic::backend(
            "package slice initializer has a non-slice static value",
        ));
    };
    for addition in additions {
        values.push(evaluate_typed_value(
            addition, element, constants, types, functions, source, 0,
        )?);
    }
    Ok(true)
}

fn is_identifier(expression: &ExprSyntax, expected: &str) -> bool {
    matches!(&expression.kind, ExprSyntaxKind::Ident(identifier) if identifier.name.as_ref() == expected)
}
