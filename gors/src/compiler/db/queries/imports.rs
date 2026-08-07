//! Typed function-scope symbols from resolver-owned import bindings.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use super::support::{
    PackageReferences, function_source, semantic_failure, specialized_range_iterator_ids,
};
use super::{
    Db, FunctionProjection, PackageInput, file_projection, package_constant_named_product,
    package_function_named_product, package_function_product, package_type_aliases_product,
    package_variable_named_product, typed_constant_product, typed_signature_product,
    typed_variable_product,
};
use crate::compiler::Diagnostic;
use crate::compiler::db::ResolvedImportBinding;
use crate::compiler::db::products::StageFailure;
use crate::compiler::ids::{DefId, PackageId, QualifiedDefId};
use crate::compiler::provenance::SourceRef;
use crate::compiler::semantic::{
    ConstantSymbol, FunctionSymbol, FunctionSymbols, GenericFunctionSymbol, GenericTypeSymbol,
    MethodSymbol, VariableSymbol,
};
use crate::compiler::syntax::{FieldListSyntax, function_is_generic};
use crate::compiler::types::{Signature, Ty};

pub(super) fn function_dependency<'db>(
    db: &'db dyn Db,
    input: PackageInput,
    function: FunctionProjection<'db>,
    target: QualifiedDefId,
) -> Result<Option<(PackageInput, FunctionProjection<'db>)>, Arc<StageFailure>> {
    if target.package() == input.package(db) {
        return Ok(package_function_product(db, input, target.definition())
            .map(|projection| (input, projection)));
    }
    let source = function_source(db, input, function)?;
    let resolved = source.resolved_imports(db);
    let target_input = resolved
        .targets(db)
        .iter()
        .find_map(|(package, input)| (*package == target.package()).then_some(*input));
    Ok(target_input.and_then(|target_input| {
        package_function_product(db, target_input, target.definition())
            .map(|projection| (target_input, projection))
    }))
}

pub(super) fn function_symbols(
    db: &dyn Db,
    input: PackageInput,
    function: FunctionProjection<'_>,
    references: &PackageReferences,
    signature: &Signature,
) -> Result<FunctionSymbols, Arc<StageFailure>> {
    let definition = function.id(db);
    let mut symbols = FunctionSymbols {
        functions: BTreeMap::new(),
        qualified_functions: BTreeMap::new(),
        methods: BTreeMap::new(),
        generic_functions: BTreeMap::new(),
        generic_methods: BTreeMap::new(),
        generic_types: collect_generic_type_symbols(db, input),
        constants: BTreeMap::new(),
        qualified_constants: BTreeMap::new(),
        variables: BTreeMap::new(),
        qualified_variables: BTreeMap::new(),
        intrinsic_packages: BTreeSet::new(),
    };
    let local_range_iterators = specialized_range_iterator_ids(db, input);
    add_unqualified_symbols(
        db,
        input,
        input.package(db),
        &references.unqualified,
        &references.range_functions,
        &local_range_iterators,
        definition,
        &mut symbols,
        false,
    )?;
    let types = package_type_aliases_product(db, input)?;
    let mut method_names = references.method_names.clone();
    for ty in signature.params.iter().chain(&signature.results) {
        collect_interface_method_names(ty, &mut method_names);
    }
    for ty in types.values() {
        collect_interface_method_names(ty, &mut method_names);
    }
    for name in &references.unqualified {
        if name.as_ref() == "error" {
            method_names.insert(Arc::from("Error"));
        }
    }
    add_method_symbols(db, input, &method_names, definition, &mut symbols)?;

    let source = function_source(db, input, function)?;
    let resolved_input = source.resolved_imports(db);
    let resolved = resolved_input.value(db);
    let targets = resolved_input
        .targets(db)
        .iter()
        .copied()
        .collect::<BTreeMap<_, _>>();
    let mut ordinary_bindings = BTreeMap::<String, PackageId>::new();

    for import in resolved.imports() {
        db.unwind_if_revision_cancelled();
        let target = targets
            .get(&import.target_package())
            .copied()
            .ok_or_else(|| {
                semantic_failure(
                    definition,
                    Diagnostic::backend(format!(
                        "resolved import {} has no tracked package input",
                        import.canonical_path()
                    )),
                )
            })?;
        match import.binding() {
            ResolvedImportBinding::Blank { .. } => {}
            ResolvedImportBinding::Dot { .. } => {
                let range_iterators = specialized_range_iterator_ids(db, target);
                add_unqualified_symbols(
                    db,
                    target,
                    import.target_package(),
                    &references.unqualified,
                    &references.range_functions,
                    &range_iterators,
                    definition,
                    &mut symbols,
                    true,
                )?;
            }
            ResolvedImportBinding::Default { local_name, .. }
            | ResolvedImportBinding::Named { local_name, .. } => {
                let local_name = local_name.to_string();
                if let Some(previous) =
                    ordinary_bindings.insert(local_name.clone(), import.target_package())
                    && previous != import.target_package()
                {
                    return Err(semantic_failure(
                        definition,
                        Diagnostic::semantic(
                            format!("import name {local_name} is declared more than once"),
                            SourceRef::definition(definition),
                        ),
                    ));
                }
                if import.canonical_path().as_str() == "unsafe" {
                    symbols.intrinsic_packages.insert(local_name);
                    continue;
                }
                for (base, member) in references
                    .qualified
                    .iter()
                    .filter(|(base, _)| base.as_ref() == local_name)
                {
                    add_qualified_symbol(
                        db,
                        target,
                        import.target_package(),
                        base,
                        member,
                        definition,
                        &mut symbols,
                    )?;
                }
            }
        }
    }

    Ok(symbols)
}

fn collect_generic_type_symbols(
    db: &dyn Db,
    input: PackageInput,
) -> BTreeMap<String, GenericTypeSymbol> {
    let mut result = BTreeMap::new();
    let mut sources = input.sources(db).iter().copied().collect::<Vec<_>>();
    sources.sort_by_key(|source| source.file(db));
    for source in sources {
        for alias in file_projection(db, source).type_aliases(db) {
            let syntax = alias.syntax(db);
            let Some(type_parameters) = &syntax.type_parameters else {
                continue;
            };
            result.insert(
                alias.name(db).to_string(),
                GenericTypeSymbol {
                    id: alias.id(db),
                    type_parameters: Arc::new(type_parameters.clone()),
                    underlying: syntax.target.clone(),
                    alias: true,
                },
            );
        }
        for definition in file_projection(db, source).type_definitions(db) {
            let syntax = definition.syntax(db);
            result.insert(
                definition.name(db).to_string(),
                GenericTypeSymbol {
                    id: definition.id(db),
                    type_parameters: Arc::new(syntax.type_parameters.clone().unwrap_or_else(
                        || FieldListSyntax {
                            fields: Arc::from([]),
                        },
                    )),
                    underlying: syntax.underlying.clone(),
                    alias: false,
                },
            );
        }
    }
    result
}

fn collect_interface_method_names(ty: &Ty, names: &mut BTreeSet<Arc<str>>) {
    match ty {
        Ty::Named { underlying, .. }
        | Ty::LocalNamed { underlying, .. }
        | Ty::Pointer(underlying)
        | Ty::Slice(underlying) => {
            collect_interface_method_names(underlying, names);
        }
        Ty::Array(_, element) | Ty::Channel(_, element) => {
            collect_interface_method_names(element, names);
        }
        Ty::Map(key, value) => {
            collect_interface_method_names(key, names);
            collect_interface_method_names(value, names);
        }
        Ty::Struct(fields) => {
            for field in fields {
                collect_interface_method_names(&field.ty, names);
            }
        }
        Ty::Interface(methods) => {
            for method in methods {
                names.insert(Arc::from(method.name.as_str()));
                for ty in method
                    .signature
                    .params
                    .iter()
                    .chain(&method.signature.results)
                {
                    collect_interface_method_names(ty, names);
                }
            }
        }
        Ty::Function(signature) => {
            for ty in signature.params.iter().chain(&signature.results) {
                collect_interface_method_names(ty, names);
            }
        }
        Ty::Tuple(elements) => {
            for element in elements {
                collect_interface_method_names(element, names);
            }
        }
        Ty::Unit
        | Ty::Bool
        | Ty::Int(_)
        | Ty::Uint(_)
        | Ty::Float(_)
        | Ty::Complex(_)
        | Ty::NamedRef { .. }
        | Ty::String
        | Ty::Untyped(_) => {}
    }
}

fn add_method_symbols(
    db: &dyn Db,
    input: PackageInput,
    names: &BTreeSet<Arc<str>>,
    caller: crate::compiler::ids::DefId,
    symbols: &mut FunctionSymbols,
) -> Result<(), Arc<StageFailure>> {
    if names.is_empty() {
        return Ok(());
    }
    let types = package_type_aliases_product(db, input)?;
    let mut sources = input.sources(db).iter().copied().collect::<Vec<_>>();
    sources.sort_by_key(|source| source.file(db));
    for source in sources {
        for method in file_projection(db, source).functions(db) {
            let name = method.name(db);
            let Some(receiver) = method.receiver_type(db) else {
                continue;
            };
            if !names.contains(&name) {
                continue;
            }
            let signature_syntax = method.signature(db);
            let header = signature_syntax.structure();
            if function_is_generic(header) {
                let generic_type =
                    symbols
                        .generic_types
                        .get(receiver.as_ref())
                        .ok_or_else(|| {
                            semantic_failure(
                                caller,
                                Diagnostic::semantic(
                                    format!(
                                        "method receiver type {receiver} is not a defined type"
                                    ),
                                    SourceRef::definition(caller),
                                ),
                            )
                        })?;
                if generic_type.alias {
                    return Err(semantic_failure(
                        caller,
                        Diagnostic::semantic(
                            format!(
                                "method receiver base {receiver} must be a defined type, not an alias"
                            ),
                            SourceRef::definition(caller),
                        ),
                    ));
                }
                symbols.generic_methods.insert(
                    (generic_type.id, name.to_string()),
                    GenericFunctionSymbol {
                        id: QualifiedDefId::new(input.package(db), method.id(db)),
                        header: Arc::new(header.clone()),
                        body: Arc::new(method.body(db).structure().clone()),
                        pointer_receiver: method.pointer_receiver(db),
                    },
                );
                continue;
            }
            let Some(Ty::Named { definition, .. }) = types.get(receiver.as_ref()) else {
                return Err(semantic_failure(
                    caller,
                    Diagnostic::semantic(
                        format!("method receiver type {receiver} is not a defined type"),
                        SourceRef::definition(caller),
                    ),
                ));
            };
            let signature = typed_signature_product(db, input, method)?;
            let key = (*definition, name.to_string());
            let symbol = MethodSymbol {
                id: QualifiedDefId::new(input.package(db), method.id(db)),
                signature: signature.signature().clone(),
                pointer_receiver: method.pointer_receiver(db),
            };
            if symbols
                .methods
                .insert(key, symbol.clone())
                .is_some_and(|previous| previous.id != symbol.id)
            {
                return Err(semantic_failure(
                    caller,
                    Diagnostic::backend("ambiguous receiver-qualified method identity"),
                ));
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn add_unqualified_symbols(
    db: &dyn Db,
    input: PackageInput,
    package: PackageId,
    names: &BTreeSet<Arc<str>>,
    range_names: &BTreeSet<Arc<str>>,
    range_iterators: &BTreeSet<DefId>,
    caller: crate::compiler::ids::DefId,
    symbols: &mut FunctionSymbols,
    imported: bool,
) -> Result<(), Arc<StageFailure>> {
    for name in names {
        if imported && !is_exported(name) {
            continue;
        }
        if let Some(projection) = package_function_named_product(db, input, Arc::clone(name)) {
            let signature_syntax = projection.signature(db);
            let header = signature_syntax.structure();
            if function_is_generic(header) {
                symbols.generic_functions.insert(
                    name.to_string(),
                    GenericFunctionSymbol {
                        id: QualifiedDefId::new(package, projection.id(db)),
                        header: Arc::new(header.clone()),
                        body: Arc::new(projection.body(db).structure().clone()),
                        pointer_receiver: false,
                    },
                );
            } else {
                let typed = typed_signature_product(db, input, projection)?;
                insert_function(
                    &mut symbols.functions,
                    name.to_string(),
                    FunctionSymbol {
                        id: QualifiedDefId::new(package, projection.id(db)),
                        signature: typed.signature().clone(),
                        range_header: (range_names.contains(name)
                            && range_iterators.contains(&projection.id(db)))
                        .then(|| Arc::new(header.clone())),
                        range_body: (range_names.contains(name)
                            && range_iterators.contains(&projection.id(db)))
                        .then(|| Arc::new(projection.body(db).structure().clone())),
                    },
                    caller,
                )?;
            }
        }
        if let Some(projection) = package_constant_named_product(db, input, Arc::clone(name)) {
            let typed = typed_constant_product(db, input, projection)?;
            insert_constant(
                &mut symbols.constants,
                name.to_string(),
                ConstantSymbol {
                    id: QualifiedDefId::new(package, typed.id),
                    ty: typed.ty.clone(),
                    value: typed.value.clone(),
                },
                caller,
            )?;
        }
        if let Some(projection) = package_variable_named_product(db, input, Arc::clone(name)) {
            let typed = typed_variable_product(db, input, projection)?;
            insert_variable(
                &mut symbols.variables,
                name.to_string(),
                VariableSymbol {
                    id: QualifiedDefId::new(package, typed.id),
                    ty: typed.ty.clone(),
                    value: typed.value.clone(),
                },
                caller,
            )?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn add_qualified_symbol(
    db: &dyn Db,
    input: PackageInput,
    package: PackageId,
    base: &Arc<str>,
    member: &Arc<str>,
    caller: crate::compiler::ids::DefId,
    symbols: &mut FunctionSymbols,
) -> Result<(), Arc<StageFailure>> {
    if !is_exported(member) {
        return Err(semantic_failure(
            caller,
            Diagnostic::semantic(
                format!("cannot refer to unexported name {base}.{member}"),
                SourceRef::definition(caller),
            ),
        ));
    }
    let key = (base.to_string(), member.to_string());
    if let Some(projection) = package_function_named_product(db, input, Arc::clone(member)) {
        let typed = typed_signature_product(db, input, projection)?;
        symbols.qualified_functions.insert(
            key,
            FunctionSymbol {
                id: QualifiedDefId::new(package, projection.id(db)),
                signature: typed.signature().clone(),
                range_header: None,
                range_body: None,
            },
        );
    } else if let Some(projection) = package_constant_named_product(db, input, Arc::clone(member)) {
        let typed = typed_constant_product(db, input, projection)?;
        symbols.qualified_constants.insert(
            key,
            ConstantSymbol {
                id: QualifiedDefId::new(package, typed.id),
                ty: typed.ty.clone(),
                value: typed.value.clone(),
            },
        );
    } else if let Some(projection) = package_variable_named_product(db, input, Arc::clone(member)) {
        let typed = typed_variable_product(db, input, projection)?;
        symbols.qualified_variables.insert(
            key,
            VariableSymbol {
                id: QualifiedDefId::new(package, typed.id),
                ty: typed.ty.clone(),
                value: typed.value.clone(),
            },
        );
    }
    Ok(())
}

fn insert_function(
    functions: &mut BTreeMap<String, FunctionSymbol>,
    name: String,
    symbol: FunctionSymbol,
    caller: crate::compiler::ids::DefId,
) -> Result<(), Arc<StageFailure>> {
    if functions
        .insert(name.clone(), symbol.clone())
        .is_some_and(|previous| previous.id != symbol.id)
    {
        return Err(ambiguous_name(caller, &name));
    }
    Ok(())
}

fn insert_constant(
    constants: &mut BTreeMap<String, ConstantSymbol>,
    name: String,
    symbol: ConstantSymbol,
    caller: crate::compiler::ids::DefId,
) -> Result<(), Arc<StageFailure>> {
    if constants
        .insert(name.clone(), symbol.clone())
        .is_some_and(|previous| previous.id != symbol.id)
    {
        return Err(ambiguous_name(caller, &name));
    }
    Ok(())
}

fn insert_variable(
    variables: &mut BTreeMap<String, VariableSymbol>,
    name: String,
    symbol: VariableSymbol,
    caller: crate::compiler::ids::DefId,
) -> Result<(), Arc<StageFailure>> {
    if variables
        .insert(name.clone(), symbol.clone())
        .is_some_and(|previous| previous.id != symbol.id)
    {
        return Err(ambiguous_name(caller, &name));
    }
    Ok(())
}

fn ambiguous_name(caller: crate::compiler::ids::DefId, name: &str) -> Arc<StageFailure> {
    semantic_failure(
        caller,
        Diagnostic::semantic(
            format!("package name {name} is ambiguous across dot imports"),
            SourceRef::definition(caller),
        ),
    )
}

fn is_exported(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_uppercase)
}
