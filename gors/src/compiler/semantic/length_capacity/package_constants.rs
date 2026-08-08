//! Package-constant `len`/`cap` operand checking.

use std::collections::{BTreeMap, BTreeSet};

use super::{CheckedOperand, classify, is_nilable, source_is_addressable};
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::provenance::SourceRef;
use crate::compiler::semantic::ConstantSymbol;
use crate::compiler::semantic::expressions::{
    exact_common_operand_type, is_assignable, lower_binary_op, validate_binary_operator,
};
use crate::compiler::syntax::{ExprSyntax, ExprSyntaxKind};
use crate::compiler::types::{ConstValue, Ty};
use crate::token::Token;

pub(in crate::compiler::semantic) fn eval_package_constant(
    expression: &ExprSyntax,
    constants: &BTreeMap<String, ConstantSymbol>,
    types: &BTreeMap<String, Ty>,
    shadowed_predeclared: &BTreeSet<String>,
    source: SourceRef,
    iota: Option<u64>,
) -> Result<(Ty, ConstValue), Diagnostic> {
    eval_package_constant_with_variables(
        expression,
        constants,
        &BTreeMap::new(),
        types,
        shadowed_predeclared,
        source,
        iota,
    )
}

pub(in crate::compiler::semantic) fn eval_package_constant_with_variables(
    expression: &ExprSyntax,
    constants: &BTreeMap<String, ConstantSymbol>,
    variables: &BTreeMap<String, Ty>,
    types: &BTreeMap<String, Ty>,
    shadowed_predeclared: &BTreeSet<String>,
    source: SourceRef,
    iota: Option<u64>,
) -> Result<(Ty, ConstValue), Diagnostic> {
    crate::compiler::semantic::constants::eval_constant_with_length_capacity(
        expression,
        &|name| {
            constants
                .get(name)
                .map(|constant| (constant.ty.clone(), constant.value.clone()))
        },
        &|name| types.get(name).cloned(),
        &|operation, operand, operand_source| {
            if shadowed_predeclared.contains(operation.name()) {
                return Err(Diagnostic::semantic(
                    format!(
                        "{} does not resolve to the predeclared builtin",
                        operation.name()
                    ),
                    operand_source,
                ));
            }
            let checked = check_package_operand(
                operand,
                constants,
                variables,
                types,
                shadowed_predeclared,
                operand_source,
                iota,
            )?;
            classify(
                operation,
                &checked.ty,
                checked.constant.as_ref(),
                checked.contains_call_or_receive,
                operand_source,
            )
        },
        source,
        iota,
    )
}

fn check_package_operand(
    expression: &ExprSyntax,
    constants: &BTreeMap<String, ConstantSymbol>,
    variables: &BTreeMap<String, Ty>,
    types: &BTreeMap<String, Ty>,
    shadowed_predeclared: &BTreeSet<String>,
    source: SourceRef,
    iota: Option<u64>,
) -> Result<CheckedOperand, Diagnostic> {
    match &expression.kind {
        ExprSyntaxKind::Literal { .. } => {
            let (ty, value) = eval_package_constant_with_variables(
                expression,
                constants,
                variables,
                types,
                shadowed_predeclared,
                source,
                iota,
            )?;
            Ok(CheckedOperand {
                ty,
                constant: Some(value),
                contains_call_or_receive: false,
            })
        }
        ExprSyntaxKind::Ident(identifier) => {
            if let Some(constant) = constants.get(identifier.name.as_ref()) {
                return Ok(CheckedOperand {
                    ty: constant.ty.clone(),
                    constant: Some(constant.value.clone()),
                    contains_call_or_receive: false,
                });
            }
            if let Some(ty) = variables.get(identifier.name.as_ref()) {
                return Ok(CheckedOperand {
                    ty: ty.clone(),
                    constant: None,
                    contains_call_or_receive: false,
                });
            }
            let name = identifier.name.as_ref();
            if matches!(name, "true" | "false") && !shadowed_predeclared.contains(name) {
                return Ok(CheckedOperand {
                    ty: Ty::Untyped(crate::compiler::types::UntypedTy::Bool),
                    constant: Some(ConstValue::Bool(name == "true")),
                    contains_call_or_receive: false,
                });
            }
            if name == "iota" && !shadowed_predeclared.contains(name) {
                let Some(iota) = iota else {
                    return Err(Diagnostic::semantic(
                        "iota is only defined in constant declarations",
                        source,
                    ));
                };
                return Ok(CheckedOperand {
                    ty: Ty::Untyped(crate::compiler::types::UntypedTy::Int),
                    constant: Some(ConstValue::Int(iota.to_string())),
                    contains_call_or_receive: false,
                });
            }
            Err(Diagnostic::semantic(
                format!("{} is not a constant", identifier.name),
                source,
            ))
        }
        ExprSyntaxKind::Paren(inner) => check_package_operand(
            inner,
            constants,
            variables,
            types,
            shadowed_predeclared,
            source,
            iota,
        ),
        ExprSyntaxKind::Unary {
            token: Token::ARROW,
            expression,
        } => {
            let channel = check_package_operand(
                expression,
                constants,
                variables,
                types,
                shadowed_predeclared,
                source,
                iota,
            )?;
            let Ty::Channel(direction, element) = channel.ty.underlying() else {
                return Err(Diagnostic::semantic(
                    "receive requires a channel operand",
                    source,
                ));
            };
            if !direction.can_receive() {
                return Err(Diagnostic::semantic(
                    "cannot receive from a send-only channel",
                    source,
                ));
            }
            Ok(CheckedOperand {
                ty: element.as_ref().clone(),
                constant: None,
                contains_call_or_receive: true,
            })
        }
        ExprSyntaxKind::Unary {
            token,
            expression: inner,
        } => {
            let operand = check_package_operand(
                inner,
                constants,
                variables,
                types,
                shadowed_predeclared,
                source,
                iota,
            )?;
            let ty = match token {
                Token::AND => {
                    if !source_is_addressable(inner, &|name| variables.contains_key(name)) {
                        return Err(Diagnostic::semantic(
                            "cannot take the address of this expression",
                            source,
                        ));
                    }
                    Ty::Pointer(Box::new(operand.ty.clone()))
                }
                Token::MUL => {
                    let Ty::Pointer(element) = operand.ty.underlying() else {
                        return Err(Diagnostic::semantic(
                            "indirection requires a pointer operand",
                            source,
                        ));
                    };
                    element.as_ref().clone()
                }
                Token::ADD | Token::SUB if operand.ty.default_typed().is_numeric() => {
                    operand.ty.clone()
                }
                Token::NOT if operand.ty.default_typed().underlying() == &Ty::Bool => {
                    operand.ty.clone()
                }
                Token::XOR if operand.ty.default_typed().is_integer() => operand.ty.clone(),
                _ => {
                    return Err(Diagnostic::semantic(
                        format!("invalid unary {token:?} operand {:?}", operand.ty),
                        source,
                    ));
                }
            };
            let constant =
                if operand.constant.is_some() && !matches!(token, Token::AND | Token::MUL) {
                    Some(
                        eval_package_constant_with_variables(
                            expression,
                            constants,
                            variables,
                            types,
                            shadowed_predeclared,
                            source,
                            iota,
                        )?
                        .1,
                    )
                } else {
                    None
                };
            Ok(CheckedOperand {
                ty,
                constant,
                contains_call_or_receive: operand.contains_call_or_receive,
            })
        }
        ExprSyntaxKind::Binary { left, token, right } => {
            let left = check_package_operand(
                left,
                constants,
                variables,
                types,
                shadowed_predeclared,
                source,
                iota,
            )?;
            let right = check_package_operand(
                right,
                constants,
                variables,
                types,
                shadowed_predeclared,
                source,
                iota,
            )?;
            let op = lower_binary_op(*token).ok_or_else(|| {
                Diagnostic::unsupported(
                    format!("binary operator {token:?} is not implemented"),
                    source,
                )
            })?;
            let both_constant = left.constant.is_some() && right.constant.is_some();
            let operand_ty = exact_common_operand_type(&left.ty, &right.ty).ok_or_else(|| {
                Diagnostic::semantic(
                    format!("incompatible operands {:?} and {:?}", left.ty, right.ty),
                    source,
                )
            })?;
            if both_constant {
                crate::compiler::semantic::validate_constant_binary_operator(
                    op,
                    &operand_ty,
                    source,
                )?;
            } else {
                validate_binary_operator(op, &operand_ty.default_typed(), source)?;
            }
            let comparison_or_logical = matches!(
                op,
                hir::BinaryOp::Equal
                    | hir::BinaryOp::NotEqual
                    | hir::BinaryOp::Less
                    | hir::BinaryOp::LessEqual
                    | hir::BinaryOp::Greater
                    | hir::BinaryOp::GreaterEqual
                    | hir::BinaryOp::LogicalAnd
                    | hir::BinaryOp::LogicalOr
            );
            let (ty, constant) = if both_constant {
                let (ty, value) = eval_package_constant_with_variables(
                    expression,
                    constants,
                    variables,
                    types,
                    shadowed_predeclared,
                    source,
                    iota,
                )?;
                (ty, Some(value))
            } else if comparison_or_logical {
                (Ty::Bool, None)
            } else {
                (operand_ty.default_typed(), None)
            };
            Ok(CheckedOperand {
                ty,
                constant,
                contains_call_or_receive: left.contains_call_or_receive
                    || right.contains_call_or_receive,
            })
        }
        ExprSyntaxKind::Call {
            callee,
            arguments,
            spread,
        } => check_package_call(
            expression,
            callee,
            arguments,
            *spread,
            constants,
            variables,
            types,
            shadowed_predeclared,
            source,
            iota,
        ),
        ExprSyntaxKind::CompositeLiteral {
            ty: Some(ty_syntax),
            elements,
        } => check_package_composite(
            ty_syntax,
            elements,
            constants,
            variables,
            types,
            shadowed_predeclared,
            PackageEvaluationSite { source, iota },
        ),
        _ => Err(Diagnostic::unsupported(
            "this package len/cap operand has no check-only semantic representation",
            source,
        )),
    }
}

#[allow(clippy::too_many_arguments)]
fn check_package_call(
    whole: &ExprSyntax,
    callee: &ExprSyntax,
    arguments: &[ExprSyntax],
    spread: bool,
    constants: &BTreeMap<String, ConstantSymbol>,
    variables: &BTreeMap<String, Ty>,
    types: &BTreeMap<String, Ty>,
    shadowed_predeclared: &BTreeSet<String>,
    source: SourceRef,
    iota: Option<u64>,
) -> Result<CheckedOperand, Diagnostic> {
    if package_type_expression(callee, types, shadowed_predeclared) {
        if spread || arguments.len() != 1 {
            return Err(Diagnostic::semantic(
                "a conversion requires exactly one argument",
                source,
            ));
        }
        let target = lower_package_length_capacity_type(
            callee,
            constants,
            variables,
            types,
            shadowed_predeclared,
            source,
            iota,
        )?;
        let argument = arguments
            .first()
            .ok_or_else(|| Diagnostic::backend("conversion argument disappeared"))?;
        if matches!(
            &argument.kind,
            ExprSyntaxKind::Ident(identifier)
                if identifier.name.as_ref() == "nil"
                    && !shadowed_predeclared.contains("nil")
        ) {
            if !is_nilable(&target) {
                return Err(Diagnostic::semantic(
                    format!("cannot convert nil to {target:?}"),
                    source,
                ));
            }
            return Ok(CheckedOperand {
                ty: target,
                constant: None,
                contains_call_or_receive: false,
            });
        }
        let argument = check_package_operand(
            argument,
            constants,
            variables,
            types,
            shadowed_predeclared,
            source,
            iota,
        )?;
        if !is_assignable(&argument.ty, &target)
            && argument.ty.underlying() != target.underlying()
            && !(argument.ty.is_numeric() && target.is_numeric())
        {
            return Err(Diagnostic::semantic(
                format!("cannot convert {:?} to {target:?}", argument.ty),
                source,
            ));
        }
        return Ok(CheckedOperand {
            ty: target,
            constant: argument.constant,
            contains_call_or_receive: argument.contains_call_or_receive,
        });
    }
    if matches!(
        &callee.kind,
        ExprSyntaxKind::Ident(identifier)
            if matches!(
                identifier.name.as_ref(),
                "len" | "cap" | "min" | "max" | "complex" | "real" | "imag"
            )
                && !constants.contains_key(identifier.name.as_ref())
                && !variables.contains_key(identifier.name.as_ref())
                && !types.contains_key(identifier.name.as_ref())
                && !shadowed_predeclared.contains(identifier.name.as_ref())
    ) {
        for argument in arguments {
            check_package_operand(
                argument,
                constants,
                variables,
                types,
                shadowed_predeclared,
                source,
                iota,
            )?;
        }
        let (ty, value) = eval_package_constant_with_variables(
            whole,
            constants,
            variables,
            types,
            shadowed_predeclared,
            source,
            iota,
        )?;
        return Ok(CheckedOperand {
            ty,
            constant: Some(value),
            contains_call_or_receive: false,
        });
    }
    Err(Diagnostic::semantic(
        "function call makes len/cap non-constant",
        source,
    ))
}

#[derive(Clone, Copy)]
struct PackageEvaluationSite {
    source: SourceRef,
    iota: Option<u64>,
}

fn check_package_composite(
    ty_syntax: &ExprSyntax,
    elements: &[ExprSyntax],
    constants: &BTreeMap<String, ConstantSymbol>,
    variables: &BTreeMap<String, Ty>,
    types: &BTreeMap<String, Ty>,
    shadowed_predeclared: &BTreeSet<String>,
    site: PackageEvaluationSite,
) -> Result<CheckedOperand, Diagnostic> {
    let PackageEvaluationSite { source, iota } = site;
    let ty = lower_package_length_capacity_type(
        ty_syntax,
        constants,
        variables,
        types,
        shadowed_predeclared,
        source,
        iota,
    )?;
    let Ty::Array(length, element) = ty.underlying() else {
        return Err(Diagnostic::unsupported(
            "check-only package len/cap composite operands require an array type",
            source,
        ));
    };
    let length = *length;
    let element = element.as_ref().clone();
    let mut next_index = 0_u64;
    let mut initialized = BTreeSet::new();
    let mut contains_call_or_receive = false;
    for syntax in elements {
        let (index, value) = match &syntax.kind {
            ExprSyntaxKind::KeyValue { key, value } => {
                let checked_key = check_package_operand(
                    key,
                    constants,
                    variables,
                    types,
                    shadowed_predeclared,
                    source,
                    iota,
                )?;
                if checked_key.constant.is_none() {
                    return Err(Diagnostic::semantic(
                        "array literal index must be an integer constant",
                        source,
                    ));
                }
                let (_, key) = eval_package_constant_with_variables(
                    key,
                    constants,
                    variables,
                    types,
                    shadowed_predeclared,
                    source,
                    iota,
                )?;
                let ConstValue::Int(key) = key else {
                    return Err(Diagnostic::semantic(
                        "array literal index must be an integer constant",
                        source,
                    ));
                };
                let index = key.parse::<u64>().map_err(|_| {
                    Diagnostic::semantic("array literal index is outside u64", source)
                })?;
                (index, value.as_ref())
            }
            _ => (next_index, syntax),
        };
        if index >= length || !initialized.insert(index) {
            return Err(Diagnostic::semantic(
                "invalid array literal index in len/cap operand",
                source,
            ));
        }
        next_index = index
            .checked_add(1)
            .ok_or_else(|| Diagnostic::semantic("array literal index overflow", source))?;
        let checked = if is_package_predeclared_identifier(value, "nil", shadowed_predeclared)
            && is_nilable(&element)
        {
            CheckedOperand {
                ty: element.clone(),
                constant: None,
                contains_call_or_receive: false,
            }
        } else {
            check_package_operand(
                value,
                constants,
                variables,
                types,
                shadowed_predeclared,
                source,
                iota,
            )?
        };
        if !is_assignable(&checked.ty, &element)
            || checked
                .constant
                .as_ref()
                .is_some_and(|constant| !constant.is_representable_as(&element))
        {
            return Err(Diagnostic::semantic(
                format!("cannot use {:?} as array element {element:?}", checked.ty),
                source,
            ));
        }
        contains_call_or_receive |= checked.contains_call_or_receive;
    }
    Ok(CheckedOperand {
        ty,
        constant: None,
        contains_call_or_receive,
    })
}

#[allow(clippy::too_many_arguments)]
fn lower_package_length_capacity_type(
    expression: &ExprSyntax,
    constants: &BTreeMap<String, ConstantSymbol>,
    variables: &BTreeMap<String, Ty>,
    types: &BTreeMap<String, Ty>,
    shadowed_predeclared: &BTreeSet<String>,
    source: SourceRef,
    iota: Option<u64>,
) -> Result<Ty, Diagnostic> {
    check_package_type_constants(
        expression,
        constants,
        variables,
        types,
        shadowed_predeclared,
        source,
        iota,
    )?;
    crate::compiler::semantic::type_lowering::lower_type_with_constant_lookup(
        expression,
        types,
        &|name| {
            constants
                .get(name)
                .map(|constant| (constant.ty.clone(), constant.value.clone()))
                .or_else(|| {
                    (name == "iota" && !shadowed_predeclared.contains(name))
                        .then_some(iota)
                        .flatten()
                        .map(|value| {
                            (
                                Ty::Untyped(crate::compiler::types::UntypedTy::Int),
                                ConstValue::Int(value.to_string()),
                            )
                        })
                })
        },
        source,
    )
}

#[allow(clippy::too_many_arguments)]
fn check_package_type_constants(
    expression: &ExprSyntax,
    constants: &BTreeMap<String, ConstantSymbol>,
    variables: &BTreeMap<String, Ty>,
    types: &BTreeMap<String, Ty>,
    shadowed_predeclared: &BTreeSet<String>,
    source: SourceRef,
    iota: Option<u64>,
) -> Result<(), Diagnostic> {
    match &expression.kind {
        ExprSyntaxKind::Paren(inner)
        | ExprSyntaxKind::Unary {
            token: Token::MUL,
            expression: inner,
        }
        | ExprSyntaxKind::ChannelType { element: inner, .. } => check_package_type_constants(
            inner,
            constants,
            variables,
            types,
            shadowed_predeclared,
            source,
            iota,
        ),
        ExprSyntaxKind::ArrayType { length, element } => {
            if let Some(length) = length {
                let checked = check_package_operand(
                    length,
                    constants,
                    variables,
                    types,
                    shadowed_predeclared,
                    source,
                    iota,
                )?;
                if checked.constant.is_none() {
                    return Err(Diagnostic::semantic(
                        "array length must be an integer constant",
                        source,
                    ));
                }
            }
            check_package_type_constants(
                element,
                constants,
                variables,
                types,
                shadowed_predeclared,
                source,
                iota,
            )
        }
        ExprSyntaxKind::MapType { key, value } => {
            check_package_type_constants(
                key,
                constants,
                variables,
                types,
                shadowed_predeclared,
                source,
                iota,
            )?;
            check_package_type_constants(
                value,
                constants,
                variables,
                types,
                shadowed_predeclared,
                source,
                iota,
            )
        }
        ExprSyntaxKind::StructType { fields }
        | ExprSyntaxKind::InterfaceType { methods: fields } => {
            for field in fields.fields.iter() {
                if let Some(ty) = &field.ty {
                    check_package_type_constants(
                        ty,
                        constants,
                        variables,
                        types,
                        shadowed_predeclared,
                        source,
                        iota,
                    )?;
                }
            }
            Ok(())
        }
        ExprSyntaxKind::FunctionType {
            params, results, ..
        } => {
            for field in params
                .fields
                .iter()
                .chain(results.iter().flat_map(|fields| fields.fields.iter()))
            {
                if let Some(ty) = &field.ty {
                    check_package_type_constants(
                        ty,
                        constants,
                        variables,
                        types,
                        shadowed_predeclared,
                        source,
                        iota,
                    )?;
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn package_type_expression(
    expression: &ExprSyntax,
    types: &BTreeMap<String, Ty>,
    shadowed_predeclared: &BTreeSet<String>,
) -> bool {
    match &expression.kind {
        ExprSyntaxKind::Paren(inner) => package_type_expression(inner, types, shadowed_predeclared),
        ExprSyntaxKind::Ident(identifier) => {
            types.contains_key(identifier.name.as_ref())
                || (!shadowed_predeclared.contains(identifier.name.as_ref())
                    && (crate::compiler::semantic::type_lowering::predeclared_constant_type(
                        &identifier.name,
                    )
                    .is_some()
                        || matches!(identifier.name.as_ref(), "any" | "error")))
        }
        ExprSyntaxKind::ArrayType { .. }
        | ExprSyntaxKind::MapType { .. }
        | ExprSyntaxKind::ChannelType { .. }
        | ExprSyntaxKind::StructType { .. }
        | ExprSyntaxKind::InterfaceType { .. }
        | ExprSyntaxKind::FunctionType { .. }
        | ExprSyntaxKind::Unary {
            token: Token::MUL, ..
        } => true,
        _ => false,
    }
}

fn is_package_predeclared_identifier(
    expression: &ExprSyntax,
    name: &str,
    shadowed_predeclared: &BTreeSet<String>,
) -> bool {
    match &expression.kind {
        ExprSyntaxKind::Paren(inner) => {
            is_package_predeclared_identifier(inner, name, shadowed_predeclared)
        }
        ExprSyntaxKind::Ident(identifier) => {
            identifier.name.as_ref() == name && !shadowed_predeclared.contains(name)
        }
        _ => false,
    }
}
