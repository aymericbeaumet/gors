//! Exact package-constant dependency evaluation.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::support::{check_semantic_barrier, semantic_build_dependency, semantic_failure};
use super::{
    ConstantProjection, Db, PackageInput, package_constant_named_product,
    package_type_aliases_product,
};
use crate::compiler::Diagnostic;
use crate::compiler::db::products::{CompilerStage, StageFailure, StageResult};
use crate::compiler::ids::DefId;
use crate::compiler::provenance::SourceRef;
use crate::compiler::semantic::{ConstantSymbol, TypedConstant};

pub(super) fn evaluate_constant(
    db: &dyn Db,
    input: PackageInput,
    root: ConstantProjection<'_>,
) -> StageResult<TypedConstant> {
    let root_definition = root.id(db);
    let mut stack = Vec::new();
    let mut cache = BTreeMap::new();
    evaluate(db, input, root, root_definition, &mut stack, &mut cache)
}

fn evaluate(
    db: &dyn Db,
    input: PackageInput,
    constant: ConstantProjection<'_>,
    root_definition: DefId,
    stack: &mut Vec<(DefId, Arc<str>)>,
    cache: &mut BTreeMap<DefId, Arc<TypedConstant>>,
) -> StageResult<TypedConstant> {
    let definition = constant.id(db);
    if let Some(cached) = cache.get(&definition) {
        return Ok(Arc::clone(cached));
    }
    if let Some(cycle_start) = stack.iter().position(|(id, _)| *id == definition) {
        let mut path = stack
            .iter()
            .skip(cycle_start)
            .map(|(_, name)| name.to_string())
            .collect::<Vec<_>>();
        path.push(constant.name(db).to_string());
        return Err(Arc::new(StageFailure::one_for_definition(
            CompilerStage::Semantic,
            root_definition,
            Diagnostic::semantic(
                format!("constant initialization cycle: {}", path.join(" -> ")),
                SourceRef::definition(root_definition),
            ),
        )));
    }

    check_semantic_barrier(db, definition, constant.semantic_barrier(db))?;
    semantic_build_dependency(db, definition)?;
    stack.push((definition, constant.name(db)));
    let mut constants = BTreeMap::new();
    for dependency_name in constant.dependencies(db).iter() {
        db.unwind_if_revision_cancelled();
        let Some(dependency) =
            package_constant_named_product(db, input, Arc::clone(dependency_name))
        else {
            continue;
        };
        let typed = evaluate(db, input, dependency, root_definition, stack, cache)?;
        constants.insert(
            typed.name.clone(),
            ConstantSymbol {
                id: crate::compiler::ids::QualifiedDefId::new(input.package(db), typed.id),
                ty: typed.ty.clone(),
                value: typed.value.clone(),
            },
        );
    }
    stack.pop();

    let syntax = constant.syntax(db);
    let type_aliases = if syntax.explicit_type.is_some() {
        package_type_aliases_product(db, input)?
    } else {
        Arc::new(BTreeMap::new())
    };
    let typed = super::super::super::semantic::lower_constant(
        definition,
        &syntax,
        &constants,
        &type_aliases,
    )
    .map(Arc::new)
    .map_err(|diagnostic| semantic_failure(definition, diagnostic))?;
    cache.insert(definition, Arc::clone(&typed));
    Ok(typed)
}
