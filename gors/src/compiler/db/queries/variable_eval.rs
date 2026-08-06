//! Typed package-variable initializer evaluation.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::support::{check_semantic_barrier, semantic_build_dependency, semantic_failure};
use super::{
    Db, PackageInput, VariableProjection, file_projection, package_constant_named_product,
    package_type_aliases_product, package_variable_named_product, typed_constant_product,
};
use crate::compiler::Diagnostic;
use crate::compiler::db::products::{CompilerStage, StageFailure, StageResult};
use crate::compiler::db::telemetry::QueryKind;
use crate::compiler::ids::{FileId, QualifiedDefId};
use crate::compiler::provenance::{DefinitionSourceTable, FileRange, SourceRef};
use crate::compiler::semantic::{ConstantSymbol, TypedVariable};

#[salsa::tracked(returns(clone))]
pub(in crate::compiler::db) fn typed_variable_product(
    db: &dyn Db,
    input: PackageInput,
    variable: VariableProjection<'_>,
) -> StageResult<TypedVariable> {
    db.query_telemetry().record_query(QueryKind::TypedVariable);
    let definition = variable.id(db);
    check_semantic_barrier(db, definition, variable.semantic_barrier(db))?;
    semantic_build_dependency(db, definition)?;

    let mut constants = BTreeMap::new();
    for dependency_name in variable.dependencies(db).iter() {
        db.unwind_if_revision_cancelled();
        if let Some(constant) =
            package_constant_named_product(db, input, Arc::clone(dependency_name))
        {
            let typed = typed_constant_product(db, input, constant)?;
            constants.insert(
                typed.name.clone(),
                ConstantSymbol {
                    id: QualifiedDefId::new(input.package(db), typed.id),
                    ty: typed.ty.clone(),
                    value: typed.value.clone(),
                },
            );
        } else if package_variable_named_product(db, input, Arc::clone(dependency_name)).is_some() {
            return Err(semantic_failure(
                definition,
                Diagnostic::unsupported(
                    "package variable initializers that depend on another variable are not yet represented",
                    SourceRef::definition(definition),
                ),
            ));
        }
    }

    let type_aliases = package_type_aliases_product(db, input)?;
    let mut static_functions = BTreeMap::new();
    let mut package_initializers = Vec::new();
    let mut sources = input.sources(db).iter().copied().collect::<Vec<_>>();
    sources.sort_by_key(|source| source.file(db));
    for source in sources {
        let mut functions = file_projection(db, source).functions(db);
        functions.sort_by_key(|function| function.id(db));
        for function in functions {
            if function.receiver_type(db).is_some() {
                continue;
            }
            let body = Arc::new(function.body(db).structure().clone());
            if function.name(db).as_ref() == "init" {
                package_initializers.push(body);
            } else {
                static_functions.insert(function.name(db).to_string(), body);
            }
        }
    }
    super::super::super::semantic::lower_variable(
        definition,
        &variable.syntax(db),
        &constants,
        &type_aliases,
        &static_functions,
        &package_initializers,
    )
    .map(Arc::new)
    .map_err(|diagnostic| semantic_failure(definition, diagnostic))
}

#[salsa::tracked(returns(clone))]
pub(in crate::compiler::db) fn variable_source_table_product(
    db: &dyn Db,
    file: FileId,
    variable: VariableProjection<'_>,
) -> StageResult<DefinitionSourceTable> {
    db.query_telemetry()
        .record_query(QueryKind::DefinitionSourceTable);
    let definition = variable.id(db);
    let layout = variable.layout(db);
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
                "failed to construct variable source table: {error}"
            )),
        ))
    })
}
