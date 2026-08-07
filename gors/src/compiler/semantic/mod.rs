//! Definition-demanded name resolution and type checking over owned syntax.

mod arrays;
mod assignments;
mod calls;
mod channels;
mod closures;
mod composites;
mod conversions;
mod expression_lower;
mod expressions;
mod function;
mod generics;
mod goroutines;
mod goto_scopes;
mod imports;
mod interfaces;
mod iteration;
mod maps;
mod numeric_builtins;
mod pointers;
mod ranges;
mod recovery;
mod selects;
mod slices;
mod statements;
mod static_values;
mod structs;
mod switches;
mod type_lowering;
mod type_switches;
mod unsafe_intrinsics;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use num_bigint::BigInt;

use expressions::*;
use function::FunctionLowerer;
use type_lowering::{
    field_types, field_types_with_constant_lookup, parameter_types,
    parameter_types_with_constant_lookup,
};
pub(super) use type_lowering::{
    lower_type, lower_type_with_constant_lookup, lower_type_with_constants,
};

use super::Diagnostic;
use super::hir;
use super::ids::{DefId, NodeId, QualifiedDefId};
use super::provenance::SourceRef;
use super::syntax::{
    ConstantSyntax, ConstantValueSyntax, ExprSyntax, ExprSyntaxKind, FieldListSyntax,
    FunctionBodySyntax, FunctionHeaderSyntax, SyntaxSource, VariableSyntax, VariableValueSyntax,
};
use super::types::{ComplexTy, ConstValue, FloatTy, Signature, StaticValue, Ty, UntypedTy};

#[derive(Clone)]
pub(super) struct FunctionSymbol {
    pub(super) id: QualifiedDefId,
    pub(super) signature: Signature,
    pub(super) range_header: Option<Arc<FunctionHeaderSyntax>>,
    pub(super) range_body: Option<Arc<FunctionBodySyntax>>,
}

#[derive(Clone)]
pub(super) struct MethodSymbol {
    pub(super) id: QualifiedDefId,
    pub(super) signature: Signature,
    pub(super) pointer_receiver: bool,
}

#[derive(Clone)]
pub(super) struct GenericFunctionSymbol {
    pub(super) id: QualifiedDefId,
    pub(super) header: Arc<FunctionHeaderSyntax>,
    pub(super) body: Arc<FunctionBodySyntax>,
    pub(super) pointer_receiver: bool,
}

#[derive(Clone)]
pub(super) struct GenericTypeSymbol {
    pub(super) id: DefId,
    pub(super) type_parameters: Arc<FieldListSyntax>,
    pub(super) underlying: ExprSyntax,
    pub(super) alias: bool,
}

#[derive(Clone)]
pub(super) struct ConstantSymbol {
    pub(super) id: QualifiedDefId,
    pub(super) ty: Ty,
    pub(super) value: ConstValue,
}

#[derive(Clone)]
pub(super) struct VariableSymbol {
    pub(super) id: QualifiedDefId,
    pub(super) ty: Ty,
    pub(super) value: StaticValue,
}

pub(super) struct FunctionSymbols {
    pub(super) functions: BTreeMap<String, FunctionSymbol>,
    pub(super) qualified_functions: BTreeMap<(String, String), FunctionSymbol>,
    pub(super) methods: BTreeMap<(DefId, String), MethodSymbol>,
    pub(super) generic_functions: BTreeMap<String, GenericFunctionSymbol>,
    pub(super) generic_methods: BTreeMap<(DefId, String), GenericFunctionSymbol>,
    pub(super) generic_types: BTreeMap<String, GenericTypeSymbol>,
    pub(super) constants: BTreeMap<String, ConstantSymbol>,
    pub(super) qualified_constants: BTreeMap<(String, String), ConstantSymbol>,
    pub(super) variables: BTreeMap<String, VariableSymbol>,
    pub(super) qualified_variables: BTreeMap<(String, String), VariableSymbol>,
    pub(super) intrinsic_packages: BTreeSet<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct TypedConstant {
    pub(super) id: DefId,
    pub(super) name: String,
    pub(super) ty: Ty,
    pub(super) value: ConstValue,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct TypedVariable {
    pub(super) id: DefId,
    pub(super) name: String,
    pub(super) ty: Ty,
    pub(super) value: StaticValue,
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
    constants: &BTreeMap<String, ConstantSymbol>,
) -> Result<Signature, Diagnostic> {
    let source = SourceRef::definition(definition);
    let constant_lookup = |name: &str| {
        constants
            .get(name)
            .map(|constant| (constant.ty.clone(), constant.value.clone()))
    };
    if header.has_type_parameters {
        return Err(Diagnostic::unsupported(
            "generic functions with this declaration form are not yet supported",
            source,
        ));
    }
    let mut params = Vec::new();
    if let Some(receiver) = &header.receiver {
        let (receiver, receiver_variadic) =
            parameter_types_with_constant_lookup(receiver, type_aliases, &constant_lookup, source)?;
        if receiver_variadic || receiver.len() != 1 {
            return Err(Diagnostic::semantic(
                "a method must declare exactly one non-variadic receiver",
                source,
            ));
        }
        params.extend(receiver);
    }
    let (ordinary_params, variadic) = parameter_types_with_constant_lookup(
        &header.params,
        type_aliases,
        &constant_lookup,
        source,
    )?;
    params.extend(ordinary_params);
    let results = header
        .results
        .as_ref()
        .map(|fields| {
            field_types_with_constant_lookup(fields, type_aliases, &constant_lookup, source)
        })
        .transpose()?
        .unwrap_or_default();
    for ty in params.iter().chain(&results) {
        ensure_bootstrap_value_type(ty, source)?;
    }
    if header.receiver.is_none()
        && header.name.name.as_ref() == "main"
        && (!params.is_empty() || !results.is_empty())
    {
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
    let (raw_ty, value) = eval_constant(expression, constants, type_aliases, source, syntax.iota)?;
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
    let value = value.normalized_for(&ty);
    Ok(TypedConstant {
        id: definition,
        name: syntax.name.name.to_string(),
        ty,
        value,
    })
}

pub(super) fn lower_variable(
    definition: DefId,
    syntax: &VariableSyntax,
    constants: &BTreeMap<String, ConstantSymbol>,
    type_aliases: &BTreeMap<String, Ty>,
    static_functions: &BTreeMap<String, Arc<FunctionBodySyntax>>,
    package_initializers: &[Arc<FunctionBodySyntax>],
) -> Result<TypedVariable, Diagnostic> {
    let source = SourceRef::definition(definition);
    let explicit_ty = syntax
        .explicit_type
        .as_ref()
        .map(|ty| lower_type_with_constants(ty, type_aliases, constants, source))
        .transpose()?;
    let (raw_ty, mut value) = match &syntax.value {
        VariableValueSyntax::Expression(expression) => static_values::evaluate_initializer(
            expression,
            constants,
            type_aliases,
            static_functions,
            source,
        )?,
        VariableValueSyntax::Zero => {
            let ty = explicit_ty.clone().ok_or_else(|| {
                Diagnostic::semantic(
                    format!(
                        "variable {} has neither a type nor an initializer",
                        syntax.name.name
                    ),
                    source,
                )
            })?;
            let value = StaticValue::zero(&ty).ok_or_else(|| {
                Diagnostic::unsupported(
                    format!("zero value for package variable type {ty:?} is not implemented"),
                    source,
                )
            })?;
            (ty, value)
        }
        VariableValueSyntax::ArityMismatch => {
            return Err(Diagnostic::unsupported(
                "multi-valued package variable initializers are not yet represented",
                source,
            ));
        }
    };
    let ty = explicit_ty.unwrap_or_else(|| raw_ty.default_typed());
    ensure_bootstrap_value_type(&ty, source)?;
    if !is_assignable(&raw_ty, &ty) || !value.is_representable_as(&ty) {
        return Err(Diagnostic::semantic(
            format!(
                "initializer for package variable {} is not assignable to {ty:?}",
                syntax.name.name
            ),
            source,
        ));
    }
    static_values::apply_package_initializers(
        syntax.name.name.as_ref(),
        &ty,
        &mut value,
        package_initializers,
        constants,
        type_aliases,
        static_functions,
        source,
    )?;
    if !value.is_representable_as(&ty) {
        return Err(Diagnostic::backend(format!(
            "initialized package variable {} no longer matches {ty:?}",
            syntax.name.name
        )));
    }
    Ok(TypedVariable {
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
    symbols: FunctionSymbols,
    type_aliases: BTreeMap<String, Ty>,
) -> Result<LoweredFunction, FunctionLoweringFailure> {
    let node = NodeId::owner_local(definition, 0);
    let mut initial_source_plan = vec![
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
    if let Err(error) = goto_scopes::validate(body) {
        let source = SourceRef::node(NodeId::owner_local(definition, 1));
        initial_source_plan.push((source, error.source));
        return Err(FunctionLoweringFailure {
            diagnostic: Diagnostic::semantic(error.message, source),
            source_plan: initial_source_plan,
        });
    }
    let mut lowerer = FunctionLowerer {
        owner: definition,
        next_node: 1,
        next_local_type: 0,
        functions: symbols.functions,
        qualified_functions: symbols.qualified_functions,
        methods: symbols.methods,
        generic_functions: symbols.generic_functions,
        generic_methods: symbols.generic_methods,
        generic_types: symbols.generic_types,
        constants: symbols.constants,
        qualified_constants: symbols.qualified_constants,
        local_constant_scopes: vec![BTreeMap::new()],
        variables: symbols.variables,
        qualified_variables: symbols.qualified_variables,
        intrinsic_packages: symbols.intrinsic_packages,
        type_aliases,
        type_scope_changes: vec![BTreeMap::new()],
        signature: signature.clone(),
        locals: Vec::new(),
        scopes: vec![BTreeMap::new()],
        closures: Vec::new(),
        closure_scopes: vec![BTreeMap::new()],
        named_results: Vec::new(),
        loop_labels: Vec::new(),
        range_yield_loop_depth: None,
        iteration_capture_scopes: Vec::new(),
        declared_labels: BTreeSet::new(),
        referenced_gotos: BTreeMap::new(),
        defer_registration_depth: 0,
        inside_deferred_closure: false,
        inside_local_closure: false,
        active_generic_functions: Vec::new(),
        source_override: None,
        source_plan: initial_source_plan,
    };
    let lowered = (|| {
        let receiver_count = usize::from(header.receiver.is_some());
        let (receiver_types, parameter_types) =
            signature
                .params
                .split_at_checked(receiver_count)
                .ok_or_else(|| Diagnostic::backend("method signature omitted its receiver type"))?;
        let mut params = Vec::new();
        if let Some(receiver) = &header.receiver {
            params.extend(lowerer.declare_field_bindings(
                receiver,
                receiver_types,
                hir::LocalKind::Parameter,
            )?);
        }
        params.extend(lowerer.declare_field_bindings(
            &header.params,
            parameter_types,
            hir::LocalKind::Parameter,
        )?);
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

pub(super) fn eval_constant(
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

pub(super) fn eval_constant_with_lookup(
    expression: &ExprSyntax,
    lookup: &impl Fn(&str) -> Option<(Ty, ConstValue)>,
    source: SourceRef,
    iota: u64,
) -> Result<(Ty, ConstValue), Diagnostic> {
    eval_constant_with_lookups(expression, lookup, &|_| None, source, iota)
}

pub(super) fn eval_constant_with_lookups(
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
            let left = left.normalized_for(&operand_ty);
            let right = right.normalized_for(&operand_ty);
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
            Ok((result_ty, value))
        }
        ExprSyntaxKind::Paren(expression) => {
            eval_constant_with_lookups(expression, constant_lookup, type_lookup, source, iota)
        }
        ExprSyntaxKind::Unary { token, expression } => {
            let (ty, value) =
                eval_constant_with_lookups(expression, constant_lookup, type_lookup, source, iota)?;
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

fn validate_constant_binary_operator(
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
    if let Some(target) = type_lookup(name) {
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
        value = fold_constant_binary(
            op,
            &value.normalized_for(&common),
            &right.normalized_for(&common),
            source,
        )?
        .ok_or_else(|| Diagnostic::semantic(format!("invalid constant {name}"), source))?;
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
    let component = |value: ConstValue| match value {
        ConstValue::Int(value) | ConstValue::Float(value) => Ok(value),
        _ => Err(Diagnostic::semantic(
            "complex requires real numeric constant arguments",
            source,
        )),
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

fn negate_number_spelling(value: &str) -> String {
    value
        .strip_prefix('-')
        .map_or_else(|| format!("-{value}"), str::to_string)
}
