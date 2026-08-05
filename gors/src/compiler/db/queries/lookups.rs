//! Stable package-level function and constant lookups.

use std::sync::Arc;

use super::{ConstantProjection, Db, FunctionProjection, PackageInput, file_projection};
use crate::compiler::db::telemetry::QueryKind;
use crate::compiler::ids::DefId;

#[salsa::tracked(returns(copy))]
pub(in crate::compiler::db) fn package_function_product<'db>(
    db: &'db dyn Db,
    input: PackageInput,
    definition: DefId,
) -> Option<FunctionProjection<'db>> {
    db.query_telemetry()
        .record_query(QueryKind::PackageFunctionLookup);
    let mut sources = input.sources(db).iter().copied().collect::<Vec<_>>();
    sources.sort_by_key(|source| source.file(db));
    for source in sources {
        let facts = file_projection(db, source);
        if let Some(function) = facts
            .functions(db)
            .into_iter()
            .find(|function| function.id(db) == definition)
        {
            return Some(function);
        }
    }
    None
}

#[salsa::tracked(returns(copy))]
pub(in crate::compiler::db) fn package_function_named_product<'db>(
    db: &'db dyn Db,
    input: PackageInput,
    name: Arc<str>,
) -> Option<FunctionProjection<'db>> {
    db.query_telemetry()
        .record_query(QueryKind::PackageFunctionLookup);
    let mut sources = input.sources(db).iter().copied().collect::<Vec<_>>();
    sources.sort_by_key(|source| source.file(db));
    for source in sources {
        if let Some(function) = file_projection(db, source)
            .functions(db)
            .into_iter()
            .find(|function| function.name(db) == name)
        {
            return Some(function);
        }
    }
    None
}

#[salsa::tracked(returns(copy))]
pub(in crate::compiler::db) fn package_constant_named_product<'db>(
    db: &'db dyn Db,
    input: PackageInput,
    name: Arc<str>,
) -> Option<ConstantProjection<'db>> {
    db.query_telemetry()
        .record_query(QueryKind::PackageConstantLookup);
    let mut sources = input.sources(db).iter().copied().collect::<Vec<_>>();
    sources.sort_by_key(|source| source.file(db));
    for source in sources {
        if let Some(constant) = file_projection(db, source)
            .constants(db)
            .into_iter()
            .find(|constant| constant.name(db) == name)
        {
            return Some(constant);
        }
    }
    None
}
