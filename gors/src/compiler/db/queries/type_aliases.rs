//! Package-wide type-alias resolution over owned declaration projections.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use super::support::{
    check_semantic_barrier, collect_all_expression_names, collect_constant_references,
    semantic_build_dependency,
};
use super::{
    ConstantProjection, Db, PackageInput, VariableProjection, file_projection, semantic_failure,
};
use crate::compiler::Diagnostic;
use crate::compiler::db::products::{CompilerStage, StageFailure, StageResult};
use crate::compiler::db::telemetry::QueryKind;
use crate::compiler::ids::{DefId, DefinitionKey, FileId, NodeId, QualifiedDefId};
use crate::compiler::provenance::{DefinitionSourceTable, FileRange, SourceRef};
use crate::compiler::semantic::ConstantSymbol;
use crate::compiler::syntax::{
    ExprSyntax, ExprSyntaxKind, FieldListSyntax, TypeAliasSyntax, TypeDeclarationLayout,
    TypeDefinitionSyntax,
};
use crate::compiler::types::Ty;

#[salsa::tracked]
pub(in crate::compiler::db) struct TypeAliasProjection<'db> {
    #[returns(copy)]
    pub(in crate::compiler::db) id: DefId,
    #[tracked]
    #[returns(clone)]
    pub(super) key: DefinitionKey,
    #[tracked]
    #[returns(clone)]
    pub(super) name: Arc<str>,
    #[tracked]
    #[returns(clone)]
    pub(super) syntax: Arc<TypeAliasSyntax>,
    #[tracked]
    #[returns(clone)]
    pub(super) layout: Arc<TypeDeclarationLayout>,
}

#[salsa::tracked]
pub(in crate::compiler::db) struct TypeDefinitionProjection<'db> {
    #[returns(copy)]
    pub(in crate::compiler::db) id: DefId,
    #[tracked]
    #[returns(clone)]
    pub(super) key: DefinitionKey,
    #[tracked]
    #[returns(clone)]
    pub(super) name: Arc<str>,
    #[tracked]
    #[returns(clone)]
    pub(super) syntax: Arc<TypeDefinitionSyntax>,
    #[tracked]
    #[returns(clone)]
    pub(super) layout: Arc<TypeDeclarationLayout>,
}

#[salsa::tracked(returns(clone))]
pub(in crate::compiler::db) fn type_alias_source_table_product(
    db: &dyn Db,
    file: FileId,
    declaration: TypeAliasProjection<'_>,
) -> StageResult<DefinitionSourceTable> {
    type_declaration_source_table(
        db,
        file,
        declaration.id(db),
        declaration.layout(db).as_ref(),
    )
}

#[salsa::tracked(returns(clone))]
pub(in crate::compiler::db) fn type_definition_source_table_product(
    db: &dyn Db,
    file: FileId,
    declaration: TypeDefinitionProjection<'_>,
) -> StageResult<DefinitionSourceTable> {
    type_declaration_source_table(
        db,
        file,
        declaration.id(db),
        declaration.layout(db).as_ref(),
    )
}

fn type_declaration_source_table(
    db: &dyn Db,
    file: FileId,
    definition: DefId,
    layout: &TypeDeclarationLayout,
) -> StageResult<DefinitionSourceTable> {
    db.query_telemetry()
        .record_query(QueryKind::DefinitionSourceTable);
    let mappings = std::iter::once((
        SourceRef::definition(definition),
        FileRange::new(file, layout.declaration()),
    ))
    .chain(
        (0_u32..)
            .zip(layout.sources().iter().copied())
            .map(|(index, range)| {
                (
                    SourceRef::node(NodeId::owner_local(definition, index)),
                    FileRange::new(file, range),
                )
            }),
    );
    DefinitionSourceTable::try_new(definition, file, layout.source_len(), mappings)
        .map(Arc::new)
        .map_err(|error| {
            Arc::new(StageFailure::one_for_definition(
                CompilerStage::Semantic,
                definition,
                Diagnostic::backend(format!(
                    "failed to construct type declaration source table: {error}"
                )),
            ))
        })
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
    let mut constants = BTreeMap::new();
    let mut variables = BTreeMap::new();
    let mut shadowed_predeclared = BTreeSet::new();
    for source in sources {
        let facts = file_projection(db, source);
        shadowed_predeclared.extend(
            facts
                .functions(db)
                .into_iter()
                .filter(|function| function.receiver_type(db).is_none())
                .map(|function| function.name(db).to_string()),
        );
        for variable in facts.variables(db) {
            shadowed_predeclared.insert(variable.name(db).to_string());
            if variable.name(db).as_ref() != "_" {
                variables.insert(variable.name(db).to_string(), variable);
            }
        }
        for alias in facts.type_aliases(db) {
            shadowed_predeclared.insert(alias.name(db).to_string());
            if alias.syntax(db).type_parameters.is_none() {
                projections.insert(alias.name(db).to_string(), alias);
            }
        }
        for definition in facts.type_definitions(db) {
            shadowed_predeclared.insert(definition.name(db).to_string());
            let syntax = definition.syntax(db);
            if syntax.type_parameters.is_none() && !is_constraint_type(&syntax.underlying) {
                definitions.insert(definition.name(db).to_string(), definition);
            }
        }
        for constant in facts.constants(db) {
            if constant.name(db).as_ref() != "_" {
                shadowed_predeclared.insert(constant.name(db).to_string());
                constants.insert(constant.name(db).to_string(), constant);
            }
        }
    }

    let mut resolved = BTreeMap::new();
    let mut stack = Vec::new();
    let context = TypeResolutionContext {
        db,
        input,
        projections: &projections,
        definitions: &definitions,
        constants: &constants,
        variables: &variables,
        shadowed_predeclared: &shadowed_predeclared,
    };
    let mut constant_cache = BTreeMap::new();
    let mut constant_stack = Vec::new();
    for name in projections.keys().chain(definitions.keys()) {
        resolve_type_name(
            &context,
            name,
            &mut resolved,
            &mut stack,
            &mut constant_cache,
            &mut constant_stack,
            TypeResolutionMode::ROOT,
        )?;
    }
    Ok(Arc::new(resolved))
}

/// Resolve one package type and only its transitive type/constant
/// dependencies. Constant evaluation uses this narrow query so an unrelated
/// array declaration whose length names that constant cannot introduce a
/// package-wide query cycle.
#[salsa::tracked(returns(clone))]
pub(in crate::compiler::db) fn package_type_named_product(
    db: &dyn Db,
    input: PackageInput,
    name: Arc<str>,
) -> StageResult<NamedTypeResolution> {
    db.query_telemetry()
        .record_query(QueryKind::PackageTypeLookup);
    let mut sources = input.sources(db).iter().copied().collect::<Vec<_>>();
    sources.sort_by_key(|source| source.file(db));
    let mut projections = BTreeMap::new();
    let mut definitions = BTreeMap::new();
    let mut constants = BTreeMap::new();
    let mut variables = BTreeMap::new();
    let mut shadowed_predeclared = BTreeSet::new();
    for source in sources {
        let facts = file_projection(db, source);
        shadowed_predeclared.extend(
            facts
                .functions(db)
                .into_iter()
                .filter(|function| function.receiver_type(db).is_none())
                .map(|function| function.name(db).to_string()),
        );
        for variable in facts.variables(db) {
            shadowed_predeclared.insert(variable.name(db).to_string());
            if variable.name(db).as_ref() != "_" {
                variables.insert(variable.name(db).to_string(), variable);
            }
        }
        for alias in facts.type_aliases(db) {
            shadowed_predeclared.insert(alias.name(db).to_string());
            projections.insert(alias.name(db).to_string(), alias);
        }
        for definition in facts.type_definitions(db) {
            shadowed_predeclared.insert(definition.name(db).to_string());
            definitions.insert(definition.name(db).to_string(), definition);
        }
        for constant in facts.constants(db) {
            if constant.name(db).as_ref() != "_" {
                shadowed_predeclared.insert(constant.name(db).to_string());
                constants.insert(constant.name(db).to_string(), constant);
            }
        }
    }
    if !projections.contains_key(name.as_ref()) && !definitions.contains_key(name.as_ref()) {
        return Ok(Arc::new(NamedTypeResolution {
            types: BTreeMap::new(),
            constants: BTreeMap::new(),
        }));
    }

    let context = TypeResolutionContext {
        db,
        input,
        projections: &projections,
        definitions: &definitions,
        constants: &constants,
        variables: &variables,
        shadowed_predeclared: &shadowed_predeclared,
    };
    let mut resolved = BTreeMap::new();
    let mut constant_cache = BTreeMap::new();
    resolve_type_name(
        &context,
        name.as_ref(),
        &mut resolved,
        &mut Vec::new(),
        &mut constant_cache,
        &mut Vec::new(),
        TypeResolutionMode::ROOT,
    )?;
    Ok(Arc::new(NamedTypeResolution {
        types: resolved,
        constants: constant_cache,
    }))
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

struct TypeResolutionContext<'db, 'declarations> {
    db: &'db dyn Db,
    input: PackageInput,
    projections: &'declarations BTreeMap<String, TypeAliasProjection<'db>>,
    definitions: &'declarations BTreeMap<String, TypeDefinitionProjection<'db>>,
    constants: &'declarations BTreeMap<String, ConstantProjection<'db>>,
    variables: &'declarations BTreeMap<String, VariableProjection<'db>>,
    shadowed_predeclared: &'declarations BTreeSet<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::compiler::db) struct NamedTypeResolution {
    pub(super) types: BTreeMap<String, Ty>,
    pub(super) constants: BTreeMap<DefId, ConstantSymbol>,
}

#[derive(Clone, Copy)]
struct TypeResolutionMode {
    incoming_guarded: bool,
    allow_incomplete: bool,
}

impl TypeResolutionMode {
    const ROOT: Self = Self {
        incoming_guarded: false,
        allow_incomplete: false,
    };
}

fn resolve_type_name(
    context: &TypeResolutionContext<'_, '_>,
    name: &str,
    resolved: &mut BTreeMap<String, Ty>,
    stack: &mut Vec<(String, bool)>,
    constant_cache: &mut BTreeMap<DefId, ConstantSymbol>,
    constant_stack: &mut Vec<(DefId, Arc<str>)>,
    mode: TypeResolutionMode,
) -> Result<Ty, Arc<StageFailure>> {
    let TypeResolutionMode {
        incoming_guarded,
        allow_incomplete,
    } = mode;
    let declaration = if let Some(alias) = context.projections.get(name).copied() {
        let definition = alias.id(context.db);
        let syntax = alias.syntax(context.db);
        if syntax.type_parameters.is_some() {
            return Err(semantic_failure(
                definition,
                Diagnostic::unsupported(
                    "generic type aliases require an explicit instantiation",
                    SourceRef::definition(definition),
                ),
            ));
        }
        (definition, syntax.target.clone(), false)
    } else if let Some(defined) = context.definitions.get(name).copied() {
        let definition = defined.id(context.db);
        let syntax = defined.syntax(context.db);
        if syntax.type_parameters.is_some() || is_constraint_type(&syntax.underlying) {
            return Err(semantic_failure(
                definition,
                Diagnostic::unsupported(
                    "generic or constraint types require an explicit instantiation",
                    SourceRef::definition(definition),
                ),
            ));
        }
        (definition, syntax.underlying.clone(), true)
    } else {
        return Err(Arc::new(StageFailure::one(
            CompilerStage::Semantic,
            Diagnostic::backend(format!("missing projected type declaration {name}")),
        )));
    };
    let (definition, expression, is_definition) = declaration;
    if allow_incomplete && let Some(ty) = resolved.get(name) {
        return Ok(ty.clone());
    }
    if allow_incomplete
        && stack.iter().any(|(entry, _)| entry == name)
        && let Ok(target) = lower_declaration_target(
            &expression,
            resolved,
            &BTreeMap::new(),
            context.shadowed_predeclared,
            definition,
        )
    {
        return Ok(if is_definition {
            Ty::Named {
                definition,
                underlying: Box::new(target.underlying().clone()),
            }
        } else {
            target
        });
    }
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
                .all(|(entry, _)| context.definitions.contains_key(entry));
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

    let previously_resolved = resolved.keys().cloned().collect::<BTreeSet<_>>();
    stack.push((name.to_owned(), incoming_guarded));
    let mut dependencies = BTreeMap::new();
    collect_type_dependencies(&expression, &mut dependencies, false);
    for (dependency, guarded) in dependencies {
        if context.projections.contains_key(&dependency)
            || context.definitions.contains_key(&dependency)
        {
            let target_ty = resolve_type_name(
                context,
                &dependency,
                resolved,
                stack,
                constant_cache,
                constant_stack,
                TypeResolutionMode {
                    incoming_guarded: guarded,
                    allow_incomplete,
                },
            )?;
            resolved.insert(dependency, target_ty);
        }
    }

    // Go exposes an array type's incomplete zero length while its declaration
    // is being evaluated. This is what makes declarations such as
    // `type A [len((*A)(nil)) + 1]int` well-defined: the self-reference sees
    // length zero, while only the completed length-one type is published.
    // Keep that state resolver-local and install it only for a named array.
    if is_definition && let Some(element) = fixed_array_element(&expression) {
        let mut element_constants = BTreeMap::new();
        let mut element_names = BTreeSet::new();
        collect_all_expression_names(element, &mut element_names);
        for element_name in element_names {
            let Some(constant) = context.constants.get(&element_name).copied() else {
                continue;
            };
            let symbol = resolve_type_constant(
                context,
                constant,
                resolved,
                stack,
                constant_cache,
                constant_stack,
                constant.id(context.db),
            )?;
            element_constants.insert(element_name, symbol);
        }
        let element_ty = lower_declaration_target(
            element,
            resolved,
            &element_constants,
            context.shadowed_predeclared,
            definition,
        )?;
        resolved.insert(
            name.to_owned(),
            Ty::Named {
                definition,
                underlying: Box::new(Ty::Array(0, Box::new(element_ty))),
            },
        );
    }

    let mut referenced_names = BTreeSet::new();
    collect_all_expression_names(&expression, &mut referenced_names);
    for referenced_name in &referenced_names {
        if resolved.contains_key(referenced_name)
            || (!context.projections.contains_key(referenced_name)
                && !context.definitions.contains_key(referenced_name))
        {
            continue;
        }
        let ty = resolve_type_name(
            context,
            referenced_name,
            resolved,
            stack,
            constant_cache,
            constant_stack,
            TypeResolutionMode {
                incoming_guarded: false,
                allow_incomplete: true,
            },
        )?;
        resolved.insert(referenced_name.clone(), ty);
    }
    let mut constant_symbols = BTreeMap::new();
    for referenced_name in referenced_names {
        let Some(constant) = context.constants.get(&referenced_name).copied() else {
            continue;
        };
        let symbol = resolve_type_constant(
            context,
            constant,
            resolved,
            stack,
            constant_cache,
            constant_stack,
            constant.id(context.db),
        );
        constant_symbols.insert(referenced_name, symbol?);
    }
    let target = lower_declaration_target(
        &expression,
        resolved,
        &constant_symbols,
        context.shadowed_predeclared,
        definition,
    )?;
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

    // A type reached only through this declaration's resolver-local
    // incomplete array may contain a cloned length-zero view. Once the array
    // is complete, refresh every newly reached type that is not still active
    // on an outer resolution stack so no incomplete clone can escape in the
    // published map.
    let refresh = if is_definition && fixed_array_element(&expression).is_some() {
        resolved
            .keys()
            .filter(|resolved_name| {
                resolved_name.as_str() != name
                    && !previously_resolved.contains(*resolved_name)
                    && !stack.iter().any(|(active, _)| active == *resolved_name)
            })
            .cloned()
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    for dependent in refresh {
        let previous = resolved.remove(&dependent).ok_or_else(|| {
            Arc::new(StageFailure::one(
                CompilerStage::Semantic,
                Diagnostic::backend("type selected for refresh disappeared"),
            ))
        })?;
        match resolve_type_name(
            context,
            &dependent,
            resolved,
            stack,
            constant_cache,
            constant_stack,
            TypeResolutionMode {
                incoming_guarded: false,
                allow_incomplete: !stack.is_empty(),
            },
        ) {
            Ok(refreshed) => {
                resolved.insert(dependent, refreshed);
            }
            Err(failure) => {
                resolved.insert(dependent, previous);
                return Err(failure);
            }
        }
    }
    Ok(ty)
}

fn fixed_array_element(expression: &ExprSyntax) -> Option<&ExprSyntax> {
    match &expression.kind {
        ExprSyntaxKind::Paren(inner) => fixed_array_element(inner),
        ExprSyntaxKind::ArrayType {
            length: Some(_),
            element,
        } => Some(element),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn resolve_type_constant(
    context: &TypeResolutionContext<'_, '_>,
    constant: ConstantProjection<'_>,
    resolved: &mut BTreeMap<String, Ty>,
    type_stack: &mut Vec<(String, bool)>,
    cache: &mut BTreeMap<DefId, ConstantSymbol>,
    stack: &mut Vec<(DefId, Arc<str>)>,
    root_definition: DefId,
) -> Result<ConstantSymbol, Arc<StageFailure>> {
    let definition = constant.id(context.db);
    if let Some(cached) = cache.get(&definition) {
        return Ok(cached.clone());
    }
    if let Some(start) = stack.iter().position(|(id, _)| *id == definition) {
        let mut path = stack
            .iter()
            .skip(start)
            .map(|(_, name)| name.to_string())
            .collect::<Vec<_>>();
        path.push(constant.name(context.db).to_string());
        return Err(Arc::new(StageFailure::one_for_definition(
            CompilerStage::Semantic,
            root_definition,
            Diagnostic::semantic(
                format!("constant initialization cycle: {}", path.join(" -> ")),
                SourceRef::definition(root_definition),
            ),
        )));
    }

    check_semantic_barrier(
        context.db,
        definition,
        constant.semantic_barrier(context.db),
    )?;
    semantic_build_dependency(context.db, definition)?;
    stack.push((definition, constant.name(context.db)));

    let mut constants = BTreeMap::new();
    for dependency_name in constant.dependencies(context.db).iter() {
        context.db.unwind_if_revision_cancelled();
        let Some(dependency) = context.constants.get(dependency_name.as_ref()).copied() else {
            continue;
        };
        let symbol = resolve_type_constant(
            context,
            dependency,
            resolved,
            type_stack,
            cache,
            stack,
            root_definition,
        )?;
        constants.insert(dependency_name.to_string(), symbol);
    }

    let syntax = constant.syntax(context.db);
    let mut referenced_names = BTreeSet::new();
    collect_constant_references(&syntax, &mut referenced_names);
    for referenced_name in &referenced_names {
        if resolved.contains_key(referenced_name)
            || (!context.projections.contains_key(referenced_name)
                && !context.definitions.contains_key(referenced_name))
        {
            continue;
        }
        let ty = resolve_type_name(
            context,
            referenced_name,
            resolved,
            type_stack,
            cache,
            stack,
            TypeResolutionMode {
                incoming_guarded: false,
                allow_incomplete: true,
            },
        )?;
        resolved.insert(referenced_name.clone(), ty);
    }

    let mut variables = BTreeMap::new();
    for referenced_name in &referenced_names {
        let Some(variable) = context.variables.get(referenced_name).copied() else {
            continue;
        };
        let variable_syntax = variable.syntax(context.db);
        let Some(variable_type) =
            super::constant_eval::package_variable_type_syntax(&variable_syntax)
        else {
            continue;
        };
        let mut variable_references = BTreeSet::new();
        collect_all_expression_names(variable_type, &mut variable_references);
        let mut variable_constants = constants.clone();
        for dependency_name in &variable_references {
            let Some(dependency) = context.constants.get(dependency_name).copied() else {
                continue;
            };
            let dependency_definition = dependency.id(context.db);
            if let Some(cycle_start) = stack
                .iter()
                .position(|(id, _)| *id == dependency_definition)
            {
                let mut path = stack
                    .iter()
                    .skip(cycle_start)
                    .map(|(_, name)| name.to_string())
                    .collect::<Vec<_>>();
                path.push(variable.name(context.db).to_string());
                path.push(dependency.name(context.db).to_string());
                return Err(Arc::new(StageFailure::one_for_definition(
                    CompilerStage::Semantic,
                    root_definition,
                    Diagnostic::semantic(
                        format!("package initialization cycle: {}", path.join(" -> ")),
                        SourceRef::definition(root_definition),
                    ),
                )));
            }
            let symbol = resolve_type_constant(
                context,
                dependency,
                resolved,
                type_stack,
                cache,
                stack,
                root_definition,
            )?;
            variable_constants.insert(dependency_name.clone(), symbol);
        }
        for type_name in &variable_references {
            if resolved.contains_key(type_name)
                || (!context.projections.contains_key(type_name)
                    && !context.definitions.contains_key(type_name))
            {
                continue;
            }
            let ty = resolve_type_name(
                context,
                type_name,
                resolved,
                type_stack,
                cache,
                stack,
                TypeResolutionMode {
                    incoming_guarded: false,
                    allow_incomplete: true,
                },
            )?;
            resolved.insert(type_name.clone(), ty);
        }
        if let Some(ty) = super::constant_eval::lower_package_variable_type(
            context.db,
            variable,
            resolved,
            &variable_constants,
        )? {
            variables.insert(referenced_name.clone(), ty);
        }
    }

    let mut shadowed_predeclared = context.shadowed_predeclared.clone();
    shadowed_predeclared.remove(constant.name(context.db).as_ref());
    let typed = crate::compiler::semantic::lower_constant_with_variables(
        definition,
        &syntax,
        &constants,
        &variables,
        resolved,
        &shadowed_predeclared,
    )
    .map_err(|diagnostic| semantic_failure(definition, diagnostic))?;
    stack.pop();
    let symbol = ConstantSymbol {
        id: QualifiedDefId::new(context.input.package(context.db), typed.id),
        ty: typed.ty,
        value: typed.value,
    };
    cache.insert(definition, symbol.clone());
    Ok(symbol)
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
        | ExprSyntaxKind::IndexList { .. }
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
    constants: &BTreeMap<String, ConstantSymbol>,
    shadowed_predeclared: &BTreeSet<String>,
    definition: DefId,
) -> Result<Ty, Arc<StageFailure>> {
    if let ExprSyntaxKind::Paren(inner) = &expression.kind {
        return lower_declaration_target(
            inner,
            resolved,
            constants,
            shadowed_predeclared,
            definition,
        );
    }
    if let ExprSyntaxKind::ArrayType {
        length: Some(length),
        element,
    } = &expression.kind
    {
        let source = SourceRef::definition(definition);
        let (length_ty, length_value) = crate::compiler::semantic::eval_constant(
            length,
            constants,
            resolved,
            shadowed_predeclared,
            source,
            None,
        )
        .map_err(|diagnostic| semantic_failure(definition, diagnostic))?;
        let length = crate::compiler::semantic::array_length_from_constant(
            &length_ty,
            &length_value,
            source,
        )
        .map_err(|diagnostic| semantic_failure(definition, diagnostic))?;
        let element = lower_declaration_target(
            element,
            resolved,
            constants,
            shadowed_predeclared,
            definition,
        )?;
        return Ok(Ty::Array(length, Box::new(element)));
    }
    crate::compiler::semantic::lower_type_with_constants(
        expression,
        resolved,
        constants,
        SourceRef::definition(definition),
    )
    .map_err(|diagnostic| semantic_failure(definition, diagnostic))
}
