//! Type-checked instantiation of generic declarations into caller-owned HIR.

mod calls;
mod constraints;

use std::collections::{BTreeMap, BTreeSet};

use super::expressions::coerce_expr;
use super::{FunctionLowerer, GenericFunctionSymbol, GenericTypeSymbol, lower_type};
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::{ClosureId, NodeId};
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{
    BlockSyntax, ExprSyntax, ExprSyntaxKind, FieldListSyntax, FunctionHeaderSyntax, IdentSyntax,
    SyntaxSource,
};
use crate::compiler::types::{ChannelDir, Signature, Ty, UntypedTy};
use crate::token::Token;

type LoweredGenericBody = (
    Vec<crate::compiler::ids::LocalId>,
    Vec<Option<crate::compiler::ids::LocalId>>,
    hir::Block,
);
use constraints::{
    infer_from_instantiated_constraint, type_parameter_names, type_parameter_names_in_order,
    validate_constraints, validate_declared_constraints,
};

pub(super) fn lower_type_with_generics(
    expression: &ExprSyntax,
    aliases: &BTreeMap<String, Ty>,
    generic_types: &BTreeMap<String, GenericTypeSymbol>,
    source: SourceRef,
) -> Result<Ty, Diagnostic> {
    let (base, arguments) = match &expression.kind {
        ExprSyntaxKind::Index { base, index } => {
            (base.as_ref(), std::slice::from_ref(index.as_ref()))
        }
        ExprSyntaxKind::IndexList { base, indices } => (base.as_ref(), indices.as_ref()),
        _ => return lower_type(expression, aliases, source),
    };
    let ExprSyntaxKind::Ident(base) = &base.kind else {
        return Err(Diagnostic::unsupported(
            "parameterized type base must be a named type",
            source,
        ));
    };
    let generic = generic_types.get(base.name.as_ref()).ok_or_else(|| {
        Diagnostic::semantic(format!("{} is not a generic type", base.name), source)
    })?;
    let parameters = type_parameter_names_in_order(&generic.type_parameters, source)?;
    if parameters.len() != arguments.len() {
        return Err(Diagnostic::semantic(
            format!(
                "generic type {} requires {} type arguments; got {}",
                base.name,
                parameters.len(),
                arguments.len()
            ),
            source,
        ));
    }
    let substitutions = parameters
        .into_iter()
        .zip(arguments)
        .map(|(parameter, argument)| {
            lower_type_with_generics(argument, aliases, generic_types, source)
                .map(|argument| (parameter, argument))
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    validate_constraints(
        &generic.type_parameters,
        &substitutions,
        aliases,
        generic_types,
        source,
    )?;
    let mut instantiated_aliases = aliases.clone();
    instantiated_aliases.extend(substitutions);
    let underlying = lower_type_with_generics(
        &generic.underlying,
        &instantiated_aliases,
        generic_types,
        source,
    )?;
    if generic.alias {
        Ok(underlying)
    } else {
        Ok(Ty::Named {
            definition: generic.id,
            underlying: Box::new(underlying.underlying().clone()),
        })
    }
}

fn infer_function_arguments(
    header: &FunctionHeaderSyntax,
    type_arguments: &[ExprSyntax],
    arguments: &[Ty],
    spread: bool,
    aliases: &BTreeMap<String, Ty>,
    generic_types: &BTreeMap<String, GenericTypeSymbol>,
    source: SourceRef,
) -> Result<BTreeMap<String, Ty>, Diagnostic> {
    let parameters = header
        .type_parameters
        .as_ref()
        .ok_or_else(|| Diagnostic::backend("generic function omitted its type-parameter syntax"))?;
    let ordered_names = type_parameter_names_in_order(parameters, source)?;
    if type_arguments.len() > ordered_names.len() {
        return Err(Diagnostic::semantic(
            format!(
                "generic function requires at most {} type arguments; got {}",
                ordered_names.len(),
                type_arguments.len()
            ),
            source,
        ));
    }
    let names = ordered_names.iter().cloned().collect::<BTreeSet<_>>();
    let mut substitutions = ordered_names
        .iter()
        .zip(type_arguments)
        .map(|(name, argument)| {
            lower_type_with_generics(argument, aliases, generic_types, source)
                .map(|argument| (name.clone(), argument))
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    infer_parameter_list(
        &header.params,
        arguments,
        spread,
        &names,
        &mut substitutions,
        aliases,
        generic_types,
        source,
    )?;
    infer_constraint_arguments(
        parameters,
        &names,
        &mut substitutions,
        aliases,
        generic_types,
        source,
    )?;
    let missing = names
        .iter()
        .filter(|name| !substitutions.contains_key(*name))
        .cloned()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(Diagnostic::semantic(
            format!("cannot infer type parameters {}", missing.join(", ")),
            source,
        ));
    }
    Ok(substitutions)
}

#[allow(clippy::too_many_arguments)]
fn infer_parameter_list(
    fields: &FieldListSyntax,
    arguments: &[Ty],
    spread: bool,
    parameter_names: &BTreeSet<String>,
    substitutions: &mut BTreeMap<String, Ty>,
    aliases: &BTreeMap<String, Ty>,
    generic_types: &BTreeMap<String, GenericTypeSymbol>,
    source: SourceRef,
) -> Result<(), Diagnostic> {
    let formal = parameter_patterns_for_call(fields, arguments.len(), spread, source)?;
    if formal.len() != arguments.len() {
        return Err(Diagnostic::semantic(
            format!(
                "generic call has {} arguments; expected {}",
                arguments.len(),
                formal.len()
            ),
            source,
        ));
    }
    // Typed arguments determine type parameters first. Untyped constant
    // arguments adapt to a parameter a typed argument already determined
    // (argument coercion checks representability); only parameters no typed
    // argument determined receive the merged default type of their untyped
    // constant arguments.
    for (formal, actual) in formal.iter().zip(arguments) {
        if matches!(actual, Ty::Untyped(_)) {
            continue;
        }
        infer_type_expression(
            formal,
            &actual.default_typed(),
            parameter_names,
            substitutions,
            aliases,
            generic_types,
            source,
        )?;
    }
    let mut constant_defaults = BTreeMap::<String, UntypedTy>::new();
    for (formal, actual) in formal.iter().zip(arguments) {
        let Ty::Untyped(kind) = actual else {
            continue;
        };
        let Some(parameter) = bare_type_parameter(formal, parameter_names) else {
            infer_type_expression(
                formal,
                &actual.default_typed(),
                parameter_names,
                substitutions,
                aliases,
                generic_types,
                source,
            )?;
            continue;
        };
        if substitutions.contains_key(parameter) {
            continue;
        }
        let merged = match constant_defaults.get(parameter) {
            None => *kind,
            Some(previous) => merge_untyped_constant_kinds(*previous, *kind).ok_or_else(|| {
                Diagnostic::semantic(
                    format!(
                        "mismatched default types {:?} and {:?} for {parameter}",
                        Ty::Untyped(*previous).default_typed(),
                        Ty::Untyped(*kind).default_typed()
                    ),
                    source,
                )
            })?,
        };
        constant_defaults.insert(parameter.to_string(), merged);
    }
    for (parameter, kind) in constant_defaults {
        substitutions.insert(parameter, Ty::Untyped(kind).default_typed());
    }
    Ok(())
}

/// The type-parameter name a formal parameter references directly, if any.
fn bare_type_parameter<'syntax>(
    formal: &'syntax ExprSyntax,
    parameter_names: &BTreeSet<String>,
) -> Option<&'syntax str> {
    match &formal.kind {
        ExprSyntaxKind::Paren(inner) => bare_type_parameter(inner, parameter_names),
        ExprSyntaxKind::Ident(ident) if parameter_names.contains(ident.name.as_ref()) => {
            Some(ident.name.as_ref())
        }
        _ => None,
    }
}

/// Merged default-type kind of two untyped constants inferred for one type
/// parameter. Numeric kinds merge to the kind appearing later in the
/// specification order integer, floating-point, complex; other kinds only
/// merge with themselves.
fn merge_untyped_constant_kinds(previous: UntypedTy, next: UntypedTy) -> Option<UntypedTy> {
    if previous == next {
        return Some(previous);
    }
    let rank = |kind: UntypedTy| match kind {
        UntypedTy::Int => Some(0_u8),
        UntypedTy::Rune => Some(1),
        UntypedTy::Float => Some(2),
        UntypedTy::Complex => Some(3),
        UntypedTy::Bool | UntypedTy::String => None,
    };
    let previous_rank = rank(previous)?;
    let next_rank = rank(next)?;
    Some(if previous_rank >= next_rank {
        previous
    } else {
        next
    })
}

#[allow(clippy::too_many_arguments)]
fn infer_type_expression(
    formal: &ExprSyntax,
    actual: &Ty,
    parameter_names: &BTreeSet<String>,
    substitutions: &mut BTreeMap<String, Ty>,
    aliases: &BTreeMap<String, Ty>,
    generic_types: &BTreeMap<String, GenericTypeSymbol>,
    source: SourceRef,
) -> Result<(), Diagnostic> {
    match &formal.kind {
        ExprSyntaxKind::Paren(formal) => infer_type_expression(
            formal,
            actual,
            parameter_names,
            substitutions,
            aliases,
            generic_types,
            source,
        ),
        ExprSyntaxKind::Ident(ident) if parameter_names.contains(ident.name.as_ref()) => {
            let actual = actual.default_typed();
            if let Some(previous) = substitutions.get(ident.name.as_ref())
                && previous != &actual
            {
                let Some(preferred) = prefer_defined_type(previous, &actual) else {
                    return Err(Diagnostic::semantic(
                        format!(
                            "conflicting inferred types for {}: {previous:?} and {actual:?}",
                            ident.name
                        ),
                        source,
                    ));
                };
                substitutions.insert(ident.name.to_string(), preferred);
                return Ok(());
            }
            substitutions.insert(ident.name.to_string(), actual);
            Ok(())
        }
        ExprSyntaxKind::ArrayType {
            length: None,
            element,
        } => {
            let Ty::Slice(actual) = actual.underlying() else {
                return Err(type_inference_mismatch(formal, actual, source));
            };
            infer_type_expression(
                element,
                actual,
                parameter_names,
                substitutions,
                aliases,
                generic_types,
                source,
            )
        }
        ExprSyntaxKind::Unary {
            token: Token::MUL,
            expression,
        } => {
            let Ty::Pointer(actual) = actual.underlying() else {
                return Err(type_inference_mismatch(formal, actual, source));
            };
            infer_type_expression(
                expression,
                actual,
                parameter_names,
                substitutions,
                aliases,
                generic_types,
                source,
            )
        }
        ExprSyntaxKind::ChannelType { direction, element } => {
            let Ty::Channel(actual_direction, actual_element) = actual.underlying() else {
                return Err(type_inference_mismatch(formal, actual, source));
            };
            let formal_direction = match direction {
                crate::compiler::syntax::ChannelDirectionSyntax::SendReceive => {
                    ChannelDir::SendReceive
                }
                crate::compiler::syntax::ChannelDirectionSyntax::SendOnly => ChannelDir::SendOnly,
                crate::compiler::syntax::ChannelDirectionSyntax::ReceiveOnly => {
                    ChannelDir::ReceiveOnly
                }
            };
            if formal_direction != *actual_direction && *actual_direction != ChannelDir::SendReceive
            {
                return Err(type_inference_mismatch(formal, actual, source));
            }
            infer_type_expression(
                element,
                actual_element,
                parameter_names,
                substitutions,
                aliases,
                generic_types,
                source,
            )
        }
        ExprSyntaxKind::Index { base, .. } | ExprSyntaxKind::IndexList { base, .. } => {
            let ExprSyntaxKind::Ident(base) = &base.kind else {
                return Err(type_inference_mismatch(formal, actual, source));
            };
            let generic = generic_types.get(base.name.as_ref()).ok_or_else(|| {
                Diagnostic::semantic(format!("{} is not a generic type", base.name), source)
            })?;
            let Ty::Named {
                definition,
                underlying,
            } = actual
            else {
                return Err(type_inference_mismatch(formal, actual, source));
            };
            if *definition != generic.id {
                return Err(type_inference_mismatch(formal, actual, source));
            }
            infer_type_expression(
                &generic.underlying,
                underlying,
                parameter_names,
                substitutions,
                aliases,
                generic_types,
                source,
            )
        }
        ExprSyntaxKind::StructType { fields } => {
            let Ty::Struct(actual_fields) = actual.underlying() else {
                return Err(type_inference_mismatch(formal, actual, source));
            };
            if fields.fields.len() != actual_fields.len() {
                return Err(type_inference_mismatch(formal, actual, source));
            }
            for (formal, actual) in fields.fields.iter().zip(actual_fields) {
                let formal = formal
                    .ty
                    .as_ref()
                    .ok_or_else(|| Diagnostic::backend("generic struct field omitted its type"))?;
                infer_type_expression(
                    formal,
                    &actual.ty,
                    parameter_names,
                    substitutions,
                    aliases,
                    generic_types,
                    source,
                )?;
            }
            Ok(())
        }
        _ => {
            let mut instantiated_aliases = aliases.clone();
            instantiated_aliases.extend(substitutions.clone());
            let expected =
                lower_type_with_generics(formal, &instantiated_aliases, generic_types, source)?;
            if expected == *actual {
                Ok(())
            } else {
                Err(type_inference_mismatch(formal, actual, source))
            }
        }
    }
}

fn prefer_defined_type(previous: &Ty, actual: &Ty) -> Option<Ty> {
    if previous.underlying() != actual.underlying() {
        return None;
    }
    match (previous, actual) {
        (Ty::Named { .. } | Ty::LocalNamed { .. }, _) => Some(previous.clone()),
        (_, Ty::Named { .. } | Ty::LocalNamed { .. }) => Some(actual.clone()),
        _ => None,
    }
}

fn type_inference_mismatch(_formal: &ExprSyntax, actual: &Ty, source: SourceRef) -> Diagnostic {
    Diagnostic::semantic(
        format!("argument type {actual:?} does not match the generic parameter pattern"),
        source,
    )
}

#[allow(clippy::too_many_arguments)]
fn infer_constraint_arguments(
    fields: &FieldListSyntax,
    parameter_names: &BTreeSet<String>,
    substitutions: &mut BTreeMap<String, Ty>,
    aliases: &BTreeMap<String, Ty>,
    generic_types: &BTreeMap<String, GenericTypeSymbol>,
    source: SourceRef,
) -> Result<(), Diagnostic> {
    loop {
        if parameter_names
            .iter()
            .all(|name| substitutions.contains_key(name))
        {
            return Ok(());
        }
        let before = substitutions.len();
        for field in &*fields.fields {
            let constraint = field
                .ty
                .as_ref()
                .ok_or_else(|| Diagnostic::backend("type parameter omitted its constraint"))?;
            let names = field.names.as_ref().ok_or_else(|| {
                Diagnostic::semantic("type parameter declaration requires a name", source)
            })?;
            for name in &**names {
                if let Some(actual) = substitutions.get(name.name.as_ref()).cloned() {
                    infer_from_constraint_pattern(
                        constraint,
                        &actual,
                        parameter_names,
                        substitutions,
                        aliases,
                        generic_types,
                        source,
                    )?;
                } else if let Some(inferred) = exact_constraint_type(
                    constraint,
                    parameter_names,
                    substitutions,
                    aliases,
                    generic_types,
                    source,
                )? {
                    substitutions.insert(name.name.to_string(), inferred);
                }
            }
        }
        if substitutions.len() == before {
            return Ok(());
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn infer_from_constraint_pattern(
    constraint: &ExprSyntax,
    actual: &Ty,
    parameter_names: &BTreeSet<String>,
    substitutions: &mut BTreeMap<String, Ty>,
    aliases: &BTreeMap<String, Ty>,
    generic_types: &BTreeMap<String, GenericTypeSymbol>,
    source: SourceRef,
) -> Result<(), Diagnostic> {
    match &constraint.kind {
        ExprSyntaxKind::Paren(inner) => infer_from_constraint_pattern(
            inner,
            actual,
            parameter_names,
            substitutions,
            aliases,
            generic_types,
            source,
        ),
        ExprSyntaxKind::InterfaceType { methods } => {
            for field in &*methods.fields {
                if field.names.is_some() {
                    continue;
                }
                let Some(term) = &field.ty else {
                    continue;
                };
                infer_from_constraint_pattern(
                    term,
                    actual,
                    parameter_names,
                    substitutions,
                    aliases,
                    generic_types,
                    source,
                )?;
            }
            Ok(())
        }
        ExprSyntaxKind::Binary {
            left,
            token: Token::OR,
            right,
        } => {
            for alternative in [left.as_ref(), right.as_ref()] {
                let mut candidate = substitutions.clone();
                if infer_from_constraint_pattern(
                    alternative,
                    actual,
                    parameter_names,
                    &mut candidate,
                    aliases,
                    generic_types,
                    source,
                )
                .is_ok()
                {
                    *substitutions = candidate;
                    return Ok(());
                }
            }
            Ok(())
        }
        ExprSyntaxKind::Index { base, index } => infer_from_instantiated_constraint(
            base,
            std::slice::from_ref(index.as_ref()),
            actual,
            parameter_names,
            substitutions,
            aliases,
            generic_types,
            source,
        ),
        ExprSyntaxKind::IndexList { base, indices } => infer_from_instantiated_constraint(
            base,
            indices,
            actual,
            parameter_names,
            substitutions,
            aliases,
            generic_types,
            source,
        ),
        ExprSyntaxKind::Unary {
            token: Token::TILDE,
            expression,
        } => infer_type_expression(
            expression,
            actual.underlying(),
            parameter_names,
            substitutions,
            aliases,
            generic_types,
            source,
        ),
        ExprSyntaxKind::Ident(ident) if matches!(ident.name.as_ref(), "any" | "comparable") => {
            Ok(())
        }
        _ => infer_type_expression(
            constraint,
            actual,
            parameter_names,
            substitutions,
            aliases,
            generic_types,
            source,
        ),
    }
}

fn exact_constraint_type(
    constraint: &ExprSyntax,
    parameter_names: &BTreeSet<String>,
    substitutions: &BTreeMap<String, Ty>,
    aliases: &BTreeMap<String, Ty>,
    generic_types: &BTreeMap<String, GenericTypeSymbol>,
    source: SourceRef,
) -> Result<Option<Ty>, Diagnostic> {
    match &constraint.kind {
        ExprSyntaxKind::Paren(inner) => exact_constraint_type(
            inner,
            parameter_names,
            substitutions,
            aliases,
            generic_types,
            source,
        ),
        ExprSyntaxKind::InterfaceType { methods } => {
            let terms = methods
                .fields
                .iter()
                .filter(|field| field.names.is_none())
                .filter_map(|field| field.ty.as_ref())
                .collect::<Vec<_>>();
            let [term] = terms.as_slice() else {
                return Ok(None);
            };
            exact_constraint_type(
                term,
                parameter_names,
                substitutions,
                aliases,
                generic_types,
                source,
            )
        }
        ExprSyntaxKind::Unary {
            token: Token::TILDE,
            ..
        }
        | ExprSyntaxKind::Binary {
            token: Token::OR, ..
        } => Ok(None),
        ExprSyntaxKind::Ident(ident)
            if matches!(ident.name.as_ref(), "any" | "comparable")
                || parameter_names.contains(ident.name.as_ref()) =>
        {
            Ok(None)
        }
        _ => {
            let mut environment = aliases.clone();
            environment.extend(substitutions.clone());
            lower_type_with_generics(constraint, &environment, generic_types, source).map(Some)
        }
    }
}

fn infer_receiver_parameter_aliases(
    receiver: &ExprSyntax,
    base_parameters: &FieldListSyntax,
    substitutions: &BTreeMap<String, Ty>,
    source: SourceRef,
) -> Result<BTreeMap<String, Ty>, Diagnostic> {
    let receiver = match &receiver.kind {
        ExprSyntaxKind::Unary {
            token: Token::MUL,
            expression,
        } => expression.as_ref(),
        _ => receiver,
    };
    let arguments = match &receiver.kind {
        ExprSyntaxKind::Index { index, .. } => std::slice::from_ref(index.as_ref()),
        ExprSyntaxKind::IndexList { indices, .. } => indices.as_ref(),
        _ => {
            return Err(Diagnostic::semantic(
                "generic method receiver must instantiate its base type",
                source,
            ));
        }
    };
    let parameters = type_parameter_names_in_order(base_parameters, source)?;
    if parameters.len() != arguments.len() {
        return Err(Diagnostic::semantic(
            format!(
                "generic method receiver requires {} type arguments; got {}",
                parameters.len(),
                arguments.len()
            ),
            source,
        ));
    }
    let mut aliases = BTreeMap::new();
    for (parameter, argument) in parameters.into_iter().zip(arguments) {
        let ExprSyntaxKind::Ident(alias) = &argument.kind else {
            return Err(Diagnostic::semantic(
                "generic method receiver arguments must be identifiers",
                source,
            ));
        };
        if alias.name.as_ref() == "_" {
            continue;
        }
        let actual = substitutions.get(&parameter).cloned().ok_or_else(|| {
            Diagnostic::backend(format!(
                "generic receiver parameter {parameter} was not inferred"
            ))
        })?;
        if aliases.insert(alias.name.to_string(), actual).is_some() {
            return Err(Diagnostic::semantic(
                format!("receiver type parameter {} is repeated", alias.name),
                source,
            ));
        }
    }
    Ok(aliases)
}

fn instantiate_generic_receiver(
    generic: &GenericTypeSymbol,
    substitutions: &BTreeMap<String, Ty>,
    pointer: bool,
    aliases: &BTreeMap<String, Ty>,
    generic_types: &BTreeMap<String, GenericTypeSymbol>,
    source: SourceRef,
) -> Result<Ty, Diagnostic> {
    if generic.alias {
        return Err(Diagnostic::semantic(
            "a method receiver base must be a defined type",
            source,
        ));
    }
    let mut environment = aliases.clone();
    environment.extend(substitutions.clone());
    let underlying =
        lower_type_with_generics(&generic.underlying, &environment, generic_types, source)?;
    let receiver = Ty::Named {
        definition: generic.id,
        underlying: Box::new(underlying.underlying().clone()),
    };
    Ok(if pointer {
        Ty::Pointer(Box::new(receiver))
    } else {
        receiver
    })
}

fn instantiate_signature(
    header: &FunctionHeaderSyntax,
    substitutions: &BTreeMap<String, Ty>,
    receiver_override: Option<Ty>,
    aliases: &BTreeMap<String, Ty>,
    generic_types: &BTreeMap<String, GenericTypeSymbol>,
    source: SourceRef,
) -> Result<Signature, Diagnostic> {
    let mut aliases = aliases.clone();
    aliases.extend(substitutions.clone());
    let mut params = Vec::new();
    if let Some(receiver) = &header.receiver {
        if let Some(receiver) = receiver_override {
            params.push(receiver);
        } else {
            let (receiver, variadic) =
                parameter_types_with_generics(receiver, &aliases, generic_types, source)?;
            if variadic || receiver.len() != 1 {
                return Err(Diagnostic::semantic(
                    "a method must declare exactly one non-variadic receiver",
                    source,
                ));
            }
            params.extend(receiver);
        }
    } else if receiver_override.is_some() {
        return Err(Diagnostic::backend(
            "generic function received a method receiver override",
        ));
    }
    let (ordinary, variadic) =
        parameter_types_with_generics(&header.params, &aliases, generic_types, source)?;
    params.extend(ordinary);
    let results = header
        .results
        .as_ref()
        .map(|results| field_types_with_generics(results, &aliases, generic_types, source))
        .transpose()?
        .unwrap_or_default();
    Ok(Signature {
        params,
        results,
        variadic,
    })
}

fn field_types_with_generics(
    fields: &FieldListSyntax,
    aliases: &BTreeMap<String, Ty>,
    generic_types: &BTreeMap<String, GenericTypeSymbol>,
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
        let expression = field
            .ty
            .as_ref()
            .ok_or_else(|| Diagnostic::backend("signature field has no type"))?;
        let ty = lower_type_with_generics(expression, aliases, generic_types, source)?;
        result.extend(std::iter::repeat_n(
            ty,
            field.names.as_ref().map_or(1, |names| names.len()),
        ));
    }
    Ok(result)
}

fn parameter_types_with_generics(
    fields: &FieldListSyntax,
    aliases: &BTreeMap<String, Ty>,
    generic_types: &BTreeMap<String, GenericTypeSymbol>,
    source: SourceRef,
) -> Result<(Vec<Ty>, bool), Diagnostic> {
    let mut result = Vec::new();
    let mut variadic = false;
    for (index, field) in fields.fields.iter().enumerate() {
        let expression = field
            .ty
            .as_ref()
            .ok_or_else(|| Diagnostic::backend("signature field has no type"))?;
        let mut ty = lower_type_with_generics(expression, aliases, generic_types, source)?;
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

fn parameter_patterns_for_call(
    fields: &FieldListSyntax,
    argument_count: usize,
    spread: bool,
    source: SourceRef,
) -> Result<Vec<&ExprSyntax>, Diagnostic> {
    let mut result = Vec::new();
    let mut variadic = None;
    for (index, field) in fields.fields.iter().enumerate() {
        let ty = field
            .ty
            .as_ref()
            .ok_or_else(|| Diagnostic::backend("generic parameter omitted its type"))?;
        let count = field.names.as_ref().map_or(1, |names| names.len());
        if field.variadic {
            if variadic.is_some() || index + 1 != fields.fields.len() || count != 1 {
                return Err(Diagnostic::semantic(
                    "a variadic parameter must be the final single parameter",
                    source,
                ));
            }
            variadic = Some(ty);
        } else {
            result.extend(std::iter::repeat_n(ty, count));
        }
    }
    let fixed = result.len();
    if let Some(element) = variadic {
        if spread {
            return Err(Diagnostic::unsupported(
                "generic spread-call inference is not yet implemented",
                source,
            ));
        }
        if argument_count < fixed {
            return Err(Diagnostic::semantic(
                format!("generic call has {argument_count} arguments; requires at least {fixed}"),
                source,
            ));
        }
        result.extend(std::iter::repeat_n(element, argument_count - fixed));
    } else if argument_count != fixed {
        return Err(Diagnostic::semantic(
            format!("generic call has {argument_count} arguments; expected {fixed}"),
            source,
        ));
    }
    Ok(result)
}

fn single_receiver_type(
    header: &FunctionHeaderSyntax,
    source: SourceRef,
) -> Result<&ExprSyntax, Diagnostic> {
    let receiver = header
        .receiver
        .as_ref()
        .ok_or_else(|| Diagnostic::backend("generic method omitted its receiver"))?;
    let [field] = receiver.fields.as_ref() else {
        return Err(Diagnostic::semantic(
            "a method must declare exactly one receiver",
            source,
        ));
    };
    field
        .ty
        .as_ref()
        .ok_or_else(|| Diagnostic::backend("generic method receiver omitted its type"))
}

fn named_receiver_definition(ty: &Ty) -> Option<crate::compiler::ids::DefId> {
    match ty {
        Ty::Named { definition, .. } => Some(*definition),
        Ty::Pointer(element) => match element.as_ref() {
            Ty::Named { definition, .. } => Some(*definition),
            _ => None,
        },
        _ => None,
    }
}
