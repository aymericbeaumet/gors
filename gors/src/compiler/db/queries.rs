//! Salsa ingredients and the first source-projection query graph.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::ast;
use crate::compiler::fingerprint::{fingerprint_parts, rust_ir_root_inputs};
use crate::compiler::input::SourceContent;
use crate::compiler::source::{TextRange, TextSize};
use crate::compiler::{Diagnostic, lowering, mir, rust_ir};

use super::super::ids::{DefId, DefinitionKey, DefinitionKind, FileId, PackageId};
use super::model::{
    FileAnalysis, FileIssue, FunctionBody, FunctionDescriptor, FunctionSignature, PackageAnalysis,
    PackageIssue, ParseFailure, PublicApi,
};
use super::products::{
    CompilerStage, FunctionProvenance, MirSignatureDependencies, NormalizedMirFunction,
    RustSignatureDependencies, StageFailure, StageResult, TypedFunctionSignature, TypedHirFunction,
    VerifiedMirFunction, VerifiedRustIrFunction, VerifiedRustIrPackage,
};
use super::provenance::make_function_relative;
use super::source_metadata::{FileComments, FileImports};
use super::source_projection::{body_source, project_comments, project_imports, signature_source};
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
    pub(super) target: Arc<str>,
    #[returns(clone)]
    pub(super) go_version: Arc<str>,
    #[returns(clone)]
    pub(super) runtime_abi: Arc<str>,
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
    pub(super) typed_signature: Option<crate::compiler::types::Signature>,
    #[tracked]
    #[returns(clone)]
    pub(super) hir: StageResult<TypedHirFunction>,
    #[tracked]
    #[returns(clone)]
    pub(super) provenance: Arc<FunctionProvenance>,
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
    pub(super) failure: Option<ParseFailure>,
    #[tracked]
    #[returns(clone)]
    pub(super) issues: Vec<FileIssue>,
    #[tracked]
    #[returns(clone)]
    pub(super) semantic_failure: Option<Arc<StageFailure>>,
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
            let location = error.location();
            let physical_offset = error
                .byte_offset()
                .and_then(|offset| TextSize::try_from(offset).ok())
                .unwrap_or_else(|| content.text_len());
            return FileFacts::new(
                db,
                file,
                package_id,
                logical_path,
                Arc::from(""),
                Arc::new(FileImports::new(file, Arc::from([]), Arc::from([]))),
                Arc::new(FileComments::new(file, Arc::from([]))),
                Vec::new(),
                Some(ParseFailure::new(
                    error.message(),
                    TextRange::empty(physical_offset),
                    location.as_ref().map(|(_, line, _)| *line),
                    location.as_ref().map(|(_, _, column)| *column),
                )),
                Vec::new(),
                None,
            );
        }
    };
    db.unwind_if_revision_cancelled();
    let declared_package: Arc<str> = Arc::from(parsed.name.name);
    let imports = Arc::new(project_imports(file, &parsed));
    let comments = Arc::new(project_comments(file, &content, &parsed));

    let mut seen = BTreeSet::new();
    let mut projected = Vec::new();
    let mut issues = Vec::new();
    for declaration in &parsed.decls {
        let ast::Decl::FuncDecl(function) = declaration else {
            continue;
        };
        let name: Arc<str> = Arc::from(function.name.name);
        // The identity API can represent repeated `init` declarations, but
        // this parser has no stable syntax-node anchor from which to derive a
        // semantic disambiguator yet. Reject every repeated source name at
        // this indexing frontier instead of inventing an ordinal or offset.
        if !seen.insert(Arc::clone(&name)) {
            issues.push(FileIssue::DuplicateFunction(name));
            continue;
        }
        let key =
            DefinitionKey::package_named(package_id, DefinitionKind::Function, function.name.name);
        let id = key.id();
        let signature_source = signature_source(&content, &logical_path, function);
        let body_source = function
            .body
            .as_ref()
            .map(|body| body_source(&content, body));
        let signature = Arc::new(FunctionSignature::new(
            id,
            Arc::clone(&name),
            signature_source,
            !function.type_.params.list.is_empty(),
            function
                .type_
                .results
                .as_ref()
                .is_some_and(|results| !results.list.is_empty()),
        ));
        projected.push((
            id,
            key,
            name,
            signature,
            Arc::new(FunctionBody::new(id, body_source)),
            Arc::new(FunctionProvenance::new(
                Arc::clone(&logical_path),
                function.name.name_pos.offset,
                function.name.name_pos.line,
                function.name.name_pos.column,
            )),
        ));
    }
    projected.sort_by_key(|(id, _, _, _, _, _)| *id);
    issues.sort();

    db.query_telemetry().record_query(QueryKind::SemanticFile);
    // The current semantic frontier has no version-conditioned construct yet,
    // but consuming this field makes the pinned Go language version an
    // explicit dependency instead of an ambient or forgotten input.
    let context = super::super::semantic::SemanticContext {
        package: package_id,
        file,
        logical_file: logical_path.to_string(),
    };
    let semantic = if let Some(build) = db.query_build_input() {
        let _go_version = build.go_version(db);
        super::super::semantic::lower_file_with_context(&parsed, context)
    } else {
        Err(vec![Diagnostic::backend(
            "compiler build config is missing from the semantic query database",
        )])
    };
    let (mut typed_functions, semantic_failure) = match semantic {
        Ok(mut file) => {
            let functions = file
                .functions
                .drain(..)
                .map(|mut function| {
                    let provenance = make_function_relative(&mut function);
                    (function.id, (function, provenance))
                })
                .collect::<BTreeMap<_, _>>();
            (functions, None)
        }
        Err(diagnostics) => {
            let failure = Arc::new(StageFailure::for_file(
                CompilerStage::Semantic,
                file,
                diagnostics,
            ));
            (BTreeMap::new(), Some(failure))
        }
    };
    let functions = projected
        .into_iter()
        .map(|(id, key, name, signature, body, parsed_provenance)| {
            let (typed_signature, hir, provenance) = match typed_functions.remove(&id) {
                Some((function, provenance)) => {
                    let typed_signature = Some(function.signature.clone());
                    let hir = Ok(Arc::new(TypedHirFunction::new(Arc::new(function))));
                    (typed_signature, hir, Arc::new(provenance))
                }
                None => {
                    let failure = semantic_failure.clone().unwrap_or_else(|| {
                        Arc::new(StageFailure::one_for_definition(
                            CompilerStage::Semantic,
                            id,
                            Diagnostic::backend(format!(
                                "semantic lowering omitted indexed function DefId {id}"
                            )),
                        ))
                    });
                    (None, Err(failure), parsed_provenance)
                }
            };
            FunctionProjection::new(
                db,
                id,
                key,
                name,
                Arc::clone(&declared_package),
                signature,
                body,
                typed_signature,
                hir,
                provenance,
            )
        })
        .collect();
    FileFacts::new(
        db,
        file,
        package_id,
        logical_path,
        declared_package,
        imports,
        comments,
        functions,
        None,
        issues,
        semantic_failure,
    )
}

#[salsa::tracked(returns(clone))]
pub(super) fn semantic_status_product(
    db: &dyn Db,
    facts: FileFacts<'_>,
) -> Result<(), Arc<StageFailure>> {
    facts.semantic_failure(db).map_or(Ok(()), Err)
}

#[salsa::tracked(returns(clone))]
pub(super) fn provenance_product(
    db: &dyn Db,
    function: FunctionProjection<'_>,
) -> Arc<FunctionProvenance> {
    db.query_telemetry()
        .record_query(QueryKind::FunctionProvenance);
    function.provenance(db)
}

#[salsa::tracked(returns(clone))]
pub(super) fn typed_hir_product(
    db: &dyn Db,
    function: FunctionProjection<'_>,
) -> StageResult<TypedHirFunction> {
    db.query_telemetry().record_query(QueryKind::TypedHir);
    function.hir(db)
}

#[salsa::tracked(returns(clone))]
pub(super) fn typed_signature_product(
    db: &dyn Db,
    function: FunctionProjection<'_>,
) -> StageResult<TypedFunctionSignature> {
    db.query_telemetry().record_query(QueryKind::TypedSignature);
    let definition = function.id(db);
    function
        .typed_signature(db)
        .map(|signature| Arc::new(TypedFunctionSignature::new(definition, signature)))
        .ok_or_else(|| {
            Arc::new(StageFailure::one_for_definition(
                CompilerStage::Semantic,
                definition,
                Diagnostic::backend(format!(
                    "typed signature is missing for function DefId {definition}"
                )),
            ))
        })
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

#[salsa::tracked(returns(clone))]
pub(super) fn mir_signature_dependencies_product(
    db: &dyn Db,
    input: PackageInput,
    function: FunctionProjection<'_>,
) -> StageResult<MirSignatureDependencies> {
    db.query_telemetry()
        .record_query(QueryKind::SignatureDependencies);
    let hir = typed_hir_product(db, function)?;
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
        let signature = typed_signature_product(db, projection)?;
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
    let hir = typed_hir_product(db, function)?;
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
    let hir = typed_hir_product(db, function)?;
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
    let mut sources = input.sources(db).iter().copied().collect::<Vec<_>>();
    sources.sort_by_key(|source| source.file(db));
    for source in sources {
        let facts = file_projection(db, source);
        let mut projected = facts.functions(db);
        projected.sort_by_key(|function| function.id(db));
        for function in projected {
            db.unwind_if_revision_cancelled();
            functions.push(
                verified_rust_ir_product(db, input, function)?
                    .function()
                    .clone(),
            );
        }
    }
    functions.sort_by_key(|function| function.id);
    let file = rust_ir::File {
        package: analysis.package_name().to_string(),
        functions,
    };
    rust_ir::verify(&file).map_err(|diagnostic| {
        Arc::new(StageFailure::one(
            CompilerStage::RustRepresentation,
            diagnostic,
        ))
    })?;
    let representation_key = representation_key(db)?;
    Ok(Arc::new(VerifiedRustIrPackage::new(
        file,
        representation_key,
    )))
}

#[salsa::tracked(returns(clone))]
pub(super) fn file_analysis_product(db: &dyn Db, facts: FileFacts<'_>) -> Arc<FileAnalysis> {
    db.query_telemetry().record_query(QueryKind::FileAnalysis);
    let functions = facts
        .functions(db)
        .into_iter()
        .map(|function| {
            FunctionDescriptor::new(facts.file(db), function.key(db), function.name(db))
        })
        .collect::<Vec<_>>();
    Arc::new(FileAnalysis::new(
        facts.file(db),
        facts.package(db),
        functions.into(),
        facts.failure(db),
        facts.issues(db).into(),
    ))
}

#[salsa::tracked(returns(clone))]
pub(super) fn package_analysis_product(db: &dyn Db, input: PackageInput) -> Arc<PackageAnalysis> {
    db.query_telemetry()
        .record_query(QueryKind::PackageAnalysis);
    let package = input.package(db);
    let mut sources = input.sources(db).iter().copied().collect::<Vec<_>>();
    sources.sort_by_key(|source| source.file(db));

    let mut package_name: Option<Arc<str>> = None;
    let mut files = Vec::with_capacity(sources.len());
    let mut direct_imports = BTreeSet::new();
    let mut functions = Vec::new();
    let mut exported_signatures = Vec::new();
    let mut issues = Vec::new();
    let mut definitions_by_digest = BTreeMap::<DefId, (DefinitionKey, FileId)>::new();
    let mut declarations_by_name = BTreeMap::<Arc<str>, FileId>::new();

    for source in sources {
        db.unwind_if_revision_cancelled();
        let facts = file_projection(db, source);
        let file = facts.file(db);
        files.push(file);

        if let Some(failure) = facts.failure(db) {
            issues.push(PackageIssue::FileParseFailure { file, failure });
            continue;
        }

        let imports = facts.imports(db);
        direct_imports.extend(
            imports
                .direct()
                .iter()
                .map(|import| Arc::<str>::from(import.path())),
        );
        issues.extend(
            imports
                .invalid()
                .iter()
                .map(|import| PackageIssue::InvalidImportPath {
                    file: import.file(),
                    literal: Arc::from(import.literal()),
                    line: import.line(),
                    column: import.column(),
                    virtual_file: import.virtual_file().map(Arc::from),
                    issue: import.issue().clone(),
                }),
        );
        let declared_package = facts.package(db);
        if let Some(expected) = &package_name {
            if expected != &declared_package {
                issues.push(PackageIssue::PackageClauseMismatch {
                    file,
                    expected: Arc::clone(expected),
                    found: declared_package,
                });
            }
        } else {
            package_name = Some(declared_package);
        }

        for issue in facts.issues(db) {
            let FileIssue::DuplicateFunction(name) = issue;
            issues.push(PackageIssue::DuplicateDefinition {
                name,
                first_file: file,
                second_file: file,
            });
        }

        for function in facts.functions(db) {
            let id = function.id(db);
            let key = function.key(db);
            let name = function.name(db);

            if let Some(first_file) = declarations_by_name.insert(Arc::clone(&name), file) {
                issues.push(PackageIssue::DuplicateDefinition {
                    name: Arc::clone(&name),
                    first_file,
                    second_file: file,
                });
            }

            if let Some((existing_key, _)) = definitions_by_digest.get(&id) {
                if existing_key != &key {
                    issues.push(PackageIssue::IdentityCollision {
                        id,
                        existing_key: Arc::from(format!("{existing_key:?}")),
                        requested_key: Arc::from(format!("{key:?}")),
                    });
                }
            } else {
                definitions_by_digest.insert(id, (key.clone(), file));
            }

            if is_exported(&name) {
                exported_signatures.push(signature_product(db, function).as_ref().clone());
            }
            functions.push(FunctionDescriptor::new(file, key, name));
        }
    }

    files.sort();
    functions.sort_by_key(|function| (function.id(), function.file()));
    exported_signatures.sort_by_key(FunctionSignature::id);
    issues.sort();
    Arc::new(PackageAnalysis::new(
        package,
        package_name.unwrap_or_else(|| Arc::from("")),
        files.into(),
        direct_imports.into_iter().collect::<Vec<_>>().into(),
        functions.into(),
        issues.into(),
        &exported_signatures,
    ))
}

#[salsa::tracked(returns(clone))]
pub(super) fn signature_product(
    db: &dyn Db,
    function: FunctionProjection<'_>,
) -> Arc<FunctionSignature> {
    db.query_telemetry()
        .record_query(QueryKind::FunctionSignature);
    function.signature(db)
}

#[salsa::tracked(returns(clone))]
pub(super) fn body_product(db: &dyn Db, function: FunctionProjection<'_>) -> Arc<FunctionBody> {
    db.query_telemetry().record_query(QueryKind::FunctionBody);
    function.body(db)
}

#[salsa::tracked(returns(clone))]
pub(super) fn public_api_product(db: &dyn Db, facts: FileFacts<'_>) -> Arc<PublicApi> {
    db.query_telemetry().record_query(QueryKind::PublicApi);
    let signatures = facts
        .functions(db)
        .into_iter()
        .map(|function| signature_product(db, function).as_ref().clone())
        .collect::<Vec<_>>();
    Arc::new(PublicApi::new(facts.file(db), signatures.into()))
}

fn is_exported(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_uppercase)
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
    let target = build.target(db);
    let runtime_abi = build.runtime_abi(db);
    Ok(fingerprint_parts(
        b"rust-representation-config",
        &[target.as_bytes(), runtime_abi.as_bytes()],
    ))
}

fn direct_callees(function: &crate::compiler::hir::Function) -> BTreeSet<DefId> {
    let mut callees = BTreeSet::new();
    collect_block_callees(&function.body, &mut callees);
    callees
}

fn collect_block_callees(block: &crate::compiler::hir::Block, callees: &mut BTreeSet<DefId>) {
    for statement in &block.stmts {
        collect_statement_callees(statement, callees);
    }
}

fn collect_statement_callees(
    statement: &crate::compiler::hir::Stmt,
    callees: &mut BTreeSet<DefId>,
) {
    use crate::compiler::hir::StmtKind;
    match &statement.kind {
        StmtKind::Let { values, .. }
        | StmtKind::Assign { values, .. }
        | StmtKind::Return(values) => {
            for value in values {
                collect_expression_callees(value, callees);
            }
        }
        StmtKind::Expr(expression) => collect_expression_callees(expression, callees),
        StmtKind::If {
            init,
            condition,
            then_block,
            else_branch,
        } => {
            if let Some(init) = init {
                collect_statement_callees(init, callees);
            }
            collect_expression_callees(condition, callees);
            collect_block_callees(then_block, callees);
            if let Some(branch) = else_branch {
                collect_statement_callees(branch, callees);
            }
        }
        StmtKind::For {
            init,
            condition,
            post,
            body,
        } => {
            if let Some(init) = init {
                collect_statement_callees(init, callees);
            }
            if let Some(condition) = condition {
                collect_expression_callees(condition, callees);
            }
            if let Some(post) = post {
                collect_statement_callees(post, callees);
            }
            collect_block_callees(body, callees);
        }
        StmtKind::Block(block) => collect_block_callees(block, callees),
        StmtKind::Break | StmtKind::Continue => {}
    }
}

fn collect_expression_callees(
    expression: &crate::compiler::hir::Expr,
    callees: &mut BTreeSet<DefId>,
) {
    use crate::compiler::hir::{Callee, ExprKind};
    match &expression.kind {
        ExprKind::Binary { left, right, .. } => {
            collect_expression_callees(left, callees);
            collect_expression_callees(right, callees);
        }
        ExprKind::Unary { operand, .. } => collect_expression_callees(operand, callees),
        ExprKind::Call { callee, args } => {
            if let Callee::Function(definition) = callee {
                callees.insert(*definition);
            }
            for argument in args {
                collect_expression_callees(argument, callees);
            }
        }
        ExprKind::Constant(_) | ExprKind::Local(_) | ExprKind::GlobalConstant(..) => {}
    }
}
