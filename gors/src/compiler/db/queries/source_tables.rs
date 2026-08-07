//! Revision-local source-table and layout products.

use std::sync::Arc;

use super::support::function_file;
use super::{ConstantProjection, Db, FunctionProjection, PackageInput, semantic_function_product};
use crate::compiler::Diagnostic;
use crate::compiler::db::products::{CompilerStage, StageFailure, StageResult};
use crate::compiler::db::telemetry::QueryKind;
use crate::compiler::ids::FileId;
use crate::compiler::provenance::{DefinitionSourceTable, FileRange, SourceRef};
use crate::compiler::syntax::FunctionLayout;

#[salsa::tracked(returns(clone))]
pub(in crate::compiler::db) fn definition_source_table_product(
    db: &dyn Db,
    input: PackageInput,
    function: FunctionProjection<'_>,
) -> StageResult<DefinitionSourceTable> {
    db.query_telemetry()
        .record_query(QueryKind::DefinitionSourceTable);
    let definition = function.id(db);
    let layout = function.layout(db);
    let file = function_file(db, input, function)?;
    let semantic = semantic_function_product(db, input, function);
    let source_plan = semantic.source_plan();
    let mappings = source_plan
        .iter()
        .copied()
        .map(|(source, syntax_source)| {
            layout
                .resolve(syntax_source)
                .map(|range| (source, FileRange::new(file, range)))
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            Arc::new(StageFailure::one_for_definition(
                CompilerStage::Semantic,
                definition,
                Diagnostic::backend(format!(
                    "owned syntax source plan does not match its physical layout: {error}"
                )),
            ))
        })?;
    DefinitionSourceTable::try_new(definition, file, layout.source_len(), mappings)
        .map(Arc::new)
        .map_err(|error| {
            Arc::new(StageFailure::one_for_definition(
                CompilerStage::Semantic,
                definition,
                Diagnostic::backend(format!(
                    "failed to construct definition source table: {error}"
                )),
            ))
        })
}

#[salsa::tracked(returns(clone))]
pub(in crate::compiler::db) fn constant_source_table_product(
    db: &dyn Db,
    file: FileId,
    constant: ConstantProjection<'_>,
) -> StageResult<DefinitionSourceTable> {
    db.query_telemetry()
        .record_query(QueryKind::DefinitionSourceTable);
    let definition = constant.id(db);
    let layout = constant.layout(db);
    DefinitionSourceTable::try_new(
        definition,
        file,
        layout.source_len(),
        [(
            SourceRef::definition(definition),
            FileRange::new(file, layout.declaration()),
        )],
    )
    .map(Arc::new)
    .map_err(|error| {
        Arc::new(StageFailure::one_for_definition(
            CompilerStage::Semantic,
            definition,
            Diagnostic::backend(format!(
                "failed to construct constant source table: {error}"
            )),
        ))
    })
}

#[salsa::tracked(returns(clone))]
pub(in crate::compiler::db) fn function_layout_product(
    db: &dyn Db,
    function: FunctionProjection<'_>,
) -> Arc<FunctionLayout> {
    db.query_telemetry().record_query(QueryKind::FunctionLayout);
    function.layout(db)
}
