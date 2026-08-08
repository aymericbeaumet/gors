//! Exact package-constant dependency evaluation.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::support::{
    check_semantic_barrier, collect_all_expression_names, collect_constant_references,
    semantic_build_dependency, semantic_failure,
};
use super::{
    ConstantProjection, Db, PackageInput, VariableProjection, file_projection,
    package_constant_named_product, package_type_named_product, package_variable_named_product,
};
use crate::compiler::Diagnostic;
use crate::compiler::db::products::{CompilerStage, StageFailure, StageResult};
use crate::compiler::ids::DefId;
use crate::compiler::provenance::SourceRef;
use crate::compiler::semantic::{ConstantSymbol, TypedConstant};
use crate::compiler::syntax::{ExprSyntax, ExprSyntaxKind, VariableSyntax, VariableValueSyntax};

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
    let syntax = constant.syntax(db);
    // Resolve only type names mentioned by this declaration. Loading the
    // package-wide type map here would make unrelated `type A [constant]T`
    // declarations cyclic with every package constant.
    let mut referenced_names = std::collections::BTreeSet::new();
    collect_constant_references(&syntax, &mut referenced_names);
    let mut type_aliases = BTreeMap::new();
    let mut type_resolved_constant = None;
    for name in &referenced_names {
        let resolution = package_type_named_product(db, input, Arc::from(name.as_str()))?;
        type_aliases.extend(
            resolution
                .types
                .iter()
                .map(|(name, ty)| (name.clone(), ty.clone())),
        );
        if let Some(symbol) = resolution.constants.get(&definition) {
            if type_resolved_constant
                .as_ref()
                .is_some_and(|existing| existing != symbol)
            {
                return Err(Arc::new(StageFailure::one_for_definition(
                    CompilerStage::Semantic,
                    definition,
                    Diagnostic::backend(
                        "inconsistent package type/constant strongly connected component",
                    ),
                )));
            }
            type_resolved_constant = Some(symbol.clone());
        }
    }
    if let Some(symbol) = type_resolved_constant {
        let typed = Arc::new(TypedConstant {
            id: definition,
            name: constant.name(db).to_string(),
            ty: symbol.ty,
            value: symbol.value,
        });
        stack.pop();
        cache.insert(definition, Arc::clone(&typed));
        return Ok(typed);
    }
    let mut variables = BTreeMap::new();
    for name in &referenced_names {
        let Some(variable) = package_variable_named_product(db, input, Arc::from(name.as_str()))
        else {
            continue;
        };
        let variable_syntax = variable.syntax(db);
        let Some(variable_type) = package_variable_type_syntax(&variable_syntax) else {
            continue;
        };

        let mut variable_constants = constants.clone();
        let mut variable_references = std::collections::BTreeSet::new();
        collect_all_expression_names(variable_type, &mut variable_references);
        for dependency_name in &variable_references {
            let Some(dependency) =
                package_constant_named_product(db, input, Arc::from(dependency_name.as_str()))
            else {
                continue;
            };
            let dependency_definition = dependency.id(db);
            if let Some(cycle_start) = stack
                .iter()
                .position(|(id, _)| *id == dependency_definition)
            {
                let mut path = stack
                    .iter()
                    .skip(cycle_start)
                    .map(|(_, name)| name.to_string())
                    .collect::<Vec<_>>();
                path.push(variable.name(db).to_string());
                path.push(dependency.name(db).to_string());
                return Err(Arc::new(StageFailure::one_for_definition(
                    CompilerStage::Semantic,
                    root_definition,
                    Diagnostic::semantic(
                        format!("package initialization cycle: {}", path.join(" -> ")),
                        SourceRef::definition(root_definition),
                    ),
                )));
            }
            let typed = evaluate(db, input, dependency, root_definition, stack, cache)?;
            variable_constants.insert(
                typed.name.clone(),
                ConstantSymbol {
                    id: crate::compiler::ids::QualifiedDefId::new(input.package(db), typed.id),
                    ty: typed.ty.clone(),
                    value: typed.value.clone(),
                },
            );
        }

        let mut variable_types = type_aliases.clone();
        for referenced_type in variable_references {
            let resolution = package_type_named_product(db, input, Arc::from(referenced_type))?;
            variable_types.extend(
                resolution
                    .types
                    .iter()
                    .map(|(name, ty)| (name.clone(), ty.clone())),
            );
        }
        if let Some(ty) =
            lower_package_variable_type(db, variable, &variable_types, &variable_constants)?
        {
            variables.insert(name.clone(), ty);
        }
    }
    let mut shadowed_predeclared = std::collections::BTreeSet::new();
    for source in input.sources(db).iter().copied() {
        let facts = file_projection(db, source);
        shadowed_predeclared.extend(
            facts
                .functions(db)
                .into_iter()
                .filter(|function| function.receiver_type(db).is_none())
                .map(|function| function.name(db).to_string()),
        );
        shadowed_predeclared.extend(
            facts
                .constants(db)
                .into_iter()
                .filter(|constant| constant.id(db) != definition)
                .map(|constant| constant.name(db).to_string()),
        );
        shadowed_predeclared.extend(
            facts
                .variables(db)
                .into_iter()
                .map(|variable| variable.name(db).to_string()),
        );
        shadowed_predeclared.extend(
            facts
                .type_aliases(db)
                .into_iter()
                .map(|declaration| declaration.name(db).to_string()),
        );
        shadowed_predeclared.extend(
            facts
                .type_definitions(db)
                .into_iter()
                .map(|declaration| declaration.name(db).to_string()),
        );
    }
    let typed = super::super::super::semantic::lower_constant_with_variables(
        definition,
        &syntax,
        &constants,
        &variables,
        &type_aliases,
        &shadowed_predeclared,
    )
    .map(Arc::new)
    .map_err(|diagnostic| semantic_failure(definition, diagnostic))?;
    stack.pop();
    cache.insert(definition, Arc::clone(&typed));
    Ok(typed)
}

pub(super) fn lower_package_variable_type(
    db: &dyn Db,
    variable: VariableProjection<'_>,
    type_aliases: &BTreeMap<String, crate::compiler::types::Ty>,
    constants: &BTreeMap<String, ConstantSymbol>,
) -> Result<Option<crate::compiler::types::Ty>, Arc<StageFailure>> {
    let definition = variable.id(db);
    check_semantic_barrier(db, definition, variable.semantic_barrier(db))?;
    semantic_build_dependency(db, definition)?;
    let syntax = variable.syntax(db);
    let Some(variable_type) = package_variable_type_syntax(&syntax) else {
        return Ok(None);
    };
    crate::compiler::semantic::lower_type_with_constants(
        variable_type,
        type_aliases,
        constants,
        SourceRef::definition(definition),
    )
    .map(Some)
    .map_err(|diagnostic| semantic_failure(definition, diagnostic))
}

pub(super) fn package_variable_type_syntax(syntax: &VariableSyntax) -> Option<&ExprSyntax> {
    syntax.explicit_type.as_ref().or_else(|| {
        let VariableValueSyntax::Expression(initializer) = &syntax.value else {
            return None;
        };
        let ExprSyntaxKind::CompositeLiteral { ty: Some(ty), .. } = &initializer.kind else {
            return None;
        };
        Some(ty.as_ref())
    })
}
