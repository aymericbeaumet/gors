//! Package-wide type-alias resolution over owned declaration projections.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::{Db, PackageInput, file_projection, semantic_failure};
use crate::compiler::Diagnostic;
use crate::compiler::db::products::{CompilerStage, StageFailure, StageResult};
use crate::compiler::ids::{DefId, DefinitionKey};
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{
    ExprSyntax, ExprSyntaxKind, FieldListSyntax, TypeAliasSyntax, TypeDefinitionSyntax,
};
use crate::compiler::types::Ty;

#[salsa::tracked]
pub(in crate::compiler::db) struct TypeAliasProjection<'db> {
    #[returns(copy)]
    pub(super) id: DefId,
    #[tracked]
    #[returns(clone)]
    pub(super) key: DefinitionKey,
    #[tracked]
    #[returns(clone)]
    pub(super) name: Arc<str>,
    #[tracked]
    #[returns(clone)]
    pub(super) syntax: Arc<TypeAliasSyntax>,
}

#[salsa::tracked]
pub(in crate::compiler::db) struct TypeDefinitionProjection<'db> {
    #[returns(copy)]
    pub(super) id: DefId,
    #[tracked]
    #[returns(clone)]
    pub(super) key: DefinitionKey,
    #[tracked]
    #[returns(clone)]
    pub(super) name: Arc<str>,
    #[tracked]
    #[returns(clone)]
    pub(super) syntax: Arc<TypeDefinitionSyntax>,
}

#[salsa::tracked(returns(clone))]
pub(in crate::compiler::db) fn package_type_aliases_product(
    db: &dyn Db,
    input: PackageInput,
) -> StageResult<BTreeMap<String, Ty>> {
    let mut sources = input.sources(db).iter().copied().collect::<Vec<_>>();
    sources.sort_by_key(|source| source.file(db));
    let mut projections = BTreeMap::new();
    let mut definitions = BTreeMap::new();
    for source in sources {
        let facts = file_projection(db, source);
        for alias in facts.type_aliases(db) {
            if alias.syntax(db).type_parameters.is_none() {
                projections.insert(alias.name(db).to_string(), alias);
            }
        }
        for definition in facts.type_definitions(db) {
            let syntax = definition.syntax(db);
            if syntax.type_parameters.is_none() && !is_constraint_type(&syntax.underlying) {
                definitions.insert(definition.name(db).to_string(), definition);
            }
        }
    }

    let mut resolved = BTreeMap::new();
    let mut stack = Vec::new();
    for name in projections.keys().chain(definitions.keys()) {
        resolve_type_name(
            db,
            name,
            &projections,
            &definitions,
            &mut resolved,
            &mut stack,
            false,
        )?;
    }
    Ok(Arc::new(resolved))
}

fn is_constraint_type(expression: &ExprSyntax) -> bool {
    let ExprSyntaxKind::InterfaceType { methods } = &expression.kind else {
        return false;
    };
    methods.fields.iter().any(|field| {
        field.names.is_none()
            && field.ty.as_ref().is_some_and(|ty| {
                matches!(
                    ty.kind,
                    ExprSyntaxKind::Binary {
                        token: crate::token::Token::OR,
                        ..
                    } | ExprSyntaxKind::Unary {
                        token: crate::token::Token::TILDE,
                        ..
                    }
                ) || matches!(&ty.kind, ExprSyntaxKind::Ident(ident) if ident.name.as_ref() == "comparable")
            })
    })
}

fn resolve_type_name<'db>(
    db: &'db dyn Db,
    name: &str,
    projections: &BTreeMap<String, TypeAliasProjection<'db>>,
    definitions: &BTreeMap<String, TypeDefinitionProjection<'db>>,
    resolved: &mut BTreeMap<String, Ty>,
    stack: &mut Vec<(String, bool)>,
    incoming_guarded: bool,
) -> Result<Ty, Arc<StageFailure>> {
    let declaration = projections
        .get(name)
        .copied()
        .map(|alias| (alias.id(db), alias.syntax(db).target.clone(), false))
        .or_else(|| {
            definitions
                .get(name)
                .copied()
                .map(|defined| (defined.id(db), defined.syntax(db).underlying.clone(), true))
        })
        .ok_or_else(|| {
            Arc::new(StageFailure::one(
                CompilerStage::Semantic,
                Diagnostic::backend(format!("missing projected type declaration {name}")),
            ))
        })?;
    let (definition, expression, is_definition) = declaration;
    if let Some(start) = stack.iter().position(|(entry, _)| entry == name) {
        let mut path = stack
            .iter()
            .skip(start)
            .map(|(entry, _)| entry.clone())
            .collect::<Vec<_>>();
        path.push(name.to_owned());
        let definitions_only = is_definition
            && stack
                .iter()
                .skip(start)
                .all(|(entry, _)| definitions.contains_key(entry));
        let guarded = incoming_guarded
            || stack
                .iter()
                .skip(start.saturating_add(1))
                .any(|(_, guarded)| *guarded);
        if definitions_only && guarded {
            return Ok(Ty::NamedRef { definition });
        }
        return Err(semantic_failure(
            definition,
            Diagnostic::semantic(
                if definitions_only {
                    format!("invalid recursive named type: {}", path.join(" -> "))
                } else {
                    format!("type declaration cycle: {}", path.join(" -> "))
                },
                SourceRef::definition(definition),
            ),
        ));
    }
    if let Some(ty) = resolved.get(name) {
        return Ok(ty.clone());
    }

    stack.push((name.to_owned(), incoming_guarded));
    let mut dependencies = BTreeMap::new();
    collect_type_dependencies(&expression, &mut dependencies, false);
    for (dependency, guarded) in dependencies {
        if projections.contains_key(&dependency) || definitions.contains_key(&dependency) {
            let target_ty = resolve_type_name(
                db,
                &dependency,
                projections,
                definitions,
                resolved,
                stack,
                guarded,
            )?;
            resolved.insert(dependency, target_ty);
        }
    }
    let target = lower_declaration_target(&expression, resolved, definition)?;
    let ty = if is_definition {
        Ty::Named {
            definition,
            underlying: Box::new(target.underlying().clone()),
        }
    } else {
        target
    };
    stack.pop();
    resolved.insert(name.to_owned(), ty.clone());
    Ok(ty)
}

fn collect_type_dependencies(
    expression: &ExprSyntax,
    dependencies: &mut BTreeMap<String, bool>,
    guarded: bool,
) {
    match &expression.kind {
        ExprSyntaxKind::Ident(ident) => {
            dependencies
                .entry(ident.name.to_string())
                .and_modify(|existing| *existing &= guarded)
                .or_insert(guarded);
        }
        ExprSyntaxKind::Paren(expression) => {
            collect_type_dependencies(expression, dependencies, guarded);
        }
        ExprSyntaxKind::Unary { token, expression } => {
            collect_type_dependencies(
                expression,
                dependencies,
                guarded || *token == crate::token::Token::MUL,
            );
        }
        ExprSyntaxKind::ArrayType { length, element } => {
            collect_type_dependencies(element, dependencies, guarded || length.is_none());
        }
        ExprSyntaxKind::ChannelType { element, .. } => {
            collect_type_dependencies(element, dependencies, true);
        }
        ExprSyntaxKind::MapType { key, value } => {
            collect_type_dependencies(key, dependencies, true);
            collect_type_dependencies(value, dependencies, true);
        }
        ExprSyntaxKind::FunctionType {
            params, results, ..
        } => {
            collect_field_type_dependencies(params, dependencies, true);
            if let Some(results) = results {
                collect_field_type_dependencies(results, dependencies, true);
            }
        }
        ExprSyntaxKind::StructType { fields }
        | ExprSyntaxKind::InterfaceType { methods: fields } => {
            collect_field_type_dependencies(fields, dependencies, guarded);
        }
        ExprSyntaxKind::Selector { .. }
        | ExprSyntaxKind::TypeAssert { .. }
        | ExprSyntaxKind::Literal { .. }
        | ExprSyntaxKind::Binary { .. }
        | ExprSyntaxKind::Call { .. }
        | ExprSyntaxKind::FunctionLiteral { .. }
        | ExprSyntaxKind::KeyValue { .. }
        | ExprSyntaxKind::CompositeLiteral { .. }
        | ExprSyntaxKind::Index { .. }
        | ExprSyntaxKind::Slice { .. }
        | ExprSyntaxKind::Unsupported(_) => {}
    }
}

fn collect_field_type_dependencies(
    fields: &FieldListSyntax,
    dependencies: &mut BTreeMap<String, bool>,
    guarded: bool,
) {
    for field in &*fields.fields {
        if let Some(ty) = &field.ty {
            collect_type_dependencies(ty, dependencies, guarded);
        }
    }
}

fn lower_declaration_target(
    expression: &ExprSyntax,
    resolved: &BTreeMap<String, Ty>,
    definition: DefId,
) -> Result<Ty, Arc<StageFailure>> {
    crate::compiler::semantic::lower_type(expression, resolved, SourceRef::definition(definition))
        .map_err(|diagnostic| semantic_failure(definition, diagnostic))
}
