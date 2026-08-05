//! Package-wide type-alias resolution over owned declaration projections.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::{Db, PackageInput, file_projection, semantic_failure};
use crate::compiler::Diagnostic;
use crate::compiler::db::products::{CompilerStage, StageFailure, StageResult};
use crate::compiler::ids::{DefId, DefinitionKey};
use crate::compiler::provenance::SourceRef;
use crate::compiler::syntax::TypeAliasSyntax;
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

#[salsa::tracked(returns(clone))]
pub(in crate::compiler::db) fn package_type_aliases_product(
    db: &dyn Db,
    input: PackageInput,
) -> StageResult<BTreeMap<String, Ty>> {
    let mut sources = input.sources(db).iter().copied().collect::<Vec<_>>();
    sources.sort_by_key(|source| source.file(db));
    let mut projections = BTreeMap::new();
    for source in sources {
        for alias in file_projection(db, source).type_aliases(db) {
            projections.insert(alias.name(db).to_string(), alias);
        }
    }

    let mut resolved = BTreeMap::new();
    let mut stack = Vec::new();
    for name in projections.keys() {
        resolve_type_alias(db, name, &projections, &mut resolved, &mut stack)?;
    }
    Ok(Arc::new(resolved))
}

fn resolve_type_alias<'db>(
    db: &'db dyn Db,
    name: &str,
    projections: &BTreeMap<String, TypeAliasProjection<'db>>,
    resolved: &mut BTreeMap<String, Ty>,
    stack: &mut Vec<String>,
) -> Result<Ty, Arc<StageFailure>> {
    if let Some(ty) = resolved.get(name) {
        return Ok(ty.clone());
    }
    let alias = projections.get(name).copied().ok_or_else(|| {
        Arc::new(StageFailure::one(
            CompilerStage::Semantic,
            Diagnostic::backend(format!("missing projected type alias {name}")),
        ))
    })?;
    let definition = alias.id(db);
    if let Some(start) = stack.iter().position(|entry| entry == name) {
        let mut path = stack.iter().skip(start).cloned().collect::<Vec<_>>();
        path.push(name.to_owned());
        return Err(semantic_failure(
            definition,
            Diagnostic::semantic(
                format!("type alias cycle: {}", path.join(" -> ")),
                SourceRef::definition(definition),
            ),
        ));
    }

    stack.push(name.to_owned());
    let syntax = alias.syntax(db);
    if let crate::compiler::syntax::ExprSyntaxKind::Ident(target) = &syntax.target.kind
        && projections.contains_key(target.name.as_ref())
    {
        let target_ty = resolve_type_alias(db, target.name.as_ref(), projections, resolved, stack)?;
        resolved.insert(target.name.to_string(), target_ty);
    }
    let ty = crate::compiler::semantic::lower_type(
        &syntax.target,
        resolved,
        SourceRef::definition(definition),
    )
    .map_err(|diagnostic| semantic_failure(definition, diagnostic))?;
    stack.pop();
    resolved.insert(name.to_owned(), ty.clone());
    Ok(ty)
}
