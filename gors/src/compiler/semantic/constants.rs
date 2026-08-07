//! Evaluation of package and local Go constant expressions.

use std::collections::BTreeMap;

use num_bigint::BigInt;

use super::ConstantSymbol;
use super::expressions::*;
use super::type_lowering;
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{ExprSyntax, ExprSyntaxKind};
use crate::compiler::types::{ComplexTy, ConstValue, ExactNumber, FloatTy, Ty, UntypedTy};

pub(in crate::compiler) fn eval_constant(
    expression: &ExprSyntax,
    constants: &BTreeMap<String, ConstantSymbol>,
    type_aliases: &BTreeMap<String, Ty>,
    source: SourceRef,
    iota: u64,
) -> Result<(Ty, ConstValue), Diagnostic> {
    eval_constant_with_lookups(
        expression,
        &|name| {
            constants
                .get(name)
                .map(|constant| (constant.ty.clone(), constant.value.clone()))
        },
        &|name| type_aliases.get(name).cloned(),
        source,
        iota,
    )
}

pub(in crate::compiler) fn eval_constant_with_lookup(
    expression: &ExprSyntax,
    lookup: &impl Fn(&str) -> Option<(Ty, ConstValue)>,
    source: SourceRef,
    iota: u64,
) -> Result<(Ty, ConstValue), Diagnostic> {
    eval_constant_with_lookups(expression, lookup, &|_| None, source, iota)
}

pub(in crate::compiler) fn eval_constant_with_lookups(
    expression: &ExprSyntax,
    constant_lookup: &impl Fn(&str) -> Option<(Ty, ConstValue)>,
    type_lookup: &impl Fn(&str) -> Option<Ty>,
    source: SourceRef,
    iota: u64,
) -> Result<(Ty, ConstValue), Diagnostic> {
    match &expression.kind {
        ExprSyntaxKind::Literal { token, spelling } => match *token {
            crate::token::Token::INT => parse_go_integer(spelling)
                .map(|value| (Ty::Untyped(UntypedTy::Int), ConstValue::Int(value)))
                .ok_or_else(|| {
                    Diagnostic::semantic(format!("invalid integer literal {spelling}"), source)
                }),
            crate::token::Token::FLOAT => ExactNumber::from_spelling(spelling)
                .map(|value| (Ty::Untyped(UntypedTy::Float), ConstValue::Float(value)))
                .ok_or_else(|| {
                    Diagnostic::semantic(
                        format!("invalid floating-point literal {spelling}"),
                        source,
                    )
                }),
            crate::token::Token::IMAG => parse_go_imaginary(spelling)
                .map(|imag| {
                    (
                        Ty::Untyped(UntypedTy::Complex),
                        ConstValue::Complex {
                            real: ExactNumber::zero(),
                            imag,
                        },
                    )
                })
                .ok_or_else(|| Diagnostic::semantic("invalid imaginary literal", source)),
            crate::token::Token::STRING => parse_go_string(spelling)
                .map(|value| (Ty::Untyped(UntypedTy::String), ConstValue::String(value)))
                .ok_or_else(|| Diagnostic::semantic("invalid string literal", source)),
            crate::token::Token::CHAR => parse_go_rune(spelling)
                .map(|value| {
                    (
                        Ty::Untyped(UntypedTy::Rune),
                        ConstValue::Int(value.to_string()),
                    )
                })
                .ok_or_else(|| Diagnostic::semantic("invalid rune literal", source)),
            token => Err(Diagnostic::unsupported(
                format!("literal kind {token:?} is not implemented"),
                source,
            )),
        },
        ExprSyntaxKind::Ident(ident) => {
            if let Some(constant) = constant_lookup(ident.name.as_ref()) {
                Ok(constant)
            } else if matches!(ident.name.as_ref(), "true" | "false") {
                Ok((
                    Ty::Untyped(UntypedTy::Bool),
                    ConstValue::Bool(ident.name.as_ref() == "true"),
                ))
            } else if ident.name.as_ref() == "iota" {
                Ok((
                    Ty::Untyped(UntypedTy::Int),
                    ConstValue::Int(iota.to_string()),
                ))
            } else {
                Err(Diagnostic::semantic(
                    format!("{} is not a constant", ident.name),
                    source,
                ))
            }
        }
        ExprSyntaxKind::Binary { left, token, right } => {
            let (left_ty, left) =
                eval_constant_with_lookups(left, constant_lookup, type_lookup, source, iota)?;
            let (right_ty, right) =
                eval_constant_with_lookups(right, constant_lookup, type_lookup, source, iota)?;
            let op = lower_binary_op(*token).ok_or_else(|| {
                Diagnostic::unsupported(
                    format!("constant operator {token:?} is not implemented"),
                    source,
                )
            })?;
            if matches!(op, hir::BinaryOp::Shl | hir::BinaryOp::Shr)
                && matches!(left_ty, Ty::Untyped(_))
                && (right_ty.is_integer() || matches!(right_ty, Ty::Untyped(_)))
            {
                let value = fold_untyped_constant_shift(op, &left, &right, source)?;
                return Ok((Ty::Untyped(UntypedTy::Int), value));
            }
            let operand_ty = exact_common_operand_type(&left_ty, &right_ty).ok_or_else(|| {
                Diagnostic::semantic(
                    format!("incompatible constant operands {left_ty:?} and {right_ty:?}"),
                    source,
                )
            })?;
            validate_constant_binary_operator(op, &operand_ty, source)?;
            // An untyped constant operand first converts to the common
            // operand type, so an integral float spelling participates in
            // integer arithmetic at integer operand types.
            let left = normalize_constant_for_type(left, &operand_ty, source)?;
            let right = normalize_constant_for_type(right, &operand_ty, source)?;
            let value = fold_constant_binary(op, &left, &right, source)?.ok_or_else(|| {
                Diagnostic::unsupported("this constant operation is not yet supported", source)
            })?;
            let result_ty = if matches!(
                op,
                hir::BinaryOp::Equal
                    | hir::BinaryOp::NotEqual
                    | hir::BinaryOp::Less
                    | hir::BinaryOp::LessEqual
                    | hir::BinaryOp::Greater
                    | hir::BinaryOp::GreaterEqual
                    | hir::BinaryOp::LogicalAnd
                    | hir::BinaryOp::LogicalOr
            ) {
                Ty::Untyped(UntypedTy::Bool)
            } else {
                operand_ty
            };
            let value = normalize_constant_for_type(value, &result_ty, source)?;
            Ok((result_ty, value))
        }
        ExprSyntaxKind::Paren(expression) => {
            eval_constant_with_lookups(expression, constant_lookup, type_lookup, source, iota)
        }
        ExprSyntaxKind::Unary { token, expression } => {
            let (ty, value) =
                eval_constant_with_lookups(expression, constant_lookup, type_lookup, source, iota)?;
            let value = match (*token, value) {
                (crate::token::Token::ADD, value) => value,
                (crate::token::Token::SUB, ConstValue::Int(value)) => {
                    let value = BigInt::parse_bytes(value.as_bytes(), 10)
                        .map(|value| (-value).to_string())
                        .ok_or_else(|| Diagnostic::semantic("invalid exact integer", source))?;
                    ConstValue::Int(value)
                }
                (crate::token::Token::SUB, ConstValue::Float(value)) => {
                    ConstValue::Float(value.negated())
                }
                (crate::token::Token::SUB, ConstValue::Complex { real, imag }) => {
                    ConstValue::Complex {
                        real: real.negated(),
                        imag: imag.negated(),
                    }
                }
                (crate::token::Token::NOT, ConstValue::Bool(value)) => ConstValue::Bool(!value),
                _ => {
                    return Err(Diagnostic::unsupported(
                        "constant unary operation is not implemented",
                        source,
                    ));
                }
            };
            let value = normalize_constant_for_type(value, &ty, source)?;
            Ok((ty, value))
        }
        ExprSyntaxKind::Call {
            callee,
            arguments,
            spread,
        } => eval_constant_call(
            callee,
            arguments,
            *spread,
            constant_lookup,
            type_lookup,
            source,
            iota,
        ),
        ExprSyntaxKind::FunctionLiteral { .. }
        | ExprSyntaxKind::FunctionType { .. }
        | ExprSyntaxKind::Selector { .. }
        | ExprSyntaxKind::TypeAssert { .. }
        | ExprSyntaxKind::ArrayType { .. }
        | ExprSyntaxKind::MapType { .. }
        | ExprSyntaxKind::ChannelType { .. }
        | ExprSyntaxKind::StructType { .. }
        | ExprSyntaxKind::InterfaceType { .. }
        | ExprSyntaxKind::KeyValue { .. }
        | ExprSyntaxKind::CompositeLiteral { .. }
        | ExprSyntaxKind::Index { .. }
        | ExprSyntaxKind::IndexList { .. }
        | ExprSyntaxKind::Slice { .. }
        | ExprSyntaxKind::Unsupported(_) => Err(Diagnostic::unsupported(
            "this constant expression is not yet supported",
            source,
        )),
    }
}

pub(in crate::compiler) fn validate_constant_binary_operator(
    op: hir::BinaryOp,
    ty: &Ty,
    source: SourceRef,
) -> Result<(), Diagnostic> {
    let ty = ty.default_typed();
    let ty = ty.underlying();
    let ordered = ty.is_integer() || matches!(ty, Ty::Float(_) | Ty::String);
    let valid = match op {
        hir::BinaryOp::LogicalAnd | hir::BinaryOp::LogicalOr => *ty == Ty::Bool,
        hir::BinaryOp::Equal | hir::BinaryOp::NotEqual => {
            *ty == Ty::Bool || ty.is_numeric() || *ty == Ty::String
        }
        hir::BinaryOp::Less
        | hir::BinaryOp::LessEqual
        | hir::BinaryOp::Greater
        | hir::BinaryOp::GreaterEqual
        | hir::BinaryOp::Min
        | hir::BinaryOp::Max => ordered,
        hir::BinaryOp::Add => ty.is_numeric() || *ty == Ty::String,
        hir::BinaryOp::Sub | hir::BinaryOp::Mul | hir::BinaryOp::Div => ty.is_numeric(),
        hir::BinaryOp::Rem
        | hir::BinaryOp::BitAnd
        | hir::BinaryOp::BitOr
        | hir::BinaryOp::BitXor
        | hir::BinaryOp::Shl
        | hir::BinaryOp::Shr
        | hir::BinaryOp::AndNot => ty.is_integer(),
        hir::BinaryOp::Complex => matches!(ty, Ty::Int(_) | Ty::Uint(_) | Ty::Float(_)),
    };
    if valid {
        Ok(())
    } else {
        Err(Diagnostic::semantic(
            format!("operator {op:?} is invalid for constant type {ty:?}"),
            source,
        ))
    }
}

#[allow(clippy::too_many_arguments)]
fn eval_constant_call(
    callee: &ExprSyntax,
    arguments: &[ExprSyntax],
    spread: bool,
    constant_lookup: &impl Fn(&str) -> Option<(Ty, ConstValue)>,
    type_lookup: &impl Fn(&str) -> Option<Ty>,
    source: SourceRef,
    iota: u64,
) -> Result<(Ty, ConstValue), Diagnostic> {
    let ExprSyntaxKind::Ident(callee) = &callee.kind else {
        return Err(Diagnostic::semantic(
            "constant call target must be a predeclared function or type",
            source,
        ));
    };
    if spread {
        return Err(Diagnostic::semantic(
            "... is not permitted in a constant expression",
            source,
        ));
    }
    let name = callee.name.as_ref();
    if let Some(target) =
        type_lookup(name).or_else(|| type_lowering::predeclared_constant_type(name))
    {
        let [argument] = arguments else {
            return Err(Diagnostic::semantic(
                format!("conversion to {name} requires exactly one argument"),
                source,
            ));
        };
        let (actual, mut value) =
            eval_constant_with_lookups(argument, constant_lookup, type_lookup, source, iota)?;
        if !is_assignable(&actual, &target) || !value.is_representable_as(&target) {
            return Err(Diagnostic::semantic(
                format!("constant is not representable as {target:?}"),
                source,
            ));
        }
        value = value.normalized_for(&target);
        return Ok((target, value));
    }
    match name {
        "len" => {
            let [argument] = arguments else {
                return Err(Diagnostic::semantic(
                    "constant len requires exactly one argument",
                    source,
                ));
            };
            let (_, value) =
                eval_constant_with_lookups(argument, constant_lookup, type_lookup, source, iota)?;
            let ConstValue::String(value) = value else {
                return Err(Diagnostic::semantic(
                    "constant len currently requires a constant string",
                    source,
                ));
            };
            Ok((
                Ty::Untyped(UntypedTy::Int),
                ConstValue::Int(value.len().to_string()),
            ))
        }
        "min" | "max" => {
            eval_constant_min_max(name, arguments, constant_lookup, type_lookup, source, iota)
        }
        "complex" => eval_constant_complex(arguments, constant_lookup, type_lookup, source, iota),
        "real" | "imag" => eval_constant_complex_component(
            name,
            arguments,
            constant_lookup,
            type_lookup,
            source,
            iota,
        ),
        _ => Err(Diagnostic::semantic(
            format!("{name} is not a constant function or type"),
            source,
        )),
    }
}

#[allow(clippy::too_many_arguments)]
fn eval_constant_min_max(
    name: &str,
    arguments: &[ExprSyntax],
    constant_lookup: &impl Fn(&str) -> Option<(Ty, ConstValue)>,
    type_lookup: &impl Fn(&str) -> Option<Ty>,
    source: SourceRef,
    iota: u64,
) -> Result<(Ty, ConstValue), Diagnostic> {
    let mut arguments = arguments.iter();
    let first = arguments.next().ok_or_else(|| {
        Diagnostic::semantic(
            format!("call to {name} requires at least one argument"),
            source,
        )
    })?;
    let (mut ty, mut value) =
        eval_constant_with_lookups(first, constant_lookup, type_lookup, source, iota)?;
    let op = if name == "min" {
        hir::BinaryOp::Min
    } else {
        hir::BinaryOp::Max
    };
    for argument in arguments {
        let (right_ty, right) =
            eval_constant_with_lookups(argument, constant_lookup, type_lookup, source, iota)?;
        let common = exact_common_operand_type(&ty, &right_ty).ok_or_else(|| {
            Diagnostic::semantic(
                format!("incompatible {name} operands {ty:?} and {right_ty:?}"),
                source,
            )
        })?;
        if !(common.is_numeric() || common.default_typed().underlying() == &Ty::String)
            || matches!(
                common.default_typed().underlying(),
                Ty::Complex(ComplexTy::Complex64 | ComplexTy::Complex128)
            )
        {
            return Err(Diagnostic::semantic(
                format!("{name} requires ordered arguments"),
                source,
            ));
        }
        let left = normalize_constant_for_type(value, &common, source)?;
        let right = normalize_constant_for_type(right, &common, source)?;
        value = fold_constant_binary(op, &left, &right, source)?
            .ok_or_else(|| Diagnostic::semantic(format!("invalid constant {name}"), source))?;
        value = normalize_constant_for_type(value, &common, source)?;
        ty = common;
    }
    Ok((ty, value))
}

#[allow(clippy::too_many_arguments)]
fn eval_constant_complex(
    arguments: &[ExprSyntax],
    constant_lookup: &impl Fn(&str) -> Option<(Ty, ConstValue)>,
    type_lookup: &impl Fn(&str) -> Option<Ty>,
    source: SourceRef,
    iota: u64,
) -> Result<(Ty, ConstValue), Diagnostic> {
    let [real, imag] = arguments else {
        return Err(Diagnostic::semantic(
            "call to complex requires exactly two arguments",
            source,
        ));
    };
    let (real_ty, real) =
        eval_constant_with_lookups(real, constant_lookup, type_lookup, source, iota)?;
    let (imag_ty, imag) =
        eval_constant_with_lookups(imag, constant_lookup, type_lookup, source, iota)?;
    let component_ty = exact_common_operand_type(&real_ty, &imag_ty).ok_or_else(|| {
        Diagnostic::semantic(
            format!("incompatible complex components {real_ty:?} and {imag_ty:?}"),
            source,
        )
    })?;
    let component = |value: ConstValue| {
        value.exact_number().ok_or_else(|| {
            Diagnostic::semantic("complex requires real numeric constant arguments", source)
        })
    };
    let ty = match component_ty.default_typed().underlying() {
        Ty::Float(FloatTy::Float32) => Ty::Complex(ComplexTy::Complex64),
        Ty::Int(_) | Ty::Uint(_) | Ty::Float(FloatTy::Float64)
            if matches!(component_ty, Ty::Untyped(_)) =>
        {
            Ty::Untyped(UntypedTy::Complex)
        }
        Ty::Float(FloatTy::Float64) => Ty::Complex(ComplexTy::Complex128),
        _ => {
            return Err(Diagnostic::semantic(
                "complex requires floating-point arguments or numeric constants",
                source,
            ));
        }
    };
    Ok((
        ty,
        ConstValue::Complex {
            real: component(real)?,
            imag: component(imag)?,
        },
    ))
}

#[allow(clippy::too_many_arguments)]
fn eval_constant_complex_component(
    name: &str,
    arguments: &[ExprSyntax],
    constant_lookup: &impl Fn(&str) -> Option<(Ty, ConstValue)>,
    type_lookup: &impl Fn(&str) -> Option<Ty>,
    source: SourceRef,
    iota: u64,
) -> Result<(Ty, ConstValue), Diagnostic> {
    let [argument] = arguments else {
        return Err(Diagnostic::semantic(
            format!("call to {name} requires exactly one argument"),
            source,
        ));
    };
    let (argument_ty, argument) =
        eval_constant_with_lookups(argument, constant_lookup, type_lookup, source, iota)?;
    let ConstValue::Complex { real, imag } = argument else {
        return Err(Diagnostic::semantic(
            format!("{name} requires a complex argument"),
            source,
        ));
    };
    let ty = match argument_ty {
        Ty::Untyped(_) => Ty::Untyped(UntypedTy::Float),
        Ty::Complex(ComplexTy::Complex64) => Ty::Float(FloatTy::Float32),
        Ty::Complex(ComplexTy::Complex128) => Ty::Float(FloatTy::Float64),
        _ => {
            return Err(Diagnostic::semantic(
                format!("{name} requires a complex argument"),
                source,
            ));
        }
    };
    Ok((
        ty,
        ConstValue::Float(if name == "real" { real } else { imag }),
    ))
}
