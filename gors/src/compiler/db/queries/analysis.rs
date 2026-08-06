//! Body-independent file and package analysis queries.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use super::support::specialized_range_iterator_ids;
use super::{
    Db, FileFacts, FunctionProjection, PackageInput, file_projection, package_type_aliases_product,
    typed_constant_product, typed_variable_product,
};
use crate::compiler::db::model::{
    ConstantDescriptor, FileAnalysis, FileIssue, FunctionBody, FunctionDescriptor,
    FunctionSignature, PackageAnalysis, PackageIssue, PublicApi, TypeAliasDescriptor,
    TypeDefinitionDescriptor, VariableDescriptor,
};
use crate::compiler::db::telemetry::QueryKind;
use crate::compiler::ids::{DefId, FileId};
use crate::compiler::provenance::SourceRef;

#[salsa::tracked(returns(clone))]
pub(in crate::compiler::db) fn file_analysis_product(
    db: &dyn Db,
    facts: FileFacts<'_>,
) -> Arc<FileAnalysis> {
    db.query_telemetry().record_query(QueryKind::FileAnalysis);
    let functions = facts
        .functions(db)
        .into_iter()
        .map(|function| {
            FunctionDescriptor::new(facts.file(db), function.key(db), function.name(db))
        })
        .collect::<Vec<_>>();
    let constants = facts
        .constants(db)
        .into_iter()
        .map(|constant| {
            ConstantDescriptor::new(facts.file(db), constant.key(db), constant.name(db))
        })
        .collect::<Vec<_>>();
    let variables = facts
        .variables(db)
        .into_iter()
        .map(|variable| {
            VariableDescriptor::new(facts.file(db), variable.key(db), variable.name(db))
        })
        .collect::<Vec<_>>();
    let type_aliases = facts
        .type_aliases(db)
        .into_iter()
        .map(|alias| {
            let syntax = alias.syntax(db);
            let target = match &syntax.target.kind {
                crate::compiler::syntax::ExprSyntaxKind::Ident(target) => Arc::clone(&target.name),
                _ => Arc::from("<unsupported>"),
            };
            TypeAliasDescriptor::new(facts.file(db), alias.key(db), alias.name(db), target)
        })
        .collect::<Vec<_>>();
    let type_definitions = facts
        .type_definitions(db)
        .into_iter()
        .map(|definition| {
            let syntax = definition.syntax(db);
            let underlying = match &syntax.underlying.kind {
                crate::compiler::syntax::ExprSyntaxKind::Ident(underlying) => {
                    Arc::clone(&underlying.name)
                }
                _ => Arc::from("<unsupported>"),
            };
            TypeDefinitionDescriptor::new(
                facts.file(db),
                definition.key(db),
                definition.name(db),
                underlying,
            )
        })
        .collect::<Vec<_>>();
    Arc::new(FileAnalysis::new(super::super::model::FileAnalysisData {
        file: facts.file(db),
        package: facts.package(db),
        functions: functions.into(),
        constants: constants.into(),
        variables: variables.into(),
        type_aliases: type_aliases.into(),
        type_definitions: type_definitions.into(),
        failure: facts.failure(db),
        issues: facts.issues(db).into(),
    }))
}

#[salsa::tracked(returns(clone))]
pub(in crate::compiler::db) fn package_analysis_product(
    db: &dyn Db,
    input: PackageInput,
) -> Arc<PackageAnalysis> {
    db.query_telemetry()
        .record_query(QueryKind::PackageAnalysis);
    let package = input.package(db);
    let specialized_range_iterators = specialized_range_iterator_ids(db, input);
    let mut sources = input.sources(db).iter().copied().collect::<Vec<_>>();
    sources.sort_by_key(|source| source.file(db));

    let mut package_name: Option<Arc<str>> = None;
    let mut files = Vec::with_capacity(sources.len());
    let mut direct_imports = BTreeSet::new();
    let mut functions = Vec::new();
    let mut constants = Vec::new();
    let mut variables = Vec::new();
    let mut type_aliases = Vec::new();
    let mut type_definitions = Vec::new();
    let mut exported_signatures = Vec::new();
    let mut exported_constants = Vec::new();
    let mut exported_variables = Vec::new();
    let mut exported_type_aliases = Vec::new();
    let mut exported_type_definitions = Vec::new();
    let mut issues = Vec::new();
    let mut definitions_by_digest =
        BTreeMap::<DefId, (crate::compiler::ids::DefinitionKey, FileId)>::new();
    let mut declarations_by_name = BTreeMap::<Arc<str>, FileId>::new();
    let mut method_declarations = BTreeMap::<crate::compiler::ids::DefinitionKey, FileId>::new();

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
            match issue {
                FileIssue::DuplicateDefinition(name) => {
                    issues.push(PackageIssue::DuplicateDefinition {
                        name,
                        first_file: file,
                        second_file: file,
                    });
                }
                FileIssue::FunctionProjectionFailure { name, message } => {
                    issues.push(PackageIssue::FunctionProjectionFailure {
                        file,
                        name,
                        message,
                    });
                }
                FileIssue::ConstantProjectionFailure { name, message } => {
                    issues.push(PackageIssue::ConstantProjectionFailure {
                        file,
                        name,
                        message,
                    });
                }
                FileIssue::VariableProjectionFailure { name, message } => {
                    issues.push(PackageIssue::VariableProjectionFailure {
                        file,
                        name,
                        message,
                    });
                }
                FileIssue::TypeProjectionFailure { name, message } => {
                    issues.push(PackageIssue::TypeProjectionFailure {
                        file,
                        name,
                        message,
                    });
                }
            }
        }

        for function in facts.functions(db) {
            let id = function.id(db);
            let key = function.key(db);
            let name = function.name(db);

            let first_file = if key.is_package_level() {
                declarations_by_name.insert(Arc::clone(&name), file)
            } else {
                method_declarations.insert(key.clone(), file)
            };
            if let Some(first_file) = first_file {
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

            if function.receiver_type(db).is_none()
                && (name.as_ref() == "init" || specialized_range_iterators.contains(&id))
            {
                continue;
            }

            if is_exported(&name) {
                exported_signatures.push(signature_product(db, function).as_ref().clone());
            }
            functions.push(FunctionDescriptor::new(file, key, name));
        }

        for constant in facts.constants(db) {
            let id = constant.id(db);
            let key = constant.key(db);
            let name = constant.name(db);
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
            if is_exported(&name)
                && let Ok(typed) = typed_constant_product(db, input, constant)
            {
                let semantic = crate::compiler::hir::Constant {
                    id: typed.id,
                    name: typed.name.clone(),
                    ty: typed.ty.clone(),
                    value: typed.value.clone(),
                    source: SourceRef::definition(typed.id),
                };
                exported_constants.push((
                    typed.id,
                    crate::compiler::fingerprint::hir_constant(&semantic),
                ));
            }
            constants.push(ConstantDescriptor::new(file, key, name));
        }

        for variable in facts.variables(db) {
            let id = variable.id(db);
            let key = variable.key(db);
            let name = variable.name(db);
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
            if is_exported(&name)
                && let Ok(typed) = typed_variable_product(db, input, variable)
            {
                let semantic = crate::compiler::hir::Variable {
                    id: typed.id,
                    name: typed.name.clone(),
                    ty: typed.ty.clone(),
                    value: typed.value.clone(),
                    source: SourceRef::definition(typed.id),
                };
                exported_variables.push((
                    typed.id,
                    crate::compiler::fingerprint::hir_variable(&semantic),
                ));
            }
            variables.push(VariableDescriptor::new(file, key, name));
        }

        for alias in facts.type_aliases(db) {
            let id = alias.id(db);
            let key = alias.key(db);
            let name = alias.name(db);
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
                definitions_by_digest.insert(id, (key, file));
            }
            let syntax = alias.syntax(db);
            let target = match &syntax.target.kind {
                crate::compiler::syntax::ExprSyntaxKind::Ident(target) => Arc::clone(&target.name),
                _ => Arc::from("<unsupported>"),
            };
            type_aliases.push(TypeAliasDescriptor::new(file, alias.key(db), name, target));
        }

        for definition in facts.type_definitions(db) {
            let id = definition.id(db);
            let key = definition.key(db);
            let name = definition.name(db);
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
                definitions_by_digest.insert(id, (key, file));
            }
            let syntax = definition.syntax(db);
            let underlying = match &syntax.underlying.kind {
                crate::compiler::syntax::ExprSyntaxKind::Ident(underlying) => {
                    Arc::clone(&underlying.name)
                }
                _ => Arc::from("<unsupported>"),
            };
            type_definitions.push(TypeDefinitionDescriptor::new(
                file,
                definition.key(db),
                name,
                underlying,
            ));
        }
    }

    files.sort();
    functions.sort_by_key(|function| (function.id(), function.file()));
    constants.sort_by_key(|constant| (constant.id(), constant.file()));
    variables.sort_by_key(|variable| (variable.id(), variable.file()));
    type_aliases.sort_by_key(|alias| (alias.id(), alias.file()));
    type_definitions.sort_by_key(|definition| (definition.id(), definition.file()));
    exported_signatures.sort_by_key(FunctionSignature::id);
    exported_constants.sort_by_key(|(definition, _)| *definition);
    exported_variables.sort_by_key(|(definition, _)| *definition);
    if let Ok(resolved) = package_type_aliases_product(db, input) {
        for alias in &type_aliases {
            if is_exported(alias.name())
                && let Some(ty) = resolved.get(alias.name())
            {
                exported_type_aliases.push((alias.id(), crate::compiler::fingerprint::go_type(ty)));
            }
        }
        for definition in &type_definitions {
            if is_exported(definition.name())
                && let Some(ty) = resolved.get(definition.name())
            {
                exported_type_definitions
                    .push((definition.id(), crate::compiler::fingerprint::go_type(ty)));
            }
        }
    }
    exported_type_aliases.sort_by_key(|(definition, _)| *definition);
    exported_type_definitions.sort_by_key(|(definition, _)| *definition);
    issues.sort();
    Arc::new(PackageAnalysis::new(
        super::super::model::PackageAnalysisData {
            package,
            package_name: package_name.unwrap_or_else(|| Arc::from("")),
            files: files.into(),
            direct_imports: direct_imports.into_iter().collect::<Vec<_>>().into(),
            functions: functions.into(),
            constants: constants.into(),
            variables: variables.into(),
            type_aliases: type_aliases.into(),
            type_definitions: type_definitions.into(),
            issues: issues.into(),
        },
        &exported_signatures,
        &exported_constants,
        &exported_variables,
        &exported_type_aliases,
        &exported_type_definitions,
    ))
}

#[salsa::tracked(returns(clone))]
pub(in crate::compiler::db) fn signature_product(
    db: &dyn Db,
    function: FunctionProjection<'_>,
) -> Arc<FunctionSignature> {
    db.query_telemetry()
        .record_query(QueryKind::FunctionSignature);
    function.signature(db)
}

#[salsa::tracked(returns(clone))]
pub(in crate::compiler::db) fn body_product(
    db: &dyn Db,
    function: FunctionProjection<'_>,
) -> Arc<FunctionBody> {
    db.query_telemetry().record_query(QueryKind::FunctionBody);
    function.body(db)
}

#[salsa::tracked(returns(clone))]
pub(in crate::compiler::db) fn public_api_product(
    db: &dyn Db,
    facts: FileFacts<'_>,
) -> Arc<PublicApi> {
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
