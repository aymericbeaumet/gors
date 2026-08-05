//! Salsa ingredients and the first source-projection query graph.

mod analysis;
mod constant_eval;
mod hir_dependencies;
mod support;
mod type_aliases;

pub(super) use analysis::{
    body_product, file_analysis_product, package_analysis_product, public_api_product,
    signature_product,
};
use hir_dependencies::direct_callees;
use support::{
    check_semantic_barrier, collect_constant_references, function_file, referenced_names_in_body,
    semantic_build_dependency, semantic_failure,
};
pub(super) use type_aliases::{TypeAliasProjection, package_type_aliases_product};

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::ast;
use crate::compiler::fingerprint::{fingerprint_parts, rust_ir_root_inputs};
use crate::compiler::input::SourceContent;
use crate::compiler::provenance::{DefinitionSourceTable, FileRange, SourceRef};
use crate::compiler::syntax::{
    ConstantLayout, ConstantSyntax, FunctionLayout, ProjectedConstantSyntax,
    ProjectedFunctionSyntax, project_constant, project_function, project_type_alias,
};
use crate::compiler::{Diagnostic, lowering, mir, rust_ir};
use crate::source::SourceCoordinateMap;

use super::super::ids::{DefId, DefinitionKey, DefinitionKind, FileId, PackageId};
use super::model::{FileIssue, FunctionBody, FunctionSignature, ParseFailure, RuntimeAbiId};
use super::products::{
    CompilerStage, MirSignatureDependencies, NormalizedMirFunction, RustSignatureDependencies,
    SemanticFunctionProduct, StageFailure, StageResult, TypedFunctionSignature, TypedHirFunction,
    VerifiedMirFunction, VerifiedRustIrFunction, VerifiedRustIrPackage,
};
use super::source_metadata::{FileComments, FileImports};
use super::source_projection::{project_comments, project_imports};
use super::telemetry::{QueryKind, Telemetry};

#[salsa::db]
pub(super) trait Db: salsa::Database {
    fn query_telemetry(&self) -> &Telemetry;
    fn query_build_input(&self) -> Option<BuildInput>;
}

#[salsa::input]
pub(super) struct SourceInput {
    #[returns(copy)]
    pub(super) package: PackageId,
    #[returns(copy)]
    pub(super) file: FileId,
    #[returns(clone)]
    pub(super) logical_path: Arc<str>,
    #[returns(clone)]
    pub(super) content: Arc<SourceContent>,
}

#[salsa::input]
pub(super) struct PackageInput {
    #[returns(copy)]
    pub(super) package: PackageId,
    #[returns(clone)]
    pub(super) sources: Arc<[SourceInput]>,
}

#[salsa::input]
pub(super) struct BuildInput {
    #[returns(clone)]
    pub(super) go_version: Arc<str>,
    #[returns(copy)]
    pub(super) runtime_abi: RuntimeAbiId,
}

#[salsa::tracked]
pub(super) struct FunctionProjection<'db> {
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
    pub(super) package_name: Arc<str>,
    #[tracked]
    #[returns(clone)]
    pub(super) signature: Arc<FunctionSignature>,
    #[tracked]
    #[returns(clone)]
    pub(super) body: Arc<FunctionBody>,
    #[tracked]
    #[returns(clone)]
    pub(super) layout: Arc<FunctionLayout>,
    #[tracked]
    #[returns(clone)]
    pub(super) semantic_barrier: Option<Arc<str>>,
}

#[salsa::tracked]
pub(super) struct ConstantProjection<'db> {
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
    pub(super) syntax: Arc<ConstantSyntax>,
    #[tracked]
    #[returns(clone)]
    pub(super) layout: Arc<ConstantLayout>,
    #[tracked]
    #[returns(clone)]
    pub(super) dependencies: Arc<[Arc<str>]>,
    #[tracked]
    #[returns(clone)]
    pub(super) semantic_barrier: Option<Arc<str>>,
}

#[salsa::tracked]
pub(super) struct FileFacts<'db> {
    #[returns(copy)]
    pub(super) file: FileId,
    #[tracked]
    #[returns(copy)]
    pub(super) package_id: PackageId,
    #[tracked]
    #[returns(clone)]
    pub(super) logical_path: Arc<str>,
    #[tracked]
    #[returns(clone)]
    pub(super) coordinate_map: Arc<SourceCoordinateMap>,
    #[tracked]
    #[returns(clone)]
    pub(super) package: Arc<str>,
    #[tracked]
    #[returns(clone)]
    pub(super) imports: Arc<FileImports>,
    #[tracked]
    #[returns(clone)]
    pub(super) comments: Arc<FileComments>,
    #[tracked]
    #[returns(clone)]
    pub(super) functions: Vec<FunctionProjection<'db>>,
    #[tracked]
    #[returns(clone)]
    pub(super) constants: Vec<ConstantProjection<'db>>,
    #[tracked]
    #[returns(clone)]
    pub(super) type_aliases: Vec<TypeAliasProjection<'db>>,
    #[tracked]
    #[returns(clone)]
    pub(super) failure: Option<ParseFailure>,
    #[tracked]
    #[returns(clone)]
    pub(super) issues: Vec<FileIssue>,
}

#[salsa::tracked(returns(copy))]
pub(super) fn file_projection<'db>(db: &'db dyn Db, source: SourceInput) -> FileFacts<'db> {
    db.query_telemetry().record_query(QueryKind::FileProjection);
    let file = source.file(db);
    let package_id = source.package(db);
    let logical_path = source.logical_path(db);
    let content = source.content(db);
    let parsed = match crate::parser::parse_file(&logical_path, content.source()) {
        Ok(parsed) => parsed,
        Err(error) => {
            let coordinate_map = Arc::new(error.source_coordinate_map().clone());
            return FileFacts::new(
                db,
                file,
                package_id,
                logical_path,
                coordinate_map,
                Arc::from(""),
                Arc::new(FileImports::new(file, Arc::from([]), Arc::from([]))),
                Arc::new(FileComments::new(file, Arc::from([]))),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Some(ParseFailure::new(error.message(), error.physical_range())),
                Vec::new(),
            );
        }
    };
    let (parsed, coordinate_map, token_observations) = parsed.into_parts();
    let coordinate_map = Arc::new(coordinate_map);
    db.unwind_if_revision_cancelled();
    let declared_package: Arc<str> = Arc::from(parsed.name.name);
    let imports = Arc::new(project_imports(file, &content, &parsed));
    let comments = Arc::new(project_comments(file, &content, &parsed));

    let semantic_barrier = if !imports.direct().is_empty() || !imports.invalid().is_empty() {
        Some(Arc::from(
            "imported packages are not implemented by the semantic query pipeline",
        ))
    } else if parsed.decls.iter().any(|declaration| match declaration {
        ast::Decl::GenDecl(declaration) if declaration.tok == crate::token::Token::VAR => true,
        ast::Decl::GenDecl(declaration) if declaration.tok == crate::token::Token::TYPE => {
            declaration.specs.iter().any(|spec| {
                !matches!(
                    spec,
                    ast::Spec::TypeSpec(spec)
                        if spec.assign.is_some() && spec.type_params.is_none()
                )
            })
        }
        _ => false,
    }) {
        Some(Arc::from(
            "package variables and declared types are not implemented by the semantic query pipeline",
        ))
    } else {
        None
    };
    let mut seen = BTreeSet::<Arc<str>>::new();
    let mut projected_functions = Vec::new();
    let mut projected_constants = Vec::new();
    let mut projected_type_aliases = Vec::new();
    let mut issues = Vec::new();
    for declaration in &parsed.decls {
        match declaration {
            ast::Decl::FuncDecl(function) => {
                let name: Arc<str> = Arc::from(function.name.name);
                if !seen.insert(Arc::clone(&name)) {
                    issues.push(FileIssue::DuplicateDefinition(name));
                    continue;
                }
                let key = DefinitionKey::package_named(
                    package_id,
                    DefinitionKind::Function,
                    function.name.name,
                );
                let id = key.id();
                let ProjectedFunctionSyntax {
                    anchor,
                    layout,
                    header,
                    body,
                    structural_header,
                    structural_body,
                } = match project_function(function, &token_observations) {
                    Ok(projected) => projected,
                    Err(error) => {
                        issues.push(FileIssue::FunctionProjectionFailure {
                            name,
                            message: Arc::from(error.to_string()),
                        });
                        continue;
                    }
                };
                let signature = Arc::new(FunctionSignature::new(
                    id,
                    Arc::clone(&name),
                    anchor.clone(),
                    header,
                    Arc::new(structural_header),
                    !function.type_.params.list.is_empty(),
                    function
                        .type_
                        .results
                        .as_ref()
                        .is_some_and(|results| !results.list.is_empty()),
                ));
                projected_functions.push((
                    id,
                    key,
                    name,
                    signature,
                    Arc::new(FunctionBody::new(
                        id,
                        anchor,
                        body,
                        Arc::new(structural_body),
                    )),
                    Arc::new(layout),
                ));
            }
            ast::Decl::GenDecl(declaration) if declaration.tok == crate::token::Token::CONST => {
                let mut previous_values = None;
                for (iota, spec) in declaration.specs.iter().enumerate() {
                    let ast::Spec::ValueSpec(spec) = spec else {
                        continue;
                    };
                    let explicit_values =
                        spec.values.as_deref().filter(|values| !values.is_empty());
                    if let Some(values) = explicit_values {
                        previous_values = Some(values);
                    }
                    let values = explicit_values.or(previous_values).unwrap_or_default();
                    let arity_mismatch = !values.is_empty() && values.len() != spec.names.len();
                    for (index, name) in spec.names.iter().enumerate() {
                        let owned_name: Arc<str> = Arc::from(name.name);
                        if !seen.insert(Arc::clone(&owned_name)) {
                            issues.push(FileIssue::DuplicateDefinition(owned_name));
                            continue;
                        }
                        let key = DefinitionKey::package_named(
                            package_id,
                            DefinitionKind::Constant,
                            name.name,
                        );
                        let id = key.id();
                        let ProjectedConstantSyntax { syntax, layout } = match project_constant(
                            name,
                            spec.type_.as_ref(),
                            values.get(index),
                            arity_mismatch,
                            u64::try_from(iota).unwrap_or(u64::MAX),
                            content.text_len(),
                        ) {
                            Ok(projected) => projected,
                            Err(error) => {
                                issues.push(FileIssue::ConstantProjectionFailure {
                                    name: owned_name,
                                    message: Arc::from(error.to_string()),
                                });
                                continue;
                            }
                        };
                        let mut referenced = BTreeSet::new();
                        collect_constant_references(&syntax, &mut referenced);
                        let dependencies = referenced
                            .into_iter()
                            .filter(|name| !matches!(name.as_str(), "true" | "false" | "iota"))
                            .map(Arc::<str>::from)
                            .collect::<Vec<_>>();
                        projected_constants.push((
                            id,
                            key,
                            Arc::clone(&owned_name),
                            Arc::new(syntax),
                            Arc::new(layout),
                            Arc::<[Arc<str>]>::from(dependencies),
                        ));
                    }
                }
            }
            ast::Decl::GenDecl(declaration) if declaration.tok == crate::token::Token::TYPE => {
                for spec in &declaration.specs {
                    let ast::Spec::TypeSpec(spec) = spec else {
                        continue;
                    };
                    if spec.assign.is_none() || spec.type_params.is_some() {
                        continue;
                    }
                    let Some(name) = spec.name.as_ref() else {
                        continue;
                    };
                    let owned_name: Arc<str> = Arc::from(name.name);
                    if !seen.insert(Arc::clone(&owned_name)) {
                        issues.push(FileIssue::DuplicateDefinition(owned_name));
                        continue;
                    }
                    let key =
                        DefinitionKey::package_named(package_id, DefinitionKind::Type, name.name);
                    let id = key.id();
                    match project_type_alias(spec) {
                        Ok(syntax) => {
                            projected_type_aliases.push((id, key, owned_name, Arc::new(syntax)))
                        }
                        Err(error) => issues.push(FileIssue::TypeProjectionFailure {
                            name: owned_name,
                            message: Arc::from(error.to_string()),
                        }),
                    }
                }
            }
            ast::Decl::GenDecl(_) => {}
        }
    }
    projected_functions.sort_by_key(|(id, _, _, _, _, _)| *id);
    projected_constants.sort_by_key(|(id, _, _, _, _, _)| *id);
    projected_type_aliases.sort_by_key(|(id, _, _, _)| *id);
    issues.sort();

    let functions = projected_functions
        .into_iter()
        .map(|(id, key, name, signature, body, layout)| {
            FunctionProjection::new(
                db,
                id,
                key,
                name,
                Arc::clone(&declared_package),
                signature,
                body,
                layout,
                semantic_barrier.clone(),
            )
        })
        .collect();
    let constants = projected_constants
        .into_iter()
        .map(|(id, key, name, syntax, layout, dependencies)| {
            ConstantProjection::new(
                db,
                id,
                key,
                name,
                syntax,
                layout,
                dependencies,
                semantic_barrier.clone(),
            )
        })
        .collect();
    let type_aliases = projected_type_aliases
        .into_iter()
        .map(|(id, key, name, syntax)| TypeAliasProjection::new(db, id, key, name, syntax))
        .collect();
    FileFacts::new(
        db,
        file,
        package_id,
        logical_path,
        coordinate_map,
        declared_package,
        imports,
        comments,
        functions,
        constants,
        type_aliases,
        None,
        issues,
    )
}

#[salsa::tracked(returns(clone))]
pub(super) fn definition_source_table_product(
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
pub(super) fn constant_source_table_product(
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
pub(super) fn function_layout_product(
    db: &dyn Db,
    function: FunctionProjection<'_>,
) -> Arc<FunctionLayout> {
    db.query_telemetry().record_query(QueryKind::FunctionLayout);
    function.layout(db)
}

#[salsa::tracked(returns(clone))]
pub(super) fn semantic_function_product(
    db: &dyn Db,
    input: PackageInput,
    function: FunctionProjection<'_>,
) -> Arc<SemanticFunctionProduct> {
    let definition = function.id(db);
    let fallback_plan: Arc<[(SourceRef, crate::compiler::syntax::SyntaxSource)]> = Arc::from([(
        SourceRef::definition(definition),
        function.signature(db).structure().name.source,
    )]);
    if let Err(failure) = check_semantic_barrier(db, definition, function.semantic_barrier(db)) {
        return Arc::new(SemanticFunctionProduct::new(Err(failure), fallback_plan));
    }
    let signature = match typed_signature_product(db, input, function) {
        Ok(signature) => signature,
        Err(failure) => {
            return Arc::new(SemanticFunctionProduct::new(Err(failure), fallback_plan));
        }
    };
    let body = function.body(db);
    let names = referenced_names_in_body(function.signature(db).structure(), body.structure());
    let mut functions = BTreeMap::new();
    let mut constants = BTreeMap::new();
    let type_aliases = match package_type_aliases_product(db, input) {
        Ok(type_aliases) => type_aliases,
        Err(failure) => {
            return Arc::new(SemanticFunctionProduct::new(Err(failure), fallback_plan));
        }
    };
    for name in names {
        db.unwind_if_revision_cancelled();
        if let Some(dependency) = package_function_named_product(db, input, name.clone()) {
            let typed = match typed_signature_product(db, input, dependency) {
                Ok(typed) => typed,
                Err(failure) => {
                    return Arc::new(SemanticFunctionProduct::new(Err(failure), fallback_plan));
                }
            };
            functions.insert(
                name.to_string(),
                super::super::semantic::FunctionSymbol {
                    id: dependency.id(db),
                    signature: typed.signature().clone(),
                },
            );
        }
        if let Some(dependency) = package_constant_named_product(db, input, name.clone()) {
            let typed = match typed_constant_product(db, input, dependency) {
                Ok(typed) => typed,
                Err(failure) => {
                    return Arc::new(SemanticFunctionProduct::new(Err(failure), fallback_plan));
                }
            };
            constants.insert(
                name.to_string(),
                super::super::semantic::ConstantSymbol {
                    id: typed.id,
                    ty: typed.ty.clone(),
                    value: typed.value.clone(),
                },
            );
        }
    }
    match super::super::semantic::lower_function(
        definition,
        function.signature(db).structure(),
        body.structure(),
        signature.signature().clone(),
        functions,
        constants,
        type_aliases.as_ref().clone(),
    ) {
        Ok(lowered) => {
            let source_plan = Arc::from(lowered.source_plan.clone());
            Arc::new(SemanticFunctionProduct::new(
                Ok(Arc::new(lowered)),
                source_plan,
            ))
        }
        Err(failure) => {
            let source_plan = Arc::from(failure.source_plan);
            Arc::new(SemanticFunctionProduct::new(
                Err(semantic_failure(definition, failure.diagnostic)),
                source_plan,
            ))
        }
    }
}

#[salsa::tracked(returns(clone))]
pub(super) fn typed_hir_product(
    db: &dyn Db,
    input: PackageInput,
    function: FunctionProjection<'_>,
) -> StageResult<TypedHirFunction> {
    db.query_telemetry().record_query(QueryKind::TypedHir);
    semantic_function_product(db, input, function)
        .result()
        .clone()
        .map(|lowered| {
            Arc::new(TypedHirFunction::new(
                Arc::new(lowered.function.clone()),
                Arc::from(lowered.source_plan.clone()),
            ))
        })
}

#[salsa::tracked(returns(clone))]
pub(super) fn typed_signature_product(
    db: &dyn Db,
    input: PackageInput,
    function: FunctionProjection<'_>,
) -> StageResult<TypedFunctionSignature> {
    db.query_telemetry().record_query(QueryKind::TypedSignature);
    let definition = function.id(db);
    check_semantic_barrier(db, definition, function.semantic_barrier(db))?;
    semantic_build_dependency(db, definition)?;
    let type_aliases = package_type_aliases_product(db, input)?;
    super::super::semantic::lower_signature(
        definition,
        function.signature(db).structure(),
        &type_aliases,
    )
    .map(|signature| Arc::new(TypedFunctionSignature::new(definition, signature)))
    .map_err(|diagnostic| semantic_failure(definition, diagnostic))
}

#[salsa::tracked(returns(clone))]
pub(super) fn typed_constant_product(
    db: &dyn Db,
    input: PackageInput,
    constant: ConstantProjection<'_>,
) -> StageResult<super::super::semantic::TypedConstant> {
    db.query_telemetry().record_query(QueryKind::TypedConstant);
    constant_eval::evaluate_constant(db, input, constant)
}

#[salsa::tracked(returns(copy))]
pub(super) fn package_function_product<'db>(
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
pub(super) fn package_function_named_product<'db>(
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
pub(super) fn package_constant_named_product<'db>(
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

#[salsa::tracked(returns(clone))]
pub(super) fn mir_signature_dependencies_product(
    db: &dyn Db,
    input: PackageInput,
    function: FunctionProjection<'_>,
) -> StageResult<MirSignatureDependencies> {
    db.query_telemetry()
        .record_query(QueryKind::SignatureDependencies);
    let hir = typed_hir_product(db, input, function)?;
    let caller = function.id(db);
    let mut definitions = direct_callees(hir.function());
    definitions.insert(caller);
    let mut signatures = BTreeMap::new();
    for definition in definitions {
        db.unwind_if_revision_cancelled();
        let projection = if definition == caller {
            function
        } else {
            package_function_product(db, input, definition).ok_or_else(|| {
                Arc::new(StageFailure::one_for_definition(
                    CompilerStage::GoMir,
                    caller,
                    Diagnostic::backend(format!(
                        "callee DefId {definition} is absent from the package index"
                    )),
                ))
            })?
        };
        let signature = typed_signature_product(db, input, projection)?;
        signatures.insert(definition, signature.signature().clone());
    }
    Ok(Arc::new(MirSignatureDependencies { signatures }))
}

#[salsa::tracked(returns(clone))]
pub(super) fn verified_mir_product(
    db: &dyn Db,
    input: PackageInput,
    function: FunctionProjection<'_>,
) -> StageResult<VerifiedMirFunction> {
    db.query_telemetry().record_query(QueryKind::VerifiedGoMir);
    let hir = typed_hir_product(db, input, function)?;
    db.unwind_if_revision_cancelled();
    let signatures = mir_signature_dependencies_product(db, input, function)?;
    db.unwind_if_revision_cancelled();
    let definition = function.id(db);
    let lowered = mir::lower_function(hir.function()).map_err(|diagnostic| {
        Arc::new(StageFailure::one_for_definition(
            CompilerStage::GoMir,
            definition,
            diagnostic,
        ))
    })?;
    db.unwind_if_revision_cancelled();
    mir::verify_function(&lowered, &signatures.signatures).map_err(|diagnostic| {
        Arc::new(StageFailure::one_for_definition(
            CompilerStage::GoMir,
            definition,
            diagnostic,
        ))
    })?;
    Ok(Arc::new(VerifiedMirFunction::new(lowered)))
}

#[salsa::tracked(returns(clone))]
pub(super) fn normalized_mir_product(
    db: &dyn Db,
    input: PackageInput,
    function: FunctionProjection<'_>,
) -> StageResult<NormalizedMirFunction> {
    db.query_telemetry()
        .record_query(QueryKind::NormalizedGoMir);
    let mir = verified_mir_product(db, input, function)?;
    db.unwind_if_revision_cancelled();
    let signatures = mir_signature_dependencies_product(db, input, function)?;
    db.unwind_if_revision_cancelled();
    let definition = function.id(db);
    let normalized = mir::normalize_function(mir.function().clone(), &signatures.signatures)
        .map_err(|diagnostics| {
            Arc::new(StageFailure::for_definition(
                CompilerStage::GoMirNormalization,
                definition,
                diagnostics,
            ))
        })?;
    db.unwind_if_revision_cancelled();
    mir::verify_function(&normalized, &signatures.signatures).map_err(|diagnostic| {
        Arc::new(StageFailure::one_for_definition(
            CompilerStage::GoMirNormalization,
            definition,
            diagnostic,
        ))
    })?;
    Ok(Arc::new(NormalizedMirFunction::new(normalized)))
}

#[salsa::tracked(returns(clone))]
pub(super) fn rust_signature_dependencies_product(
    db: &dyn Db,
    input: PackageInput,
    function: FunctionProjection<'_>,
) -> StageResult<RustSignatureDependencies> {
    let go_signatures = mir_signature_dependencies_product(db, input, function)?;
    let representation_key = representation_key(db)?;
    let signatures = go_signatures
        .signatures
        .iter()
        .map(|(id, signature)| {
            lowering::lower_signature(signature)
                .map(|signature| (*id, signature))
                .map_err(|diagnostic| {
                    Arc::new(StageFailure::one_for_definition(
                        CompilerStage::RustRepresentation,
                        *id,
                        diagnostic,
                    ))
                })
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    Ok(Arc::new(RustSignatureDependencies {
        signatures,
        representation_key,
    }))
}

#[salsa::tracked(returns(copy))]
pub(super) fn executable_role_product(db: &dyn Db, function: FunctionProjection<'_>) -> bool {
    db.query_telemetry().record_query(QueryKind::ExecutableRole);
    function.package_name(db).as_ref() == "main"
}

/// Complete provenance-free invalidation inputs for one Rust-IR function root.
///
/// Keeping this as a tracked query lets the retained session decide which
/// independent roots need a worker without first evaluating those Rust-IR
/// roots serially. Direct-callee ABI changes and representation changes are
/// part of the digest even when this function's own HIR stays unchanged.
#[salsa::tracked(returns(clone))]
pub(super) fn rust_ir_root_inputs_product(
    db: &dyn Db,
    input: PackageInput,
    function: FunctionProjection<'_>,
) -> StageResult<super::super::fingerprint::Fingerprint> {
    db.query_telemetry()
        .record_query(QueryKind::RustIrRootInputs);
    let hir = typed_hir_product(db, input, function)?;
    db.unwind_if_revision_cancelled();
    let signatures = mir_signature_dependencies_product(db, input, function)?;
    db.unwind_if_revision_cancelled();
    let representation_key = representation_key(db)?;
    let executable_package = executable_role_product(db, function);
    Ok(Arc::new(rust_ir_root_inputs(
        hir.function(),
        &signatures.signatures,
        representation_key,
        executable_package,
    )))
}

#[salsa::tracked(returns(clone))]
pub(super) fn verified_rust_ir_product(
    db: &dyn Db,
    input: PackageInput,
    function: FunctionProjection<'_>,
) -> StageResult<VerifiedRustIrFunction> {
    db.query_telemetry().record_query(QueryKind::VerifiedRustIr);
    let normalized = normalized_mir_product(db, input, function)?;
    db.unwind_if_revision_cancelled();
    let signatures = rust_signature_dependencies_product(db, input, function)?;
    db.unwind_if_revision_cancelled();
    let executable_package = executable_role_product(db, function);
    let definition = function.id(db);
    let lowered = lowering::lower_function(normalized.function().clone(), executable_package)
        .map_err(|diagnostic| {
            Arc::new(StageFailure::one_for_definition(
                CompilerStage::RustRepresentation,
                definition,
                diagnostic,
            ))
        })?;
    db.unwind_if_revision_cancelled();
    let runtime_requirement =
        rust_ir::verify_function(&lowered, &signatures.signatures).map_err(|diagnostic| {
            Arc::new(StageFailure::one_for_definition(
                CompilerStage::RustRepresentation,
                definition,
                diagnostic,
            ))
        })?;
    Ok(Arc::new(VerifiedRustIrFunction::new(
        lowered,
        signatures.representation_key,
        runtime_requirement,
    )))
}

#[salsa::tracked(returns(clone))]
pub(super) fn rust_ir_package_product(
    db: &dyn Db,
    input: PackageInput,
) -> StageResult<VerifiedRustIrPackage> {
    db.query_telemetry().record_query(QueryKind::RustIrPackage);
    let analysis = package_analysis_product(db, input);
    let mut functions = Vec::new();
    let mut runtime_requirement = rust_ir::RuntimeRequirement::default();
    let mut sources = input.sources(db).iter().copied().collect::<Vec<_>>();
    sources.sort_by_key(|source| source.file(db));
    for source in sources {
        let facts = file_projection(db, source);
        let mut constants = facts.constants(db);
        constants.sort_by_key(|constant| constant.id(db));
        for constant in constants {
            db.unwind_if_revision_cancelled();
            typed_constant_product(db, input, constant)?;
        }
        let mut projected = facts.functions(db);
        projected.sort_by_key(|function| function.id(db));
        for function in projected {
            db.unwind_if_revision_cancelled();
            let verified = verified_rust_ir_product(db, input, function)?;
            runtime_requirement = runtime_requirement.union(verified.runtime_requirement());
            functions.push(verified.function().clone());
        }
    }
    functions.sort_by_key(|function| function.id);
    let file = rust_ir::File {
        package: analysis.package_name().to_string(),
        functions,
    };
    let verified_runtime_requirement = rust_ir::verify(&file).map_err(|diagnostic| {
        Arc::new(StageFailure::one(
            CompilerStage::RustRepresentation,
            diagnostic,
        ))
    })?;
    if verified_runtime_requirement != runtime_requirement {
        return Err(Arc::new(StageFailure::one(
            CompilerStage::RustRepresentation,
            Diagnostic::backend(format!(
                "Rust IR package runtime requirement mismatch: function products derived {runtime_requirement:?}, package verification derived {verified_runtime_requirement:?}"
            )),
        )));
    }
    let representation_key = representation_key(db)?;
    Ok(Arc::new(VerifiedRustIrPackage::new(
        file,
        representation_key,
        runtime_requirement,
    )))
}

fn representation_key(
    db: &dyn Db,
) -> Result<super::super::fingerprint::Fingerprint, Arc<StageFailure>> {
    let build = db.query_build_input().ok_or_else(|| {
        Arc::new(StageFailure::one(
            CompilerStage::RustRepresentation,
            Diagnostic::backend(
                "compiler build config is missing from Rust representation lowering",
            ),
        ))
    })?;
    let runtime_abi = build.runtime_abi(db);
    Ok(fingerprint_parts(
        b"rust-representation-config",
        &[runtime_abi.as_bytes()],
    ))
}
