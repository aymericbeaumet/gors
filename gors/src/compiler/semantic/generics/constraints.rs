//! Constraint validation for semantic generic instantiations.

use std::collections::{BTreeMap, BTreeSet};

use super::{
    MethodEnvironment, infer_type_expression, instantiate_generic_method_for_receiver,
    lower_type_with_generics, type_inference_mismatch,
};
use crate::compiler::Diagnostic;
use crate::compiler::provenance::SourceRef;
use crate::compiler::semantic::GenericTypeSymbol;
use crate::compiler::semantic::member_resolution::resolve_method_set_member_with;
use crate::compiler::syntax::{ExprSyntax, ExprSyntaxKind, FieldListSyntax};
use crate::compiler::types::{Signature, Ty};
use crate::token::Token;

pub(super) fn type_parameter_names_in_order(
    fields: &FieldListSyntax,
    source: SourceRef,
) -> Result<Vec<String>, Diagnostic> {
    let mut result = BTreeSet::new();
    let mut ordered = Vec::new();
    for field in &*fields.fields {
        let names = field.names.as_ref().ok_or_else(|| {
            Diagnostic::semantic("type parameter declaration requires a name", source)
        })?;
        for name in &**names {
            if name.name.as_ref() != "_" && !result.insert(name.name.to_string()) {
                return Err(Diagnostic::semantic(
                    format!("duplicate type parameter {}", name.name),
                    source,
                ));
            }
            ordered.push(name.name.to_string());
        }
    }
    Ok(ordered)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn infer_constraint_arguments(
    fields: &FieldListSyntax,
    parameter_names: &BTreeSet<String>,
    substitutions: &mut BTreeMap<String, Ty>,
    aliases: &BTreeMap<String, Ty>,
    generic_types: &BTreeMap<String, GenericTypeSymbol>,
    methods: MethodEnvironment<'_>,
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
                        methods,
                        source,
                    )?;
                } else if let Some(inferred) = exact_constraint_type(
                    constraint,
                    parameter_names,
                    substitutions,
                    aliases,
                    generic_types,
                    methods,
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
    methods: MethodEnvironment<'_>,
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
            methods,
            source,
        ),
        ExprSyntaxKind::InterfaceType {
            methods: constraint_elements,
        } => {
            for field in &*constraint_elements.fields {
                let Some(pattern) = &field.ty else {
                    return Err(Diagnostic::backend("constraint element omitted its type"));
                };
                if let Some(names) = &field.names {
                    for name in &**names {
                        let signature = method_set_signature(
                            actual,
                            name.name.as_ref(),
                            aliases,
                            generic_types,
                            methods,
                            source,
                        )?;
                        infer_type_expression(
                            pattern,
                            &Ty::Function(signature),
                            parameter_names,
                            substitutions,
                            aliases,
                            generic_types,
                            methods,
                            source,
                        )?;
                    }
                } else {
                    infer_from_constraint_pattern(
                        pattern,
                        actual,
                        parameter_names,
                        substitutions,
                        aliases,
                        generic_types,
                        methods,
                        source,
                    )?;
                }
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
                    methods,
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
            methods,
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
            methods,
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
            methods,
            source,
        ),
        ExprSyntaxKind::Ident(ident) if matches!(ident.name.as_ref(), "any" | "comparable") => {
            Ok(())
        }
        ExprSyntaxKind::Ident(ident)
            if !parameter_names.contains(ident.name.as_ref())
                && aliases
                    .get(ident.name.as_ref())
                    .is_some_and(|ty| matches!(ty.underlying(), Ty::Interface(_))) =>
        {
            let Some(Ty::Interface(required)) =
                aliases.get(ident.name.as_ref()).map(Ty::underlying)
            else {
                return Err(Diagnostic::backend(
                    "named interface constraint changed during inference",
                ));
            };
            require_interface_method_set(actual, required, aliases, generic_types, methods, source)
        }
        _ => infer_type_expression(
            constraint,
            actual,
            parameter_names,
            substitutions,
            aliases,
            generic_types,
            methods,
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
    methods: MethodEnvironment<'_>,
    source: SourceRef,
) -> Result<Option<Ty>, Diagnostic> {
    match &constraint.kind {
        ExprSyntaxKind::Paren(inner) => exact_constraint_type(
            inner,
            parameter_names,
            substitutions,
            aliases,
            generic_types,
            methods,
            source,
        ),
        ExprSyntaxKind::InterfaceType {
            methods: constraint_elements,
        } => {
            if constraint_elements
                .fields
                .iter()
                .any(|field| field.names.is_some())
            {
                return Ok(None);
            }
            let terms = constraint_elements
                .fields
                .iter()
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
                methods,
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
        ExprSyntaxKind::Ident(ident)
            if generic_types
                .get(ident.name.as_ref())
                .is_some_and(|generic| {
                    matches!(
                        generic.underlying.kind,
                        ExprSyntaxKind::InterfaceType { .. }
                    )
                }) =>
        {
            let generic = generic_types
                .get(ident.name.as_ref())
                .ok_or_else(|| Diagnostic::backend("named constraint type disappeared"))?;
            exact_constraint_type(
                &generic.underlying,
                parameter_names,
                substitutions,
                aliases,
                generic_types,
                methods,
                source,
            )
        }
        _ => {
            let mut environment = aliases.clone();
            environment.extend(substitutions.clone());
            let inferred =
                lower_type_with_generics(constraint, &environment, generic_types, methods, source)?;
            if matches!(inferred.underlying(), Ty::Interface(_)) {
                Ok(None)
            } else {
                Ok(Some(inferred))
            }
        }
    }
}

fn method_set_signature(
    actual: &Ty,
    name: &str,
    aliases: &BTreeMap<String, Ty>,
    generic_types: &BTreeMap<String, GenericTypeSymbol>,
    methods: MethodEnvironment<'_>,
    source: SourceRef,
) -> Result<Signature, Diagnostic> {
    if let Ty::Interface(interface_methods) = actual.underlying() {
        return interface_methods
            .iter()
            .find(|method| method.name == name)
            .map(|method| method.signature.clone())
            .ok_or_else(|| {
                Diagnostic::semantic(
                    format!("type {actual:?} has no method {name} required by its constraint"),
                    source,
                )
            });
    }
    if !methods.complete {
        return Err(Diagnostic::unsupported(
            "method-constrained generic instantiation in a package type declaration requires package method facts",
            source,
        ));
    }
    let resolved = resolve_method_set_member_with(
        actual,
        name,
        methods.concrete,
        source,
        &|selected_ty, selected_name| {
            instantiate_generic_method_for_receiver(
                selected_ty,
                selected_name,
                aliases,
                generic_types,
                methods,
                source,
            )
        },
    )?;
    let Some((_, params)) = resolved.symbol.signature.params.split_first() else {
        return Err(Diagnostic::backend(
            "resolved method signature omitted its receiver",
        ));
    };
    Ok(Signature {
        params: params.to_vec(),
        results: resolved.symbol.signature.results.clone(),
        variadic: resolved.symbol.signature.variadic,
    })
}

fn require_interface_method_set(
    actual: &Ty,
    required: &[crate::compiler::types::InterfaceMethod],
    aliases: &BTreeMap<String, Ty>,
    generic_types: &BTreeMap<String, GenericTypeSymbol>,
    methods: MethodEnvironment<'_>,
    source: SourceRef,
) -> Result<(), Diagnostic> {
    for method in required {
        let actual_signature = method_set_signature(
            actual,
            &method.name,
            aliases,
            generic_types,
            methods,
            source,
        )?;
        if actual_signature != method.signature {
            return Err(Diagnostic::semantic(
                format!(
                    "method {} has signature {actual_signature:?}; constraint requires {:?}",
                    method.name, method.signature
                ),
                source,
            ));
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn infer_from_instantiated_constraint(
    base: &ExprSyntax,
    arguments: &[ExprSyntax],
    actual: &Ty,
    parameter_names: &BTreeSet<String>,
    substitutions: &mut BTreeMap<String, Ty>,
    aliases: &BTreeMap<String, Ty>,
    generic_types: &BTreeMap<String, GenericTypeSymbol>,
    methods: MethodEnvironment<'_>,
    source: SourceRef,
) -> Result<(), Diagnostic> {
    let ExprSyntaxKind::Ident(base) = &base.kind else {
        return Err(type_inference_mismatch(base, actual, source));
    };
    let generic = generic_types.get(base.name.as_ref()).ok_or_else(|| {
        Diagnostic::semantic(format!("{} is not a generic type", base.name), source)
    })?;
    let inner_names = type_parameter_names_in_order(&generic.type_parameters, source)?;
    if inner_names.len() != arguments.len() {
        return Err(Diagnostic::semantic(
            format!(
                "constraint {} requires {} type arguments; got {}",
                base.name,
                inner_names.len(),
                arguments.len()
            ),
            source,
        ));
    }

    // First match the concrete argument against the named constraint's own
    // type parameters. Then project those inferred types through the
    // instantiation arguments into the caller's parameter environment. This
    // keeps the two parameter namespaces distinct, including when the named
    // constraint deliberately renames its element parameter.
    let inner_parameter_names = inner_names.iter().cloned().collect::<BTreeSet<_>>();
    let mut inner_substitutions = BTreeMap::new();
    infer_from_constraint_pattern(
        &generic.underlying,
        actual,
        &inner_parameter_names,
        &mut inner_substitutions,
        aliases,
        generic_types,
        methods,
        source,
    )?;
    for (inner_name, argument) in inner_names.iter().zip(arguments) {
        let Some(inferred) = inner_substitutions.get(inner_name) else {
            continue;
        };
        infer_type_expression(
            argument,
            inferred,
            parameter_names,
            substitutions,
            aliases,
            generic_types,
            methods,
            source,
        )?;
    }
    Ok(())
}

pub(super) fn validate_declared_constraints(
    fields: Option<&FieldListSyntax>,
    substitutions: &BTreeMap<String, Ty>,
    positional: Option<&[Ty]>,
    aliases: &BTreeMap<String, Ty>,
    generic_types: &BTreeMap<String, GenericTypeSymbol>,
    methods: MethodEnvironment<'_>,
    source: SourceRef,
) -> Result<(), Diagnostic> {
    let fields = fields
        .ok_or_else(|| Diagnostic::backend("generic declaration omitted its type parameters"))?;
    validate_constraints(
        fields,
        substitutions,
        positional,
        aliases,
        generic_types,
        methods,
        source,
    )
}

pub(super) fn validate_constraints(
    fields: &FieldListSyntax,
    substitutions: &BTreeMap<String, Ty>,
    positional: Option<&[Ty]>,
    aliases: &BTreeMap<String, Ty>,
    generic_types: &BTreeMap<String, GenericTypeSymbol>,
    methods: MethodEnvironment<'_>,
    source: SourceRef,
) -> Result<(), Diagnostic> {
    let mut environment = aliases.clone();
    environment.extend(substitutions.clone());
    let mut position = 0_usize;
    for field in &*fields.fields {
        let constraint = field
            .ty
            .as_ref()
            .ok_or_else(|| Diagnostic::backend("type parameter omitted its constraint"))?;
        let names = field.names.as_ref().ok_or_else(|| {
            Diagnostic::semantic("type parameter declaration requires a name", source)
        })?;
        for name in &**names {
            let actual = if let Some(positional) = positional {
                positional.get(position)
            } else if name.name.as_ref() == "_" {
                position = position.saturating_add(1);
                continue;
            } else {
                substitutions.get(name.name.as_ref())
            }
            .ok_or_else(|| {
                Diagnostic::semantic(format!("cannot infer type parameter {}", name.name), source)
            })?;
            if !constraint_allows(
                constraint,
                actual,
                &environment,
                generic_types,
                methods,
                source,
            )? {
                return Err(Diagnostic::semantic(
                    format!(
                        "type {actual:?} does not satisfy the constraint for {}",
                        name.name
                    ),
                    source,
                ));
            }
            position = position.saturating_add(1);
        }
    }
    Ok(())
}

fn constraint_allows(
    constraint: &ExprSyntax,
    actual: &Ty,
    environment: &BTreeMap<String, Ty>,
    generic_types: &BTreeMap<String, GenericTypeSymbol>,
    methods: MethodEnvironment<'_>,
    source: SourceRef,
) -> Result<bool, Diagnostic> {
    match &constraint.kind {
        ExprSyntaxKind::Paren(constraint) => constraint_allows(
            constraint,
            actual,
            environment,
            generic_types,
            methods,
            source,
        ),
        ExprSyntaxKind::Ident(ident) if ident.name.as_ref() == "any" => Ok(true),
        ExprSyntaxKind::Ident(ident) if ident.name.as_ref() == "comparable" => {
            Ok(is_comparable(actual))
        }
        ExprSyntaxKind::Ident(ident) if environment.contains_key(ident.name.as_ref()) => {
            let expected = environment
                .get(ident.name.as_ref())
                .ok_or_else(|| Diagnostic::backend("constraint environment changed"))?;
            if let Ty::Interface(required) = expected.underlying() {
                require_interface_method_set(
                    actual,
                    required,
                    environment,
                    generic_types,
                    methods,
                    source,
                )
                .map(|()| true)
            } else {
                Ok(expected.is_identical_to(actual))
            }
        }
        ExprSyntaxKind::Ident(ident) if generic_types.contains_key(ident.name.as_ref()) => {
            let generic = generic_types
                .get(ident.name.as_ref())
                .ok_or_else(|| Diagnostic::backend("constraint definition disappeared"))?;
            if !generic.type_parameters.fields.is_empty() {
                return Err(Diagnostic::semantic(
                    format!("constraint {} requires type arguments", ident.name),
                    source,
                ));
            }
            constraint_allows(
                &generic.underlying,
                actual,
                environment,
                generic_types,
                methods,
                source,
            )
        }
        ExprSyntaxKind::Index { base, index } => {
            let ExprSyntaxKind::Ident(base) = &base.kind else {
                return Ok(false);
            };
            let Some(generic) = generic_types.get(base.name.as_ref()) else {
                return Ok(false);
            };
            let names = type_parameter_names_in_order(&generic.type_parameters, source)?;
            let [name] = names.as_slice() else {
                return Err(Diagnostic::unsupported(
                    "constraint instantiation with multiple arguments is not yet implemented",
                    source,
                ));
            };
            let argument =
                lower_type_with_generics(index, environment, generic_types, methods, source)?;
            let mut nested = environment.clone();
            nested.insert(name.clone(), argument);
            constraint_allows(
                &generic.underlying,
                actual,
                &nested,
                generic_types,
                methods,
                source,
            )
        }
        ExprSyntaxKind::IndexList { base, indices } => {
            let ExprSyntaxKind::Ident(base) = &base.kind else {
                return Ok(false);
            };
            let Some(generic) = generic_types.get(base.name.as_ref()) else {
                return Ok(false);
            };
            let names = type_parameter_names_in_order(&generic.type_parameters, source)?;
            if names.len() != indices.len() {
                return Err(Diagnostic::semantic(
                    format!(
                        "constraint {} requires {} type arguments; got {}",
                        base.name,
                        names.len(),
                        indices.len()
                    ),
                    source,
                ));
            }
            let mut nested = environment.clone();
            for (name, argument) in names.into_iter().zip(&**indices) {
                nested.insert(
                    name,
                    lower_type_with_generics(
                        argument,
                        environment,
                        generic_types,
                        methods,
                        source,
                    )?,
                );
            }
            constraint_allows(
                &generic.underlying,
                actual,
                &nested,
                generic_types,
                methods,
                source,
            )
        }
        ExprSyntaxKind::InterfaceType {
            methods: constraint_elements,
        } => {
            for field in &*constraint_elements.fields {
                let pattern = field
                    .ty
                    .as_ref()
                    .ok_or_else(|| Diagnostic::backend("constraint element omitted its type"))?;
                if let Some(names) = &field.names {
                    let expected = lower_type_with_generics(
                        pattern,
                        environment,
                        generic_types,
                        methods,
                        source,
                    )?;
                    let Ty::Function(expected) = expected else {
                        return Err(Diagnostic::semantic(
                            "constraint methods require function signatures",
                            source,
                        ));
                    };
                    for name in &**names {
                        if method_set_signature(
                            actual,
                            name.name.as_ref(),
                            environment,
                            generic_types,
                            methods,
                            source,
                        )? != expected
                        {
                            return Ok(false);
                        }
                    }
                } else if !constraint_allows(
                    pattern,
                    actual,
                    environment,
                    generic_types,
                    methods,
                    source,
                )? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        ExprSyntaxKind::Binary {
            left,
            token: Token::OR,
            right,
        } => Ok(
            constraint_allows(left, actual, environment, generic_types, methods, source)?
                || constraint_allows(right, actual, environment, generic_types, methods, source)?,
        ),
        ExprSyntaxKind::Unary {
            token: Token::TILDE,
            expression,
        } => type_pattern_matches(
            expression,
            actual.underlying(),
            environment,
            generic_types,
            methods,
            source,
        ),
        _ => type_pattern_matches(
            constraint,
            actual,
            environment,
            generic_types,
            methods,
            source,
        ),
    }
}

fn type_pattern_matches(
    pattern: &ExprSyntax,
    actual: &Ty,
    environment: &BTreeMap<String, Ty>,
    generic_types: &BTreeMap<String, GenericTypeSymbol>,
    methods: MethodEnvironment<'_>,
    source: SourceRef,
) -> Result<bool, Diagnostic> {
    if let ExprSyntaxKind::ArrayType {
        length: None,
        element,
    } = &pattern.kind
    {
        let Ty::Slice(actual) = actual.underlying() else {
            return Ok(false);
        };
        return type_pattern_matches(element, actual, environment, generic_types, methods, source);
    }
    if let ExprSyntaxKind::Ident(ident) = &pattern.kind
        && let Some(expected) = environment.get(ident.name.as_ref())
    {
        return Ok(expected.is_identical_to(actual));
    }
    lower_type_with_generics(pattern, environment, generic_types, methods, source)
        .map(|expected| expected.is_identical_to(actual))
}

fn is_comparable(ty: &Ty) -> bool {
    match ty.underlying() {
        Ty::Unit
        | Ty::NamedRef { .. }
        | Ty::Slice(_)
        | Ty::Map(_, _)
        | Ty::Function(_)
        | Ty::Tuple(_) => false,
        Ty::Array(_, element) => is_comparable(element),
        Ty::Struct(fields) => fields.iter().all(|field| is_comparable(&field.ty)),
        Ty::Bool
        | Ty::Int(_)
        | Ty::Uint(_)
        | Ty::Float(_)
        | Ty::Complex(_)
        | Ty::String
        | Ty::Pointer(_)
        | Ty::Channel(_, _)
        | Ty::Interface(_)
        | Ty::Untyped(_) => true,
        Ty::Named { underlying, .. } | Ty::LocalNamed { underlying, .. } => {
            is_comparable(underlying)
        }
    }
}
