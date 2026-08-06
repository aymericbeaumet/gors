//! Definition-demanded name resolution and type checking over owned syntax.

mod assignments;
mod closures;
mod expression_lower;
mod expressions;
mod function;
mod maps;
mod numeric_builtins;
mod pointers;
mod ranges;
mod statements;
mod switches;

use std::collections::{BTreeMap, BTreeSet};

use num_bigint::BigInt;

use expressions::*;
use function::FunctionLowerer;

use super::Diagnostic;
use super::hir;
use super::ids::{DefId, NodeId};
use super::provenance::SourceRef;
use super::syntax::{
    ConstantSyntax, ConstantValueSyntax, ExprSyntax, ExprSyntaxKind, FieldListSyntax,
    FunctionBodySyntax, FunctionHeaderSyntax, SyntaxSource,
};
use super::types::{ConstValue, IntTy, Signature, Ty, UntypedTy};

#[derive(Clone)]
pub(super) struct FunctionSymbol {
    pub(super) id: DefId,
    pub(super) signature: Signature,
}

#[derive(Clone)]
pub(super) struct ConstantSymbol {
    pub(super) id: DefId,
    pub(super) ty: Ty,
    pub(super) value: ConstValue,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct TypedConstant {
    pub(super) id: DefId,
    pub(super) name: String,
    pub(super) ty: Ty,
    pub(super) value: ConstValue,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct LoweredFunction {
    pub(super) function: hir::Function,
    pub(super) source_plan: Vec<(SourceRef, SyntaxSource)>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct FunctionLoweringFailure {
    pub(super) diagnostic: Diagnostic,
    pub(super) source_plan: Vec<(SourceRef, SyntaxSource)>,
}

pub(super) fn lower_signature(
    definition: DefId,
    header: &FunctionHeaderSyntax,
    type_aliases: &BTreeMap<String, Ty>,
) -> Result<Signature, Diagnostic> {
    let source = SourceRef::definition(definition);
    if header.has_receiver {
        return Err(Diagnostic::unsupported(
            "methods are not implemented by the HIR/MIR backend",
            source,
        ));
    }
    if header.has_type_parameters {
        return Err(Diagnostic::unsupported(
            "generic functions are not implemented by the HIR/MIR backend",
            source,
        ));
    }
    let (params, variadic) = parameter_types(&header.params, type_aliases, source)?;
    let results = header
        .results
        .as_ref()
        .map(|fields| field_types(fields, type_aliases, source))
        .transpose()?
        .unwrap_or_default();
    if header.name.name.as_ref() == "init" {
        return Err(Diagnostic::unsupported(
            "package init functions are not implemented by the HIR/MIR backend",
            source,
        ));
    }
    if header.name.name.as_ref() == "main" && (!params.is_empty() || !results.is_empty()) {
        return Err(Diagnostic::semantic(
            "func main must have no parameters and no results",
            source,
        ));
    }
    Ok(Signature {
        params,
        results,
        variadic,
    })
}

pub(super) fn lower_constant(
    definition: DefId,
    syntax: &ConstantSyntax,
    constants: &BTreeMap<String, ConstantSymbol>,
    type_aliases: &BTreeMap<String, Ty>,
) -> Result<TypedConstant, Diagnostic> {
    let source = SourceRef::definition(definition);
    let expression = match &syntax.value {
        ConstantValueSyntax::Expression(expression) => expression,
        ConstantValueSyntax::ImplicitOrIota => {
            return Err(Diagnostic::unsupported(
                "implicit repeated const expressions and iota are not implemented",
                source,
            ));
        }
        ConstantValueSyntax::ArityMismatch => {
            return Err(Diagnostic::unsupported(
                "multi-valued const expressions are not implemented",
                source,
            ));
        }
    };
    let (raw_ty, value) = eval_constant(expression, constants, source, syntax.iota)?;
    let ty = syntax
        .explicit_type
        .as_ref()
        .map(|ty| lower_type(ty, type_aliases, source))
        .transpose()?
        .unwrap_or_else(|| raw_ty.clone());
    ensure_bootstrap_value_type(&ty.default_typed(), source)?;
    if !is_assignable(&raw_ty, &ty) {
        return Err(Diagnostic::semantic(
            format!("constant {} is not assignable to {ty:?}", syntax.name.name),
            source,
        ));
    }
    if !value.is_representable_as(&ty) {
        return Err(Diagnostic::semantic(
            format!(
                "constant {} is not representable as {ty:?}",
                syntax.name.name
            ),
            source,
        ));
    }
    Ok(TypedConstant {
        id: definition,
        name: syntax.name.name.to_string(),
        ty,
        value,
    })
}

pub(super) fn lower_function(
    definition: DefId,
    header: &FunctionHeaderSyntax,
    body: &FunctionBodySyntax,
    signature: Signature,
    functions: BTreeMap<String, FunctionSymbol>,
    constants: BTreeMap<String, ConstantSymbol>,
    type_aliases: BTreeMap<String, Ty>,
) -> Result<LoweredFunction, FunctionLoweringFailure> {
    let node = NodeId::owner_local(definition, 0);
    let initial_source_plan = vec![
        (SourceRef::definition(definition), header.name.source),
        (SourceRef::node(node), header.name.source),
    ];
    let Some(body) = body.block.as_ref() else {
        return Err(FunctionLoweringFailure {
            diagnostic: Diagnostic::unsupported(
                "bodyless declarations require an explicit runtime intrinsic",
                SourceRef::definition(definition),
            ),
            source_plan: initial_source_plan,
        });
    };
    let mut lowerer = FunctionLowerer {
        owner: definition,
        next_node: 1,
        functions,
        constants,
        type_aliases,
        signature: signature.clone(),
        locals: Vec::new(),
        scopes: vec![BTreeMap::new()],
        closures: Vec::new(),
        closure_scopes: vec![BTreeMap::new()],
        named_results: Vec::new(),
        loop_labels: Vec::new(),
        declared_labels: BTreeSet::new(),
        referenced_gotos: BTreeMap::new(),
        defer_registration_depth: 0,
        inside_deferred_closure: false,
        inside_local_closure: false,
        source_plan: initial_source_plan,
    };
    let lowered = (|| {
        let params = lowerer.declare_field_bindings(
            &header.params,
            &signature.params,
            hir::LocalKind::Parameter,
        )?;
        lowerer.named_results = header.results.as_ref().map_or_else(
            || Ok(Vec::new()),
            |results| lowerer.declare_result_bindings(results, &signature.results),
        )?;
        let body = lowerer.lower_block(body, false)?;
        if let Some((label, source)) = lowerer
            .referenced_gotos
            .iter()
            .find(|(label, _)| !lowerer.declared_labels.contains(*label))
        {
            return Err(Diagnostic::semantic(
                format!("goto target {label} is not defined"),
                *source,
            ));
        }
        Ok((params, body))
    })();
    match lowered {
        Ok((params, body)) => Ok(LoweredFunction {
            function: hir::Function {
                id: definition,
                node,
                name: header.name.name.to_string(),
                signature,
                params,
                named_results: lowerer.named_results,
                locals: lowerer.locals,
                closures: lowerer.closures,
                body,
                source: SourceRef::definition(definition),
            },
            source_plan: lowerer.source_plan,
        }),
        Err(diagnostic) => Err(FunctionLoweringFailure {
            diagnostic,
            source_plan: lowerer.source_plan,
        }),
    }
}

fn field_types(
    fields: &FieldListSyntax,
    type_aliases: &BTreeMap<String, Ty>,
    source: SourceRef,
) -> Result<Vec<Ty>, Diagnostic> {
    let mut result = Vec::new();
    for field in &*fields.fields {
        if field.variadic {
            return Err(Diagnostic::semantic(
                "result parameters cannot be variadic",
                source,
            ));
        }
        let type_expression = field
            .ty
            .as_ref()
            .ok_or_else(|| Diagnostic::backend("signature field has no type"))?;
        let ty = lower_type(type_expression, type_aliases, source)?;
        let count = field.names.as_ref().map_or(1, |names| names.len());
        result.extend(std::iter::repeat_n(ty, count));
    }
    Ok(result)
}

fn parameter_types(
    fields: &FieldListSyntax,
    type_aliases: &BTreeMap<String, Ty>,
    source: SourceRef,
) -> Result<(Vec<Ty>, bool), Diagnostic> {
    let mut result = Vec::new();
    let mut variadic = false;
    for (index, field) in fields.fields.iter().enumerate() {
        let type_expression = field
            .ty
            .as_ref()
            .ok_or_else(|| Diagnostic::backend("signature field has no type"))?;
        let mut ty = lower_type(type_expression, type_aliases, source)?;
        let count = field.names.as_ref().map_or(1, |names| names.len());
        if field.variadic {
            if variadic || index + 1 != fields.fields.len() || count != 1 {
                return Err(Diagnostic::semantic(
                    "a variadic parameter must be the final single parameter",
                    source,
                ));
            }
            variadic = true;
            ty = Ty::Slice(Box::new(ty));
        }
        result.extend(std::iter::repeat_n(ty, count));
    }
    Ok((result, variadic))
}

pub(super) fn lower_type(
    expression: &ExprSyntax,
    type_aliases: &BTreeMap<String, Ty>,
    source: SourceRef,
) -> Result<Ty, Diagnostic> {
    if let ExprSyntaxKind::ArrayType {
        length: None,
        element,
    } = &expression.kind
    {
        return Ok(Ty::Slice(Box::new(lower_type(
            element,
            type_aliases,
            source,
        )?)));
    }
    if let ExprSyntaxKind::MapType { key, value } = &expression.kind {
        return Ok(Ty::Map(
            Box::new(lower_type(key, type_aliases, source)?),
            Box::new(lower_type(value, type_aliases, source)?),
        ));
    }
    if let ExprSyntaxKind::Unary {
        token: crate::token::Token::MUL,
        expression,
    } = &expression.kind
    {
        return Ok(Ty::Pointer(Box::new(lower_type(
            expression,
            type_aliases,
            source,
        )?)));
    }
    let ExprSyntaxKind::Ident(ident) = &expression.kind else {
        return Err(Diagnostic::unsupported(
            "this Go type is not yet implemented by the typed backend",
            source,
        ));
    };
    match ident.name.as_ref() {
        "bool" => Ok(Ty::Bool),
        "string" => Ok(Ty::String),
        "int" => Ok(Ty::Int(IntTy::Int)),
        "float64" => Ok(Ty::Float(super::types::FloatTy::Float64)),
        "complex128" => Ok(Ty::Complex(super::types::ComplexTy::Complex128)),
        "uint8" | "byte" => Ok(Ty::Uint(super::types::UintTy::Uint8)),
        "int8" | "int16" | "int32" | "rune" | "int64" | "uint" | "uint16" | "uint32" | "uint64"
        | "uintptr" | "float32" | "complex64" => Err(Diagnostic::unsupported(
            format!(
                "type {} is outside the bootstrap bool/int/string runtime frontier",
                ident.name
            ),
            source,
        )),
        other => type_aliases.get(other).cloned().ok_or_else(|| {
            Diagnostic::unsupported(
                format!("type {other} is not implemented by the HIR/MIR backend"),
                source,
            )
        }),
    }
}

pub(super) fn eval_constant(
    expression: &ExprSyntax,
    constants: &BTreeMap<String, ConstantSymbol>,
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
            crate::token::Token::FLOAT => Ok((
                Ty::Untyped(UntypedTy::Float),
                ConstValue::Float(spelling.replace('_', "")),
            )),
            crate::token::Token::IMAG => {
                let component = spelling
                    .strip_suffix('i')
                    .ok_or_else(|| Diagnostic::semantic("invalid imaginary literal", source))?;
                let imag =
                    parse_go_integer(component).unwrap_or_else(|| component.replace('_', ""));
                Ok((
                    Ty::Untyped(UntypedTy::Complex),
                    ConstValue::Complex {
                        real: "0".into(),
                        imag,
                    },
                ))
            }
            crate::token::Token::STRING => parse_go_string(spelling)
                .map(|value| (Ty::Untyped(UntypedTy::String), ConstValue::String(value)))
                .ok_or_else(|| Diagnostic::semantic("invalid string literal", source)),
            crate::token::Token::CHAR => parse_go_rune(spelling)
                .map(|value| {
                    (
                        Ty::Untyped(UntypedTy::Int),
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
            if let Some(constant) = constants.get(ident.name.as_ref()) {
                Ok((constant.ty.clone(), constant.value.clone()))
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
            let (left_ty, left) = eval_constant(left, constants, source, iota)?;
            let (right_ty, right) = eval_constant(right, constants, source, iota)?;
            let op = lower_binary_op(*token).ok_or_else(|| {
                Diagnostic::unsupported(
                    format!("constant operator {token:?} is not implemented"),
                    source,
                )
            })?;
            let operand_ty = exact_common_operand_type(&left_ty, &right_ty).ok_or_else(|| {
                Diagnostic::semantic(
                    format!("incompatible constant operands {left_ty:?} and {right_ty:?}"),
                    source,
                )
            })?;
            ensure_bootstrap_value_type(&operand_ty.default_typed(), source)?;
            validate_binary_operator(op, &operand_ty.default_typed(), source)?;
            let value = fold_constant_binary(op, &left, &right, source)?.ok_or_else(|| {
                Diagnostic::unsupported(
                    "constant operation is not implemented by the bootstrap evaluator",
                    source,
                )
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
            Ok((result_ty, value))
        }
        ExprSyntaxKind::Paren(expression) => eval_constant(expression, constants, source, iota),
        ExprSyntaxKind::Unary { token, expression } => {
            let (ty, value) = eval_constant(expression, constants, source, iota)?;
            match (*token, value) {
                (crate::token::Token::ADD, value) => Ok((ty, value)),
                (crate::token::Token::SUB, ConstValue::Int(value)) => {
                    let value = BigInt::parse_bytes(value.as_bytes(), 10)
                        .map(|value| (-value).to_string())
                        .ok_or_else(|| Diagnostic::semantic("invalid exact integer", source))?;
                    Ok((ty, ConstValue::Int(value)))
                }
                (crate::token::Token::SUB, ConstValue::Float(value)) => {
                    let value = value
                        .strip_prefix('-')
                        .map_or_else(|| format!("-{value}"), str::to_string);
                    Ok((ty, ConstValue::Float(value)))
                }
                (crate::token::Token::SUB, ConstValue::Complex { real, imag }) => Ok((
                    ty,
                    ConstValue::Complex {
                        real: negate_number_spelling(&real),
                        imag: negate_number_spelling(&imag),
                    },
                )),
                (crate::token::Token::NOT, ConstValue::Bool(value)) => {
                    Ok((ty, ConstValue::Bool(!value)))
                }
                _ => Err(Diagnostic::unsupported(
                    "constant unary operation is not implemented",
                    source,
                )),
            }
        }
        ExprSyntaxKind::Call { .. }
        | ExprSyntaxKind::FunctionLiteral { .. }
        | ExprSyntaxKind::Selector { .. }
        | ExprSyntaxKind::ArrayType { .. }
        | ExprSyntaxKind::MapType { .. }
        | ExprSyntaxKind::KeyValue { .. }
        | ExprSyntaxKind::CompositeLiteral { .. }
        | ExprSyntaxKind::Index { .. }
        | ExprSyntaxKind::Slice { .. }
        | ExprSyntaxKind::Unsupported(_) => Err(Diagnostic::unsupported(
            "constant expression is not implemented by the HIR/MIR backend",
            source,
        )),
    }
}

fn negate_number_spelling(value: &str) -> String {
    value
        .strip_prefix('-')
        .map_or_else(|| format!("-{value}"), str::to_string)
}
