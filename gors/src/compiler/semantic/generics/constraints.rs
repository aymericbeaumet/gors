//! Constraint validation for semantic generic instantiations.

use std::collections::{BTreeMap, BTreeSet};

use super::lower_type_with_generics;
use crate::compiler::Diagnostic;
use crate::compiler::provenance::SourceRef;
use crate::compiler::semantic::GenericTypeSymbol;
use crate::compiler::syntax::{ExprSyntax, ExprSyntaxKind, FieldListSyntax};
use crate::compiler::types::Ty;
use crate::token::Token;

pub(super) fn type_parameter_names(
    fields: &FieldListSyntax,
    source: SourceRef,
) -> Result<BTreeSet<String>, Diagnostic> {
    let mut result = BTreeSet::new();
    for field in &*fields.fields {
        let names = field.names.as_ref().ok_or_else(|| {
            Diagnostic::semantic("type parameter declaration requires a name", source)
        })?;
        for name in &**names {
            if !result.insert(name.name.to_string()) {
                return Err(Diagnostic::semantic(
                    format!("duplicate type parameter {}", name.name),
                    source,
                ));
            }
        }
    }
    Ok(result)
}

pub(super) fn validate_declared_constraints(
    fields: Option<&FieldListSyntax>,
    substitutions: &BTreeMap<String, Ty>,
    aliases: &BTreeMap<String, Ty>,
    generic_types: &BTreeMap<String, GenericTypeSymbol>,
    source: SourceRef,
) -> Result<(), Diagnostic> {
    let fields = fields
        .ok_or_else(|| Diagnostic::backend("generic declaration omitted its type parameters"))?;
    validate_constraints(fields, substitutions, aliases, generic_types, source)
}

pub(super) fn validate_constraints(
    fields: &FieldListSyntax,
    substitutions: &BTreeMap<String, Ty>,
    aliases: &BTreeMap<String, Ty>,
    generic_types: &BTreeMap<String, GenericTypeSymbol>,
    source: SourceRef,
) -> Result<(), Diagnostic> {
    let mut environment = aliases.clone();
    environment.extend(substitutions.clone());
    for field in &*fields.fields {
        let constraint = field
            .ty
            .as_ref()
            .ok_or_else(|| Diagnostic::backend("type parameter omitted its constraint"))?;
        let names = field.names.as_ref().ok_or_else(|| {
            Diagnostic::semantic("type parameter declaration requires a name", source)
        })?;
        for name in &**names {
            let actual = substitutions.get(name.name.as_ref()).ok_or_else(|| {
                Diagnostic::semantic(format!("cannot infer type parameter {}", name.name), source)
            })?;
            if !constraint_allows(constraint, actual, &environment, generic_types, source)? {
                return Err(Diagnostic::semantic(
                    format!(
                        "type {actual:?} does not satisfy the constraint for {}",
                        name.name
                    ),
                    source,
                ));
            }
        }
    }
    Ok(())
}

fn constraint_allows(
    constraint: &ExprSyntax,
    actual: &Ty,
    environment: &BTreeMap<String, Ty>,
    generic_types: &BTreeMap<String, GenericTypeSymbol>,
    source: SourceRef,
) -> Result<bool, Diagnostic> {
    match &constraint.kind {
        ExprSyntaxKind::Paren(constraint) => {
            constraint_allows(constraint, actual, environment, generic_types, source)
        }
        ExprSyntaxKind::Ident(ident) if ident.name.as_ref() == "any" => Ok(true),
        ExprSyntaxKind::Ident(ident) if ident.name.as_ref() == "comparable" => {
            Ok(is_comparable(actual))
        }
        ExprSyntaxKind::Ident(ident) if environment.contains_key(ident.name.as_ref()) => {
            Ok(environment.get(ident.name.as_ref()) == Some(actual))
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
            let names = type_parameter_names(&generic.type_parameters, source)?
                .into_iter()
                .collect::<Vec<_>>();
            let [name] = names.as_slice() else {
                return Err(Diagnostic::unsupported(
                    "constraint instantiation with multiple arguments is not yet implemented",
                    source,
                ));
            };
            let argument = lower_type_with_generics(index, environment, generic_types, source)?;
            let mut nested = environment.clone();
            nested.insert(name.clone(), argument);
            constraint_allows(&generic.underlying, actual, &nested, generic_types, source)
        }
        ExprSyntaxKind::IndexList { base, indices } => {
            let ExprSyntaxKind::Ident(base) = &base.kind else {
                return Ok(false);
            };
            let Some(generic) = generic_types.get(base.name.as_ref()) else {
                return Ok(false);
            };
            let names = type_parameter_names(&generic.type_parameters, source)?
                .into_iter()
                .collect::<Vec<_>>();
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
                    lower_type_with_generics(argument, environment, generic_types, source)?,
                );
            }
            constraint_allows(&generic.underlying, actual, &nested, generic_types, source)
        }
        ExprSyntaxKind::InterfaceType { methods } => {
            for field in &*methods.fields {
                if field.names.is_some() {
                    return Ok(false);
                }
                let term = field
                    .ty
                    .as_ref()
                    .ok_or_else(|| Diagnostic::backend("constraint element omitted its type"))?;
                if !constraint_allows(term, actual, environment, generic_types, source)? {
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
            constraint_allows(left, actual, environment, generic_types, source)?
                || constraint_allows(right, actual, environment, generic_types, source)?,
        ),
        ExprSyntaxKind::Unary {
            token: Token::TILDE,
            expression,
        } => type_pattern_matches(
            expression,
            actual.underlying(),
            environment,
            generic_types,
            source,
        ),
        _ => type_pattern_matches(constraint, actual, environment, generic_types, source),
    }
}

fn type_pattern_matches(
    pattern: &ExprSyntax,
    actual: &Ty,
    environment: &BTreeMap<String, Ty>,
    generic_types: &BTreeMap<String, GenericTypeSymbol>,
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
        return type_pattern_matches(element, actual, environment, generic_types, source);
    }
    if let ExprSyntaxKind::Ident(ident) = &pattern.kind
        && let Some(expected) = environment.get(ident.name.as_ref())
    {
        return Ok(expected == actual);
    }
    lower_type_with_generics(pattern, environment, generic_types, source)
        .map(|expected| expected == *actual)
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
