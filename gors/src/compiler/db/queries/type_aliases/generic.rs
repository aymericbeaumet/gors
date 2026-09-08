//! Generic type symbols shared by package type resolution and function imports.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::{Db, PackageInput, file_projection};
use crate::compiler::ids::QualifiedDefId;
use crate::compiler::semantic::GenericTypeSymbol;
use crate::compiler::syntax::{ExprSyntax, ExprSyntaxKind, FieldListSyntax};

/// Generic type symbols for one package.
///
/// This is a tracked query so that reading any declaration's syntax stays
/// behind a firewall: an edit to an unrelated ordinary type leaves the returned
/// map equal, letting salsa backdate instead of invalidating every dependent
/// per-name type lookup.
#[salsa::tracked(returns(clone))]
pub(in crate::compiler::db) fn collect_generic_type_symbols(
    db: &dyn Db,
    input: PackageInput,
) -> Arc<BTreeMap<String, GenericTypeSymbol>> {
    let mut result = BTreeMap::new();
    let mut sources = input.sources(db).iter().copied().collect::<Vec<_>>();
    sources.sort_by_key(|source| source.file(db));
    for source in sources {
        let projection = file_projection(db, source);
        for alias in projection.type_aliases(db) {
            let syntax = alias.syntax(db);
            let Some(type_parameters) = &syntax.type_parameters else {
                continue;
            };
            result.insert(
                alias.name(db).to_string(),
                GenericTypeSymbol {
                    id: QualifiedDefId::new(input.package(db), alias.id(db)),
                    type_parameters: Arc::new(type_parameters.clone()),
                    underlying: syntax.target.clone(),
                    alias: true,
                },
            );
        }
        for definition in projection.type_definitions(db) {
            // Exactly the declarations that are not ordinary named types belong
            // here: parameterized ones and constraint interfaces, which resolve
            // through this map because package type resolution excludes them. An
            // ordinary definition must stay out, so that an unrelated type edit
            // leaves this map equal and `Spare[...]` is not read as an
            // instantiation.
            if definition.ordinary(db) {
                continue;
            }
            let syntax = definition.syntax(db);
            let type_parameters =
                syntax
                    .type_parameters
                    .clone()
                    .unwrap_or_else(|| FieldListSyntax {
                        fields: Arc::from([]),
                    });
            result.insert(
                definition.name(db).to_string(),
                GenericTypeSymbol {
                    id: QualifiedDefId::new(input.package(db), definition.id(db)),
                    type_parameters: Arc::new(type_parameters),
                    underlying: syntax.underlying.clone(),
                    alias: false,
                },
            );
        }
    }
    Arc::new(result)
}

/// Collect ordinary package types required to lower one generic instantiation.
/// Type-parameter identifiers are removed while generic underlying syntax is
/// traversed transitively.
pub(in crate::compiler::db::queries) fn collect_instantiation_dependencies(
    expression: &ExprSyntax,
    generic_types: &BTreeMap<String, GenericTypeSymbol>,
    dependencies: &mut BTreeMap<String, bool>,
) {
    collect_dependencies(
        expression,
        generic_types,
        dependencies,
        false,
        &mut Vec::new(),
    );
}

pub(in crate::compiler::db::queries) fn expression_contains_instantiation(
    expression: &ExprSyntax,
) -> bool {
    match &expression.kind {
        ExprSyntaxKind::Index { .. } | ExprSyntaxKind::IndexList { .. } => true,
        ExprSyntaxKind::Paren(inner)
        | ExprSyntaxKind::Unary {
            expression: inner, ..
        } => expression_contains_instantiation(inner),
        ExprSyntaxKind::ArrayType { element, .. } | ExprSyntaxKind::ChannelType { element, .. } => {
            expression_contains_instantiation(element)
        }
        ExprSyntaxKind::MapType { key, value } => {
            expression_contains_instantiation(key) || expression_contains_instantiation(value)
        }
        ExprSyntaxKind::FunctionType {
            params, results, ..
        } => {
            field_list_contains_instantiation(params)
                || results
                    .as_ref()
                    .is_some_and(field_list_contains_instantiation)
        }
        ExprSyntaxKind::StructType { fields }
        | ExprSyntaxKind::InterfaceType { methods: fields } => {
            field_list_contains_instantiation(fields)
        }
        ExprSyntaxKind::Ident(_)
        | ExprSyntaxKind::Selector { .. }
        | ExprSyntaxKind::TypeAssert { .. }
        | ExprSyntaxKind::Literal { .. }
        | ExprSyntaxKind::Binary { .. }
        | ExprSyntaxKind::Call { .. }
        | ExprSyntaxKind::FunctionLiteral { .. }
        | ExprSyntaxKind::KeyValue { .. }
        | ExprSyntaxKind::CompositeLiteral { .. }
        | ExprSyntaxKind::Slice { .. }
        | ExprSyntaxKind::Unsupported(_) => false,
    }
}

fn field_list_contains_instantiation(fields: &FieldListSyntax) -> bool {
    fields.fields.iter().any(|field| {
        field
            .ty
            .as_ref()
            .is_some_and(expression_contains_instantiation)
    })
}

fn collect_dependencies(
    expression: &ExprSyntax,
    generic_types: &BTreeMap<String, GenericTypeSymbol>,
    dependencies: &mut BTreeMap<String, bool>,
    guarded: bool,
    active_generics: &mut Vec<String>,
) {
    match &expression.kind {
        ExprSyntaxKind::Index { base, index } => {
            collect_generic_base(base, generic_types, dependencies, guarded, active_generics);
            collect_dependencies(index, generic_types, dependencies, guarded, active_generics);
        }
        ExprSyntaxKind::IndexList { base, indices } => {
            collect_generic_base(base, generic_types, dependencies, guarded, active_generics);
            for index in indices.iter() {
                collect_dependencies(index, generic_types, dependencies, guarded, active_generics);
            }
        }
        ExprSyntaxKind::Ident(ident) => {
            dependencies
                .entry(ident.name.to_string())
                .and_modify(|existing| *existing &= guarded)
                .or_insert(guarded);
        }
        ExprSyntaxKind::Paren(inner) => {
            collect_dependencies(inner, generic_types, dependencies, guarded, active_generics);
        }
        ExprSyntaxKind::Unary { token, expression } => collect_dependencies(
            expression,
            generic_types,
            dependencies,
            guarded || *token == crate::token::Token::MUL,
            active_generics,
        ),
        // Array lengths never contribute hard type dependencies. Go resolves a
        // length-position name in the later incomplete-array pass, which is what
        // gives a self-referential `[len((*T)(nil))]int` its zero length instead
        // of a declaration cycle.
        ExprSyntaxKind::ArrayType { length, element } => {
            collect_dependencies(
                element,
                generic_types,
                dependencies,
                guarded || length.is_none(),
                active_generics,
            );
        }
        ExprSyntaxKind::ChannelType { element, .. } => {
            collect_dependencies(element, generic_types, dependencies, true, active_generics);
        }
        ExprSyntaxKind::MapType { key, value } => {
            collect_dependencies(key, generic_types, dependencies, true, active_generics);
            collect_dependencies(value, generic_types, dependencies, true, active_generics);
        }
        ExprSyntaxKind::FunctionType {
            params, results, ..
        } => {
            collect_field_dependencies(params, generic_types, dependencies, active_generics);
            if let Some(results) = results {
                collect_field_dependencies(results, generic_types, dependencies, active_generics);
            }
        }
        ExprSyntaxKind::StructType { fields }
        | ExprSyntaxKind::InterfaceType { methods: fields } => {
            for field in fields.fields.iter() {
                if let Some(ty) = &field.ty {
                    collect_dependencies(ty, generic_types, dependencies, guarded, active_generics);
                }
            }
        }
        ExprSyntaxKind::Selector { .. }
        | ExprSyntaxKind::TypeAssert { .. }
        | ExprSyntaxKind::Literal { .. }
        | ExprSyntaxKind::Binary { .. }
        | ExprSyntaxKind::Call { .. }
        | ExprSyntaxKind::FunctionLiteral { .. }
        | ExprSyntaxKind::KeyValue { .. }
        | ExprSyntaxKind::CompositeLiteral { .. }
        | ExprSyntaxKind::Slice { .. }
        | ExprSyntaxKind::Unsupported(_) => {}
    }
}

fn collect_generic_base(
    base: &ExprSyntax,
    generic_types: &BTreeMap<String, GenericTypeSymbol>,
    dependencies: &mut BTreeMap<String, bool>,
    guarded: bool,
    active_generics: &mut Vec<String>,
) {
    let ExprSyntaxKind::Ident(ident) = &base.kind else {
        collect_dependencies(base, generic_types, dependencies, guarded, active_generics);
        return;
    };
    let Some(generic) = generic_types.get(ident.name.as_ref()) else {
        collect_dependencies(base, generic_types, dependencies, guarded, active_generics);
        return;
    };
    let name = ident.name.to_string();
    if active_generics.contains(&name) {
        return;
    }
    active_generics.push(name);
    let mut scoped = BTreeMap::new();
    collect_dependencies(
        &generic.underlying,
        generic_types,
        &mut scoped,
        guarded,
        active_generics,
    );
    for field in generic.type_parameters.fields.iter() {
        if let Some(constraint) = &field.ty {
            collect_dependencies(
                constraint,
                generic_types,
                &mut scoped,
                guarded,
                active_generics,
            );
        }
    }
    active_generics.pop();
    for parameter in generic_type_parameter_names(generic) {
        scoped.remove(&parameter);
    }
    for (dependency, scoped_guarded) in scoped {
        dependencies
            .entry(dependency)
            .and_modify(|existing| *existing &= scoped_guarded)
            .or_insert(scoped_guarded);
    }
}

fn collect_field_dependencies(
    fields: &FieldListSyntax,
    generic_types: &BTreeMap<String, GenericTypeSymbol>,
    dependencies: &mut BTreeMap<String, bool>,
    active_generics: &mut Vec<String>,
) {
    for field in fields.fields.iter() {
        if let Some(ty) = &field.ty {
            collect_dependencies(ty, generic_types, dependencies, true, active_generics);
        }
    }
}

fn generic_type_parameter_names(generic: &GenericTypeSymbol) -> Vec<String> {
    generic
        .type_parameters
        .fields
        .iter()
        .flat_map(|field| field.names.iter().flat_map(|names| names.iter()))
        .map(|name| name.name.to_string())
        .collect()
}
