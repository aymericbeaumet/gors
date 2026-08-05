//! Package-wide type-alias resolution over owned declaration projections.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::{Db, PackageInput, file_projection, semantic_failure};
use crate::compiler::Diagnostic;
use crate::compiler::db::products::{CompilerStage, StageFailure, StageResult};
use crate::compiler::ids::{DefId, DefinitionKey};
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::{ExprSyntax, TypeAliasSyntax, TypeDefinitionSyntax};
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
            projections.insert(alias.name(db).to_string(), alias);
        }
        for definition in facts.type_definitions(db) {
            definitions.insert(definition.name(db).to_string(), definition);
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
        )?;
    }
    Ok(Arc::new(resolved))
}

fn resolve_type_name<'db>(
    db: &'db dyn Db,
    name: &str,
    projections: &BTreeMap<String, TypeAliasProjection<'db>>,
    definitions: &BTreeMap<String, TypeDefinitionProjection<'db>>,
    resolved: &mut BTreeMap<String, Ty>,
    stack: &mut Vec<String>,
) -> Result<Ty, Arc<StageFailure>> {
    if let Some(ty) = resolved.get(name) {
        return Ok(ty.clone());
    }
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
    if let Some(start) = stack.iter().position(|entry| entry == name) {
        let mut path = stack.iter().skip(start).cloned().collect::<Vec<_>>();
        path.push(name.to_owned());
        return Err(semantic_failure(
            definition,
            Diagnostic::semantic(
                format!("type declaration cycle: {}", path.join(" -> ")),
                SourceRef::definition(definition),
            ),
        ));
    }

    stack.push(name.to_owned());
    if let crate::compiler::syntax::ExprSyntaxKind::Ident(target) = &expression.kind
        && (projections.contains_key(target.name.as_ref())
            || definitions.contains_key(target.name.as_ref()))
    {
        let target_ty = resolve_type_name(
            db,
            target.name.as_ref(),
            projections,
            definitions,
            resolved,
            stack,
        )?;
        resolved.insert(target.name.to_string(), target_ty);
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

fn lower_declaration_target(
    expression: &ExprSyntax,
    resolved: &BTreeMap<String, Ty>,
    definition: DefId,
) -> Result<Ty, Arc<StageFailure>> {
    crate::compiler::semantic::lower_type(expression, resolved, SourceRef::definition(definition))
        .map_err(|diagnostic| semantic_failure(definition, diagnostic))
}
