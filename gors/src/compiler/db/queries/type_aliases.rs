//! Package-wide type-alias resolution over owned declaration projections.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use super::support::collect_all_expression_names;
use super::{
    ConstantProjection, Db, PackageInput, file_projection, semantic_failure, typed_constant_product,
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
    for source in sources {
        let facts = file_projection(db, source);
        for alias in facts.type_aliases(db) {
            if alias.syntax(db).type_parameters.is_none() {
                projections.insert(alias.name(db).to_string(), alias);
            }
        }
        for definition in facts.type_definitions(db) {
            let syntax = definition.syntax(db);
            if syntax.type_parameters.is_none() && !is_constraint_type(&syntax.underlying) {
                definitions.insert(definition.name(db).to_string(), definition);
            }
        }
        for constant in facts.constants(db) {
            if constant.name(db).as_ref() != "_" {
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
    };
    for name in projections.keys().chain(definitions.keys()) {
        resolve_type_name(&context, name, &mut resolved, &mut stack, false)?;
    }
    Ok(Arc::new(resolved))
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
}

fn resolve_type_name(
    context: &TypeResolutionContext<'_, '_>,
    name: &str,
    resolved: &mut BTreeMap<String, Ty>,
    stack: &mut Vec<(String, bool)>,
    incoming_guarded: bool,
) -> Result<Ty, Arc<StageFailure>> {
    let declaration = context
        .projections
        .get(name)
        .copied()
        .map(|alias| {
            (
                alias.id(context.db),
                alias.syntax(context.db).target.clone(),
                false,
            )
        })
        .or_else(|| {
            context.definitions.get(name).copied().map(|defined| {
                (
                    defined.id(context.db),
                    defined.syntax(context.db).underlying.clone(),
                    true,
                )
            })
        })
        .ok_or_else(|| {
            Arc::new(StageFailure::one(
                CompilerStage::Semantic,
                Diagnostic::backend(format!("missing projected type declaration {name}")),
            ))
        })?;
    let (definition, expression, is_definition) = declaration;
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

    stack.push((name.to_owned(), incoming_guarded));
    let mut dependencies = BTreeMap::new();
    collect_type_dependencies(&expression, &mut dependencies, false);
    for (dependency, guarded) in dependencies {
        if context.projections.contains_key(&dependency)
            || context.definitions.contains_key(&dependency)
        {
            let target_ty = resolve_type_name(context, &dependency, resolved, stack, guarded)?;
            resolved.insert(dependency, target_ty);
        }
    }
    let mut referenced_names = BTreeSet::new();
    collect_all_expression_names(&expression, &mut referenced_names);
    let mut constant_symbols = BTreeMap::new();
    for name in referenced_names {
        let Some(constant) = context.constants.get(&name).copied() else {
            continue;
        };
        let typed = typed_constant_product(context.db, context.input, constant)?;
        constant_symbols.insert(
            typed.name.clone(),
            ConstantSymbol {
                id: QualifiedDefId::new(context.input.package(context.db), typed.id),
                ty: typed.ty.clone(),
                value: typed.value.clone(),
            },
        );
    }
    let target = lower_declaration_target(&expression, resolved, &constant_symbols, definition)?;
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
    Ok(ty)
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
    definition: DefId,
) -> Result<Ty, Arc<StageFailure>> {
    crate::compiler::semantic::lower_type_with_constants(
        expression,
        resolved,
        constants,
        SourceRef::definition(definition),
    )
    .map_err(|diagnostic| semantic_failure(definition, diagnostic))
}
