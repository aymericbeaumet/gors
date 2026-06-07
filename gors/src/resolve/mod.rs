//! Go package resolver.
//!
//! This module resolves import paths to Go source packages, currently backed by
//! build-time generated metadata from the embedded Go SDK.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::{Arc, OnceLock, RwLock};

mod runtime_primitives;
mod structural_helpers;

#[derive(Clone, Copy)]
struct EmbeddedGoFile {
    filename: &'static str,
    content: &'static str,
}

#[derive(Clone, Copy)]
struct EmbeddedGoPackage {
    import_path: &'static str,
    files: &'static [EmbeddedGoFile],
    direct_imports: &'static [&'static str],
}

include!(concat!(env!("OUT_DIR"), "/go_stdlib.rs"));

type PackageFiles = Vec<(&'static str, &'static str)>;
type TypeEnv = crate::compiler::typeinfer::TypeEnv;
type TypeEnvCell = Arc<OnceLock<Option<(String, TypeEnv)>>>;
type PackageFilesCache = HashMap<String, Arc<OnceLock<Option<Arc<PackageFiles>>>>>;
type TypeEnvCache = HashMap<String, TypeEnvCell>;
type TransitiveImportsCache = HashMap<String, Arc<OnceLock<Vec<String>>>>;
type ResolvedModuleCache = HashMap<String, Arc<RwLock<ResolvedModuleEntry>>>;

enum ResolvedModuleEntry {
    Vacant,
    Missing,
    Source(String),
    Uncacheable,
}

static PACKAGE_FILES: OnceLock<RwLock<PackageFilesCache>> = OnceLock::new();
static TYPE_ENVS: OnceLock<RwLock<TypeEnvCache>> = OnceLock::new();
static TRANSITIVE_IMPORTS: OnceLock<RwLock<TransitiveImportsCache>> = OnceLock::new();
static RESOLVED_MODULES: OnceLock<RwLock<ResolvedModuleCache>> = OnceLock::new();
static RESOLVED_IMPORTS: OnceLock<RwLock<HashMap<String, Vec<String>>>> = OnceLock::new();

fn package_file_cache() -> &'static RwLock<PackageFilesCache> {
    PACKAGE_FILES.get_or_init(|| RwLock::new(HashMap::new()))
}

fn load_package_files(import_path: &str) -> Option<Arc<PackageFiles>> {
    let package = embedded_package(import_path)?;
    if package.files.is_empty() {
        return None;
    }
    Some(Arc::new(
        package
            .files
            .iter()
            .map(|file| (file.filename, file.content))
            .collect(),
    ))
}

fn type_envs() -> &'static RwLock<TypeEnvCache> {
    TYPE_ENVS.get_or_init(|| RwLock::new(HashMap::new()))
}

fn transitive_imports() -> &'static RwLock<TransitiveImportsCache> {
    TRANSITIVE_IMPORTS.get_or_init(|| RwLock::new(HashMap::new()))
}

fn resolved_modules() -> &'static RwLock<ResolvedModuleCache> {
    RESOLVED_MODULES.get_or_init(|| RwLock::new(HashMap::new()))
}

fn resolved_imports() -> &'static RwLock<HashMap<String, Vec<String>>> {
    RESOLVED_IMPORTS.get_or_init(|| RwLock::new(HashMap::new()))
}

fn embedded_package(import_path: &str) -> Option<&'static EmbeddedGoPackage> {
    EMBEDDED_PACKAGES
        .binary_search_by(|package| package.import_path.cmp(import_path))
        .ok()
        .and_then(|idx| EMBEDDED_PACKAGES.get(idx))
}

pub fn is_known(import_path: &str) -> bool {
    package_exists(import_path)
}

pub fn package_exists(import_path: &str) -> bool {
    embedded_package(import_path).is_some()
}

pub fn package_files(import_path: &str) -> Option<Arc<PackageFiles>> {
    if !package_exists(import_path) {
        return None;
    }

    let Some(cell) = package_file_cell(import_path) else {
        return load_package_files(import_path);
    };
    cell.get_or_init(|| load_package_files(import_path)).clone()
}

fn package_file_cell(import_path: &str) -> Option<Arc<OnceLock<Option<Arc<PackageFiles>>>>> {
    if let Ok(cache) = package_file_cache().read()
        && let Some(cell) = cache.get(import_path)
    {
        return Some(cell.clone());
    }

    let Ok(mut cache) = package_file_cache().write() else {
        return None;
    };
    Some(
        cache
            .entry(import_path.to_string())
            .or_insert_with(|| Arc::new(OnceLock::new()))
            .clone(),
    )
}

pub fn list_packages() -> Vec<String> {
    EMBEDDED_PACKAGES
        .iter()
        .map(|package| package.import_path.to_string())
        .collect()
}

pub fn module_name(import_path: &str) -> String {
    let mut out = String::new();
    for ch in import_path.chars() {
        match ch {
            '/' => out.push_str("__"),
            ch if ch.is_ascii_alphanumeric() || ch == '_' => out.push(ch),
            _ => out.push('_'),
        }
    }

    if out.is_empty() {
        out.push('_');
    }

    if out.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        out.insert(0, '_');
    }

    if is_rust_keyword(&out) {
        out.push('_');
    }

    out
}

pub fn resolve(import_path: &str) -> Option<syn::ItemMod> {
    resolve_cached(import_path, None)
}

pub fn resolve_with_roots(import_path: &str, roots: &HashSet<String>) -> Option<syn::ItemMod> {
    if roots.is_empty() {
        return None;
    }
    resolve_cached(import_path, Some(roots))
}

fn resolve_cached(import_path: &str, roots: Option<&HashSet<String>>) -> Option<syn::ItemMod> {
    if crate::compiler::has_external_interface_implementors() {
        return resolve_uncached(import_path, roots);
    }

    let cache_key = resolve_cache_key(import_path, roots);
    let Some(cell) = resolved_module_cell(&cache_key) else {
        return resolve_uncached(import_path, roots);
    };

    // Cold stdlib roots are expensive enough that blocking every worker behind
    // one cache initializer leaves the generated-program harness mostly idle.
    if let Ok(entry) = cell.try_read() {
        match &*entry {
            ResolvedModuleEntry::Missing => return None,
            ResolvedModuleEntry::Source(source) => {
                if let Some(module) = parse_cached_module(import_path, source) {
                    return Some(module);
                }
            }
            ResolvedModuleEntry::Vacant | ResolvedModuleEntry::Uncacheable => {}
        }
    } else {
        return resolve_uncached(import_path, roots);
    }

    let Ok(mut entry) = cell.try_write() else {
        return resolve_uncached(import_path, roots);
    };
    match &*entry {
        ResolvedModuleEntry::Missing => return None,
        ResolvedModuleEntry::Source(source) => {
            if let Some(module) = parse_cached_module(import_path, source) {
                return Some(module);
            }
        }
        ResolvedModuleEntry::Vacant | ResolvedModuleEntry::Uncacheable => {}
    }

    let resolved = resolve_uncached(import_path, roots);
    match &resolved {
        None => {
            *entry = ResolvedModuleEntry::Missing;
        }
        Some(module) => {
            let source = module_content_cache_source(module);
            if parse_cached_module(import_path, &source).is_some() {
                *entry = ResolvedModuleEntry::Source(source);
            } else {
                *entry = ResolvedModuleEntry::Uncacheable;
            }
        }
    }
    resolved
}

fn resolved_module_cell(cache_key: &str) -> Option<Arc<RwLock<ResolvedModuleEntry>>> {
    if let Ok(cache) = resolved_modules().read()
        && let Some(cell) = cache.get(cache_key)
    {
        return Some(cell.clone());
    }

    let Ok(mut cache) = resolved_modules().write() else {
        return None;
    };
    Some(
        cache
            .entry(cache_key.to_string())
            .or_insert_with(|| Arc::new(RwLock::new(ResolvedModuleEntry::Vacant)))
            .clone(),
    )
}

fn parse_cached_module(import_path: &str, source: &str) -> Option<syn::ItemMod> {
    syn::parse_str::<syn::File>(source)
        .inspect_err(|error| {
            log_skip(format_args!(
                "[gors] skip resolved module cache for {import_path}: {error}"
            ));
        })
        .ok()
        .map(|file| item_mod_for(import_path, file.items))
}

fn module_content_cache_source(module: &syn::ItemMod) -> String {
    let items = module
        .content
        .as_ref()
        .map(|(_, items)| items.clone())
        .unwrap_or_default();
    prettyplease::unparse(&syn::File {
        shebang: None,
        attrs: vec![],
        items,
    })
}

fn resolve_cache_key(import_path: &str, roots: Option<&HashSet<String>>) -> String {
    let Some(roots) = roots else {
        return import_path.to_string();
    };
    let mut roots: Vec<_> = roots.iter().map(String::as_str).collect();
    roots.sort_unstable();
    format!("{import_path}\0{}", roots.join(","))
}

fn resolve_uncached(import_path: &str, roots: Option<&HashSet<String>>) -> Option<syn::ItemMod> {
    if let Some(module) = runtime_primitives::module(import_path, roots) {
        cache_resolved_imports(import_path, roots, Vec::new());
        return Some(module);
    }

    let files = package_files(import_path)?;

    let mut parsed_files = Vec::new();
    for (filename, content) in files.iter() {
        let ast = match crate::parser::parse_file(filename, content) {
            Ok(ast) => ast,
            Err(e) => {
                log_skip(format_args!(
                    "[gors] skip {import_path}/{filename}: parse error: {e}"
                ));
                continue;
            }
        };
        parsed_files.push((*filename, ast));
    }

    let reachable_names = roots.map(|roots| reachable_package_names(&parsed_files, roots));
    if roots.is_some() && reachable_names.as_ref().is_none_or(HashSet::is_empty) {
        cache_resolved_imports(import_path, roots, Vec::new());
        return None;
    }

    let parsed_files: Vec<_> = parsed_files
        .into_iter()
        .filter_map(|(filename, ast)| {
            let filtered = match &reachable_names {
                Some(reachable) => filter_file_to_reachable(ast, reachable),
                None => ast,
            };
            file_has_compilable_decl(&filtered).then_some((filename, filtered))
        })
        .collect();

    if parsed_files.is_empty() {
        cache_resolved_imports(import_path, roots, Vec::new());
        return None;
    }

    let package_mutable_top_level_vars = crate::compiler::mutable_top_level_var_names_for_files(
        parsed_files.iter().map(|(_, ast)| ast),
        false,
    );
    let mut package_type_env = TypeEnv::new();
    let parsed_file_refs = parsed_files.iter().map(|(_, ast)| ast).collect::<Vec<_>>();
    package_type_env.scan_files(&parsed_file_refs);
    runtime_primitives::supplement_type_env(import_path, &mut package_type_env);
    let imported_type_envs = scan_imported_type_envs(import_path, &parsed_file_refs);
    refresh_borrowed_slice_params_with_imports(
        &mut package_type_env,
        &parsed_file_refs,
        &imported_type_envs,
    );
    refresh_top_level_vars_with_imports(
        &mut package_type_env,
        &parsed_file_refs,
        &imported_type_envs,
    );
    let view_method_seed = crate::compiler::fixed_array_view_method_seed_for_files(
        &parsed_file_refs,
        &package_type_env,
    );

    let import_renames = package_import_renames(&parsed_files);
    let import_path_by_module = package_import_path_by_module(&parsed_files);
    let mut all_items: Vec<syn::Item> = Vec::new();
    let recovery_context = ResolvedRecoveryContext {
        import_path,
        package_type_env: &package_type_env,
        imported_type_envs: &imported_type_envs,
        import_renames: &import_renames,
        package_mutable_top_level_vars: &package_mutable_top_level_vars,
        view_method_seed: &view_method_seed,
        roots,
    };

    for (filename, ast) in parsed_files {
        let compiled = match compile_resolved_file(
            ast,
            &package_type_env,
            &imported_type_envs,
            &import_renames,
            &package_mutable_top_level_vars,
            &view_method_seed,
            roots,
        ) {
            Ok(compiled) => compiled,
            Err(e) => {
                log_skip(format_args!(
                    "[gors] skip {import_path}/{filename}: compile error: {e}"
                ));
                if let Some(content) = files
                    .iter()
                    .find_map(|(file, content)| (*file == filename).then_some(*content))
                {
                    all_items.extend(recover_resolved_file_items(
                        filename,
                        content,
                        reachable_names.as_ref(),
                        &recovery_context,
                    ));
                }
                continue;
            }
        };
        all_items.extend(compiled.items);
    }
    runtime_primitives::supplement_items(import_path, roots, &mut all_items);
    crate::compiler::merge_package_init_items(&mut all_items);

    if all_items.is_empty() {
        cache_resolved_imports(import_path, roots, Vec::new());
        return None;
    }

    let mut merged_file = syn::File {
        shebang: None,
        attrs: vec![],
        items: all_items,
    };
    crate::compiler::passes::pass_after_package_merge(&mut merged_file);
    crate::compiler::add_post_merge_interface_helpers(&mut merged_file);
    let mut all_items = merged_file.items;

    dedupe_use_items(&mut all_items);
    let used_imports = used_imports_from_items(&mut all_items, &import_path_by_module);
    cache_resolved_imports(import_path, roots, used_imports.clone());
    let module_refs: HashSet<String> = used_imports.iter().map(|path| module_name(path)).collect();
    structural_helpers::inject(&mut all_items);
    let mut merged_file = syn::File {
        shebang: None,
        attrs: vec![],
        items: all_items,
    };
    crate::compiler::passes::pass_after_structural_helpers(&mut merged_file);
    let mut all_items = merged_file.items;
    prefix_crate_paths(&mut all_items, &module_refs);

    Some(item_mod_for(import_path, all_items))
}

fn compile_resolved_file(
    ast: crate::ast::File<'_>,
    package_type_env: &TypeEnv,
    imported_type_envs: &BTreeMap<String, crate::compiler::PackageFacts>,
    import_renames: &BTreeMap<String, String>,
    package_mutable_top_level_vars: &HashSet<String>,
    view_method_seed: &crate::compiler::FixedArrayViewMethodSeed,
    roots: Option<&HashSet<String>>,
) -> Result<syn::File, crate::compiler::CompilerError> {
    let mut type_env = package_type_env.clone();
    crate::compiler::merge_import_type_envs(
        &mut type_env,
        &ast,
        &BTreeMap::new(),
        imported_type_envs,
    );
    crate::compiler::with_active_reachability_roots(roots, || {
        crate::compiler::compile_with_type_env_import_renames_mutable_vars_and_view_seed(
            ast,
            type_env,
            import_renames.clone(),
            Some(package_mutable_top_level_vars.clone()),
            Some(view_method_seed),
        )
    })
}

fn scan_imported_type_envs(
    import_path: &str,
    files: &[&crate::ast::File<'_>],
) -> BTreeMap<String, crate::compiler::PackageFacts> {
    let mut imported_type_envs: BTreeMap<String, crate::compiler::PackageFacts> = BTreeMap::new();
    for ast in files {
        for import in ast.imports() {
            let imported_path = import.path.value.trim_matches('"');
            if imported_path == import_path {
                continue;
            }
            if let Some((package_name, env)) = scan_type_env(imported_path) {
                imported_type_envs.insert(
                    imported_path.to_string(),
                    crate::compiler::PackageFacts::new(package_name, env),
                );
            }
        }
    }
    imported_type_envs
}

fn refresh_borrowed_slice_params_with_imports(
    package_type_env: &mut TypeEnv,
    files: &[&crate::ast::File<'_>],
    imported_type_envs: &BTreeMap<String, crate::compiler::PackageFacts>,
) {
    loop {
        let mut changed = false;
        for ast in files {
            let mut inference_env = package_type_env.clone();
            crate::compiler::merge_import_type_envs(
                &mut inference_env,
                ast,
                &BTreeMap::new(),
                imported_type_envs,
            );
            changed |=
                package_type_env.refresh_borrowed_slice_params_from_env(&[*ast], &inference_env);
        }
        if !changed {
            break;
        }
    }
}

fn refresh_top_level_vars_with_imports(
    package_type_env: &mut TypeEnv,
    files: &[&crate::ast::File<'_>],
    imported_type_envs: &BTreeMap<String, crate::compiler::PackageFacts>,
) {
    for ast in files {
        let mut inference_env = package_type_env.clone();
        crate::compiler::merge_import_type_envs(
            &mut inference_env,
            ast,
            &BTreeMap::new(),
            imported_type_envs,
        );
        package_type_env.rescan_file_top_level_vars(ast, &inference_env);
    }
    merge_imported_receiver_facts_for_top_level_vars(package_type_env, files, imported_type_envs);
}

fn merge_imported_receiver_facts_for_top_level_vars(
    package_type_env: &mut TypeEnv,
    files: &[&crate::ast::File<'_>],
    imported_type_envs: &BTreeMap<String, crate::compiler::PackageFacts>,
) {
    let receiver_names_by_local = imported_receiver_names_by_local_name(package_type_env);
    if receiver_names_by_local.is_empty() {
        return;
    }
    for ast in files {
        for import in ast.imports() {
            let import_path = import.path.value.trim_matches('"');
            let Some(package_facts) = imported_type_envs.get(import_path) else {
                continue;
            };
            let Some(local_name) =
                import_type_env_local_name(&import, package_facts.package_name())
            else {
                continue;
            };
            let Some(receiver_names) = receiver_names_by_local.get(&local_name) else {
                continue;
            };
            package_type_env.merge_package_receiver_facts(
                &local_name,
                package_facts.type_env(),
                receiver_names,
            );
        }
    }
}

fn imported_receiver_names_by_local_name(
    package_type_env: &TypeEnv,
) -> BTreeMap<String, HashSet<String>> {
    let mut receiver_names = BTreeMap::new();
    for (_, go_type) in package_type_env.top_level_var_types_snapshot() {
        collect_imported_receiver_names_from_type(&go_type, &mut receiver_names);
    }
    receiver_names
}

fn collect_imported_receiver_names_from_type(
    go_type: &crate::compiler::typeinfer::GoType,
    receiver_names: &mut BTreeMap<String, HashSet<String>>,
) {
    use crate::compiler::typeinfer::GoType;

    match go_type {
        GoType::Named(name) | GoType::Interface(name) => {
            if let Some((package_name, receiver_name)) = name.split_once('.') {
                receiver_names
                    .entry(package_name.to_string())
                    .or_default()
                    .insert(receiver_name.to_string());
            }
        }
        GoType::Instantiated { name, args } => {
            if let Some((package_name, receiver_name)) = name.split_once('.') {
                receiver_names
                    .entry(package_name.to_string())
                    .or_default()
                    .insert(receiver_name.to_string());
            }
            for arg in args {
                collect_imported_receiver_names_from_type(arg, receiver_names);
            }
        }
        GoType::Pointer(inner) | GoType::Slice(inner) | GoType::Array(inner) => {
            collect_imported_receiver_names_from_type(inner, receiver_names);
        }
        GoType::Map(key, value) => {
            collect_imported_receiver_names_from_type(key, receiver_names);
            collect_imported_receiver_names_from_type(value, receiver_names);
        }
        GoType::Chan { elem, .. } => {
            collect_imported_receiver_names_from_type(elem, receiver_names);
        }
        GoType::Func {
            params, results, ..
        } => {
            for go_type in params.iter().chain(results.iter()) {
                collect_imported_receiver_names_from_type(go_type, receiver_names);
            }
        }
        GoType::Bool
        | GoType::Int
        | GoType::Int8
        | GoType::Int16
        | GoType::Int32
        | GoType::Int64
        | GoType::Uint
        | GoType::Uint8
        | GoType::Uint16
        | GoType::Uint32
        | GoType::Uint64
        | GoType::Uintptr
        | GoType::Float32
        | GoType::Float64
        | GoType::Complex64
        | GoType::Complex128
        | GoType::String
        | GoType::Any
        | GoType::Error
        | GoType::Unit
        | GoType::Unknown => {}
    }
}

fn import_type_env_local_name(
    import: &crate::ast::ImportSpec<'_>,
    package_name: &str,
) -> Option<String> {
    import
        .name
        .as_ref()
        .and_then(|name| match name.name {
            "." | "_" => None,
            other => Some(other.to_string()),
        })
        .or_else(|| Some(package_name.to_string()))
}

struct DeclRecoveryPlan {
    non_import_index: usize,
    label: String,
    split_specs: usize,
}

#[derive(Default)]
struct RecoverySelection {
    whole_decl_indices: BTreeSet<usize>,
    spec_indices: BTreeMap<usize, BTreeSet<usize>>,
}

struct ResolvedRecoveryContext<'a> {
    import_path: &'a str,
    package_type_env: &'a TypeEnv,
    imported_type_envs: &'a BTreeMap<String, crate::compiler::PackageFacts>,
    import_renames: &'a BTreeMap<String, String>,
    package_mutable_top_level_vars: &'a HashSet<String>,
    view_method_seed: &'a crate::compiler::FixedArrayViewMethodSeed,
    roots: Option<&'a HashSet<String>>,
}

fn recover_resolved_file_items<'a>(
    filename: &'a str,
    content: &'a str,
    reachable_names: Option<&HashSet<String>>,
    context: &ResolvedRecoveryContext<'_>,
) -> Vec<syn::Item> {
    let plans = recovery_plans_for_file(filename, content, reachable_names);
    let mut fallback_items = Vec::new();
    let mut selection = RecoverySelection::default();
    for plan in plans {
        let Some(shard) = parse_recovery_shard(
            filename,
            content,
            reachable_names,
            plan.non_import_index,
            None,
        ) else {
            continue;
        };
        match compile_resolved_file(
            shard,
            context.package_type_env,
            context.imported_type_envs,
            context.import_renames,
            context.package_mutable_top_level_vars,
            context.view_method_seed,
            context.roots,
        ) {
            Ok(compiled) => {
                selection.whole_decl_indices.insert(plan.non_import_index);
                fallback_items.extend(compiled.items);
                continue;
            }
            Err(error) => {
                log_skip(format_args!(
                    "[gors] skip {}/{filename} {}: compile error: {error}",
                    context.import_path, plan.label
                ));
            }
        }

        for spec_index in 0..plan.split_specs {
            let Some(shard) = parse_recovery_shard(
                filename,
                content,
                reachable_names,
                plan.non_import_index,
                Some(spec_index),
            ) else {
                continue;
            };
            let label = spec_label_for_shard(&shard)
                .unwrap_or_else(|| format!("{} spec {}", plan.label, spec_index.saturating_add(1)));
            match compile_resolved_file(
                shard,
                context.package_type_env,
                context.imported_type_envs,
                context.import_renames,
                context.package_mutable_top_level_vars,
                context.view_method_seed,
                context.roots,
            ) {
                Ok(compiled) => {
                    selection
                        .spec_indices
                        .entry(plan.non_import_index)
                        .or_default()
                        .insert(spec_index);
                    fallback_items.extend(compiled.items);
                }
                Err(error) => {
                    log_skip(format_args!(
                        "[gors] skip {}/{filename} {label}: compile error: {error}",
                        context.import_path
                    ));
                }
            }
        }
    }
    let Some(combined) = parse_recovery_selection(filename, content, reachable_names, &selection)
    else {
        return fallback_items;
    };
    match compile_resolved_file(
        combined,
        context.package_type_env,
        context.imported_type_envs,
        context.import_renames,
        context.package_mutable_top_level_vars,
        context.view_method_seed,
        context.roots,
    ) {
        Ok(compiled) => compiled.items,
        Err(error) => {
            log_skip(format_args!(
                "[gors] skip {}/{filename} combined recovered declarations: compile error: {error}",
                context.import_path
            ));
            fallback_items
        }
    }
}

fn recovery_plans_for_file<'a>(
    filename: &'a str,
    content: &'a str,
    reachable_names: Option<&HashSet<String>>,
) -> Vec<DeclRecoveryPlan> {
    let Some(file) = parse_filtered_file(filename, content, reachable_names) else {
        return Vec::new();
    };

    file.decls
        .iter()
        .filter(|decl| !is_import_decl(decl))
        .enumerate()
        .map(|(non_import_index, decl)| DeclRecoveryPlan {
            non_import_index,
            label: decl_label(decl),
            split_specs: splittable_spec_count(decl),
        })
        .collect()
}

fn parse_recovery_shard<'a>(
    filename: &'a str,
    content: &'a str,
    reachable_names: Option<&HashSet<String>>,
    target_non_import_index: usize,
    target_spec_index: Option<usize>,
) -> Option<crate::ast::File<'a>> {
    let file = parse_filtered_file(filename, content, reachable_names)?;
    file_with_recovery_shard(file, target_non_import_index, target_spec_index)
}

fn parse_recovery_selection<'a>(
    filename: &'a str,
    content: &'a str,
    reachable_names: Option<&HashSet<String>>,
    selection: &RecoverySelection,
) -> Option<crate::ast::File<'a>> {
    if selection.whole_decl_indices.is_empty() && selection.spec_indices.is_empty() {
        return None;
    }
    let file = parse_filtered_file(filename, content, reachable_names)?;
    file_with_recovery_selection(file, selection)
}

fn parse_filtered_file<'a>(
    filename: &'a str,
    content: &'a str,
    reachable_names: Option<&HashSet<String>>,
) -> Option<crate::ast::File<'a>> {
    let ast = match crate::parser::parse_file(filename, content) {
        Ok(ast) => ast,
        Err(error) => {
            log_skip(format_args!("[gors] skip {filename}: parse error: {error}"));
            return None;
        }
    };
    let filtered = match reachable_names {
        Some(reachable) => filter_file_to_reachable(ast, reachable),
        None => ast,
    };
    file_has_compilable_decl(&filtered).then_some(filtered)
}

fn file_with_recovery_shard<'a>(
    mut file: crate::ast::File<'a>,
    target_non_import_index: usize,
    target_spec_index: Option<usize>,
) -> Option<crate::ast::File<'a>> {
    let mut decls = Vec::new();
    let mut selected = None;
    let mut non_import_index = 0;
    for decl in std::mem::take(&mut file.decls) {
        if is_import_decl(&decl) {
            decls.push(decl);
            continue;
        }
        if non_import_index == target_non_import_index {
            selected = match target_spec_index {
                Some(spec_index) => single_spec_decl(decl, spec_index),
                None => Some(decl),
            };
        }
        non_import_index += 1;
    }

    decls.push(selected?);
    file.decls = decls;
    Some(file)
}

fn file_with_recovery_selection<'a>(
    mut file: crate::ast::File<'a>,
    selection: &RecoverySelection,
) -> Option<crate::ast::File<'a>> {
    let mut decls = Vec::new();
    let mut non_import_index = 0;
    for decl in std::mem::take(&mut file.decls) {
        if is_import_decl(&decl) {
            decls.push(decl);
            continue;
        }
        if selection.whole_decl_indices.contains(&non_import_index) {
            decls.push(decl);
        } else if let Some(spec_indices) = selection.spec_indices.get(&non_import_index)
            && let Some(decl) = decl_with_recovery_specs(decl, spec_indices)
        {
            decls.push(decl);
        }
        non_import_index += 1;
    }

    file.decls = decls;
    file_has_compilable_decl(&file).then_some(file)
}

fn decl_with_recovery_specs<'a>(
    decl: crate::ast::Decl<'a>,
    spec_indices: &BTreeSet<usize>,
) -> Option<crate::ast::Decl<'a>> {
    let crate::ast::Decl::GenDecl(mut gen_decl) = decl else {
        return None;
    };
    if gen_decl.tok == crate::token::Token::IMPORT {
        return None;
    }
    gen_decl.specs = std::mem::take(&mut gen_decl.specs)
        .into_iter()
        .enumerate()
        .filter_map(|(idx, spec)| spec_indices.contains(&idx).then_some(spec))
        .collect();
    (!gen_decl.specs.is_empty()).then_some(crate::ast::Decl::GenDecl(gen_decl))
}

fn single_spec_decl<'a>(
    decl: crate::ast::Decl<'a>,
    target_spec_index: usize,
) -> Option<crate::ast::Decl<'a>> {
    let crate::ast::Decl::GenDecl(mut gen_decl) = decl else {
        return None;
    };
    if gen_decl.tok == crate::token::Token::IMPORT {
        return None;
    }
    let spec = std::mem::take(&mut gen_decl.specs)
        .into_iter()
        .nth(target_spec_index)?;
    gen_decl.specs = vec![spec];
    Some(crate::ast::Decl::GenDecl(gen_decl))
}

fn is_import_decl(decl: &crate::ast::Decl<'_>) -> bool {
    matches!(
        decl,
        crate::ast::Decl::GenDecl(gen_decl) if gen_decl.tok == crate::token::Token::IMPORT
    )
}

fn splittable_spec_count(decl: &crate::ast::Decl<'_>) -> usize {
    let crate::ast::Decl::GenDecl(gen_decl) = decl else {
        return 0;
    };
    if gen_decl.tok == crate::token::Token::IMPORT || gen_decl.specs.len() <= 1 {
        return 0;
    }
    gen_decl.specs.len()
}

fn decl_label(decl: &crate::ast::Decl<'_>) -> String {
    match decl {
        crate::ast::Decl::FuncDecl(func) => {
            if let Some(receiver) = receiver_type_name(func) {
                format!("method {receiver}.{}", func.name.name)
            } else {
                format!("func {}", func.name.name)
            }
        }
        crate::ast::Decl::GenDecl(gen_decl) => {
            let token: &'static str = (&gen_decl.tok).into();
            let names = gen_decl
                .specs
                .iter()
                .flat_map(spec_names)
                .collect::<Vec<_>>()
                .join(", ");
            if names.is_empty() {
                format!("{token} declaration")
            } else {
                format!("{token} {names}")
            }
        }
    }
}

fn spec_label_for_shard(file: &crate::ast::File<'_>) -> Option<String> {
    file.decls
        .iter()
        .find(|decl| !is_import_decl(decl))
        .map(decl_label)
}

fn item_mod_for(import_path: &str, items: Vec<syn::Item>) -> syn::ItemMod {
    syn::ItemMod {
        attrs: vec![],
        vis: syn::Visibility::Inherited,
        unsafety: None,
        mod_token: <syn::Token![mod]>::default(),
        ident: syn::Ident::new(&module_name(import_path), proc_macro2::Span::mixed_site()),
        content: Some((syn::token::Brace::default(), items)),
        semi: None,
    }
}

fn dedupe_use_items(items: &mut Vec<syn::Item>) {
    let mut seen = Vec::<syn::ItemUse>::new();
    let mut deduped = Vec::with_capacity(items.len());
    for item in std::mem::take(items) {
        let syn::Item::Use(item_use) = &item else {
            deduped.push(item);
            continue;
        };
        if seen
            .iter()
            .any(|existing| use_items_match(existing, item_use))
        {
            continue;
        }
        seen.push(item_use.clone());
        deduped.push(item);
    }
    *items = deduped;
}

fn use_items_match(left: &syn::ItemUse, right: &syn::ItemUse) -> bool {
    left.leading_colon.is_some() == right.leading_colon.is_some()
        && visibilities_match(&left.vis, &right.vis)
        && use_trees_match(&left.tree, &right.tree)
}

fn visibilities_match(left: &syn::Visibility, right: &syn::Visibility) -> bool {
    match (left, right) {
        (syn::Visibility::Inherited, syn::Visibility::Inherited) => true,
        (syn::Visibility::Public(_), syn::Visibility::Public(_)) => true,
        (syn::Visibility::Restricted(left), syn::Visibility::Restricted(right)) => {
            left.in_token.is_some() == right.in_token.is_some()
                && paths_match(&left.path, &right.path)
        }
        _ => false,
    }
}

fn use_trees_match(left: &syn::UseTree, right: &syn::UseTree) -> bool {
    match (left, right) {
        (syn::UseTree::Path(left), syn::UseTree::Path(right)) => {
            left.ident == right.ident && use_trees_match(&left.tree, &right.tree)
        }
        (syn::UseTree::Name(left), syn::UseTree::Name(right)) => left.ident == right.ident,
        (syn::UseTree::Rename(left), syn::UseTree::Rename(right)) => {
            left.ident == right.ident && left.rename == right.rename
        }
        (syn::UseTree::Glob(_), syn::UseTree::Glob(_)) => true,
        (syn::UseTree::Group(left), syn::UseTree::Group(right)) => {
            left.items.len() == right.items.len()
                && left
                    .items
                    .iter()
                    .zip(right.items.iter())
                    .all(|(left, right)| use_trees_match(left, right))
        }
        _ => false,
    }
}

fn paths_match(left: &syn::Path, right: &syn::Path) -> bool {
    left.leading_colon.is_some() == right.leading_colon.is_some()
        && left.segments.len() == right.segments.len()
        && left
            .segments
            .iter()
            .zip(right.segments.iter())
            .all(|(left, right)| {
                left.ident == right.ident
                    && matches!(
                        (&left.arguments, &right.arguments),
                        (syn::PathArguments::None, syn::PathArguments::None)
                    )
            })
}

fn log_skip(args: std::fmt::Arguments<'_>) {
    if std::env::var("GORS_STDLIB_TRACE").is_ok_and(|value| value == "1" || value == "true") {
        eprintln!("{args}");
    }
}

pub fn scan_type_env(import_path: &str) -> Option<(String, TypeEnv)> {
    let Some(cell) = type_env_cell(import_path) else {
        return scan_type_env_uncached(import_path);
    };
    cell.get_or_init(|| scan_type_env_uncached(import_path))
        .clone()
}

fn type_env_cell(import_path: &str) -> Option<TypeEnvCell> {
    if let Ok(cache) = type_envs().read()
        && let Some(cell) = cache.get(import_path)
    {
        return Some(cell.clone());
    }

    let Ok(mut cache) = type_envs().write() else {
        return None;
    };
    Some(
        cache
            .entry(import_path.to_string())
            .or_insert_with(|| Arc::new(OnceLock::new()))
            .clone(),
    )
}

fn scan_type_env_uncached(import_path: &str) -> Option<(String, TypeEnv)> {
    let files = package_files(import_path)?;
    let mut env = TypeEnv::new();
    let mut package_name = None;
    let mut parsed_files = Vec::new();

    for (filename, content) in files.iter() {
        let Ok(ast) = crate::parser::parse_file(filename, content) else {
            continue;
        };
        if package_name.is_none() {
            package_name = Some(ast.name.name.to_string());
        }
        parsed_files.push(ast);
    }

    let parsed_file_refs = parsed_files.iter().collect::<Vec<_>>();
    env.scan_files(&parsed_file_refs);
    runtime_primitives::supplement_type_env(import_path, &mut env);
    let imported_type_envs = scan_imported_type_envs(import_path, &parsed_file_refs);
    refresh_borrowed_slice_params_with_imports(&mut env, &parsed_file_refs, &imported_type_envs);
    refresh_top_level_vars_with_imports(&mut env, &parsed_file_refs, &imported_type_envs);

    package_name.map(|name| (name, env))
}

pub fn collect_transitive_imports(import_path: &str) -> Vec<String> {
    let Some(cell) = transitive_imports_cell(import_path) else {
        return collect_transitive_imports_uncached(import_path);
    };
    cell.get_or_init(|| collect_transitive_imports_uncached(import_path))
        .clone()
}

fn transitive_imports_cell(import_path: &str) -> Option<Arc<OnceLock<Vec<String>>>> {
    if let Ok(cache) = transitive_imports().read()
        && let Some(cell) = cache.get(import_path)
    {
        return Some(cell.clone());
    }

    let Ok(mut cache) = transitive_imports().write() else {
        return None;
    };
    Some(
        cache
            .entry(import_path.to_string())
            .or_insert_with(|| Arc::new(OnceLock::new()))
            .clone(),
    )
}

pub fn collect_resolved_imports(import_path: &str, roots: &HashSet<String>) -> Vec<String> {
    let cache_key = resolve_cache_key(import_path, Some(roots));
    if let Ok(cache) = resolved_imports().read()
        && let Some(cached) = cache.get(&cache_key)
    {
        return cached.clone();
    }
    collect_transitive_imports(import_path)
}

fn cache_resolved_imports(
    import_path: &str,
    roots: Option<&HashSet<String>>,
    imports: Vec<String>,
) {
    let cache_key = resolve_cache_key(import_path, roots);
    if let Ok(mut cache) = resolved_imports().write() {
        cache.entry(cache_key).or_insert(imports);
    }
}

fn collect_transitive_imports_uncached(import_path: &str) -> Vec<String> {
    let Some(package) = embedded_package(import_path) else {
        return Vec::new();
    };
    package
        .direct_imports
        .iter()
        .copied()
        .filter(|path| *path != import_path && is_known(path))
        .map(str::to_string)
        .collect()
}

fn reachable_package_names(
    parsed_files: &[(&str, crate::ast::File<'_>)],
    roots: &HashSet<String>,
) -> HashSet<String> {
    let top_names = package_top_level_names(parsed_files);
    let mut env = TypeEnv::new();
    let file_refs = parsed_files
        .iter()
        .map(|(_, file)| file)
        .collect::<Vec<_>>();
    env.scan_files(&file_refs);
    let interface_method_roots = interface_method_roots(roots, &top_names, &env);
    let mut reachable: HashSet<String> = roots
        .iter()
        .filter(|name| top_names.contains(name.as_str()))
        .cloned()
        .collect();
    let mut value_reachable = reachable
        .iter()
        .filter(|name| !name.contains("::"))
        .cloned()
        .collect::<HashSet<_>>();

    let mut changed = true;
    while changed {
        changed = false;
        changed |= expand_value_receiver_methods(&mut reachable, &value_reachable, &top_names);
        changed |= expand_reachable_interface_methods(
            &mut reachable,
            &value_reachable,
            &top_names,
            &interface_method_roots,
            &env,
        );
        for (_, file) in parsed_files {
            for decl in &file.decls {
                if !decl_is_reachable(decl, &reachable) {
                    continue;
                }
                for name in decl_names(decl) {
                    changed |= reachable.insert(name);
                }
                let mut refs = HashSet::new();
                refs_from_decl(decl, &mut refs);
                for reference in refs {
                    if top_names.contains(reference.as_str()) {
                        changed |= reachable.insert(reference);
                    }
                }
                let mut method_refs = HashSet::new();
                method_refs_from_decl(decl, &env, &mut method_refs);
                for reference in method_refs {
                    if top_names.contains(reference.as_str()) {
                        changed |= reachable.insert(reference);
                    }
                }
                let mut value_refs = HashSet::new();
                value_refs_from_decl(decl, &mut value_refs);
                value_field_refs_from_decl(decl, &value_reachable, &mut value_refs);
                for reference in value_refs {
                    if top_names.contains(reference.as_str()) {
                        changed |= reachable.insert(reference.clone());
                        changed |= value_reachable.insert(reference);
                    }
                }
                changed |= expand_type_switch_case_interface_methods(
                    &mut reachable,
                    &top_names,
                    decl,
                    &env,
                );
            }
        }
    }

    reachable
}

fn expand_value_receiver_methods(
    reachable: &mut HashSet<String>,
    value_reachable: &HashSet<String>,
    top_names: &HashSet<String>,
) -> bool {
    let mut changed = false;
    for concrete in value_reachable {
        let receiver_prefix = format!("{concrete}::");
        for name in top_names {
            if name.starts_with(&receiver_prefix) {
                changed |= reachable.insert(name.clone());
            }
        }
    }
    changed
}

fn interface_method_roots(
    roots: &HashSet<String>,
    top_names: &HashSet<String>,
    env: &TypeEnv,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut method_roots: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for root in roots {
        let Some((receiver, method)) = root.split_once("::") else {
            continue;
        };
        if top_names.contains(receiver) && env.is_interface(receiver) {
            method_roots
                .entry(receiver.to_string())
                .or_default()
                .insert(method.to_string());
        }
    }
    method_roots
}

fn expand_reachable_interface_methods(
    reachable: &mut HashSet<String>,
    value_reachable: &HashSet<String>,
    top_names: &HashSet<String>,
    interface_method_roots: &BTreeMap<String, BTreeSet<String>>,
    env: &TypeEnv,
) -> bool {
    let mut changed = false;
    for concrete in value_reachable {
        for (interface_name, methods) in interface_method_roots {
            if !env.named_type_implements_interface(concrete, interface_name, true) {
                continue;
            }
            for method in methods {
                let method_root = format!("{concrete}::{method}");
                if top_names.contains(&method_root) {
                    changed |= reachable.insert(method_root);
                }
            }
        }
    }
    changed
}

fn expand_type_switch_case_interface_methods(
    reachable: &mut HashSet<String>,
    top_names: &HashSet<String>,
    decl: &crate::ast::Decl<'_>,
    env: &TypeEnv,
) -> bool {
    let mut changed = false;
    type_switch_case_interface_methods_from_decl(decl, env, &mut |case_type, methods| {
        changed |= reachable.insert(case_type.to_string());
        for method in methods {
            let method_root = format!("{case_type}::{method}");
            if top_names.contains(&method_root) {
                changed |= reachable.insert(method_root);
            }
        }
    });
    changed
}

fn type_switch_case_interface_methods_from_decl<'a>(
    decl: &'a crate::ast::Decl<'a>,
    env: &TypeEnv,
    on_case: &mut impl FnMut(&str, &[String]),
) {
    match decl {
        crate::ast::Decl::FuncDecl(func) => {
            if let Some(body) = &func.body {
                type_switch_case_interface_methods_from_block(body, env, on_case);
            }
        }
        crate::ast::Decl::GenDecl(gen_decl) => {
            type_switch_case_interface_methods_from_gen_decl(gen_decl, env, on_case);
        }
    }
}

fn type_switch_case_interface_methods_from_gen_decl<'a>(
    gen_decl: &'a crate::ast::GenDecl<'a>,
    env: &TypeEnv,
    on_case: &mut impl FnMut(&str, &[String]),
) {
    for spec in &gen_decl.specs {
        if let crate::ast::Spec::ValueSpec(value_spec) = spec {
            for expr in value_spec.values.as_deref().unwrap_or(&[]) {
                type_switch_case_interface_methods_from_expr(expr, env, on_case);
            }
        }
    }
}

fn type_switch_case_interface_methods_from_block<'a>(
    block: &'a crate::ast::BlockStmt<'a>,
    env: &TypeEnv,
    on_case: &mut impl FnMut(&str, &[String]),
) {
    for stmt in &block.list {
        type_switch_case_interface_methods_from_stmt(stmt, env, on_case);
    }
}

fn type_switch_case_interface_methods_from_stmt<'a>(
    stmt: &'a crate::ast::Stmt<'a>,
    env: &TypeEnv,
    on_case: &mut impl FnMut(&str, &[String]),
) {
    match stmt {
        crate::ast::Stmt::AssignStmt(assign) => {
            for expr in assign.lhs.iter().chain(assign.rhs.iter()) {
                type_switch_case_interface_methods_from_expr(expr, env, on_case);
            }
        }
        crate::ast::Stmt::BlockStmt(block) => {
            type_switch_case_interface_methods_from_block(block, env, on_case);
        }
        crate::ast::Stmt::BranchStmt(_) | crate::ast::Stmt::EmptyStmt(_) => {}
        crate::ast::Stmt::CaseClause(case_clause) => {
            for expr in case_clause.list.as_deref().unwrap_or(&[]) {
                type_switch_case_interface_methods_from_expr(expr, env, on_case);
            }
            for stmt in &case_clause.body {
                type_switch_case_interface_methods_from_stmt(stmt, env, on_case);
            }
        }
        crate::ast::Stmt::CommClause(comm_clause) => {
            if let Some(comm) = comm_clause.comm.as_deref() {
                type_switch_case_interface_methods_from_stmt(comm, env, on_case);
            }
            for stmt in &comm_clause.body {
                type_switch_case_interface_methods_from_stmt(stmt, env, on_case);
            }
        }
        crate::ast::Stmt::DeclStmt(decl_stmt) => {
            type_switch_case_interface_methods_from_gen_decl(&decl_stmt.decl, env, on_case);
        }
        crate::ast::Stmt::DeferStmt(defer_stmt) => {
            type_switch_case_interface_methods_from_call(&defer_stmt.call, env, on_case);
        }
        crate::ast::Stmt::ExprStmt(expr_stmt) => {
            type_switch_case_interface_methods_from_expr(&expr_stmt.x, env, on_case);
        }
        crate::ast::Stmt::ForStmt(for_stmt) => {
            if let Some(init) = for_stmt.init.as_deref() {
                type_switch_case_interface_methods_from_stmt(init, env, on_case);
            }
            if let Some(cond) = &for_stmt.cond {
                type_switch_case_interface_methods_from_expr(cond, env, on_case);
            }
            if let Some(post) = for_stmt.post.as_deref() {
                type_switch_case_interface_methods_from_stmt(post, env, on_case);
            }
            type_switch_case_interface_methods_from_block(&for_stmt.body, env, on_case);
        }
        crate::ast::Stmt::GoStmt(go_stmt) => {
            type_switch_case_interface_methods_from_call(&go_stmt.call, env, on_case);
        }
        crate::ast::Stmt::IfStmt(if_stmt) => {
            if let Some(init) = if_stmt.init.as_ref().as_ref() {
                type_switch_case_interface_methods_from_stmt(init, env, on_case);
            }
            type_switch_case_interface_methods_from_expr(&if_stmt.cond, env, on_case);
            type_switch_case_interface_methods_from_block(&if_stmt.body, env, on_case);
            if let Some(else_stmt) = if_stmt.else_.as_ref().as_ref() {
                type_switch_case_interface_methods_from_stmt(else_stmt, env, on_case);
            }
        }
        crate::ast::Stmt::IncDecStmt(inc_dec) => {
            type_switch_case_interface_methods_from_expr(&inc_dec.x, env, on_case);
        }
        crate::ast::Stmt::LabeledStmt(labeled) => {
            type_switch_case_interface_methods_from_stmt(&labeled.stmt, env, on_case);
        }
        crate::ast::Stmt::RangeStmt(range) => {
            if let Some(key) = &range.key {
                type_switch_case_interface_methods_from_expr(key, env, on_case);
            }
            if let Some(value) = &range.value {
                type_switch_case_interface_methods_from_expr(value, env, on_case);
            }
            type_switch_case_interface_methods_from_expr(&range.x, env, on_case);
            type_switch_case_interface_methods_from_block(&range.body, env, on_case);
        }
        crate::ast::Stmt::ReturnStmt(return_stmt) => {
            for expr in &return_stmt.results {
                type_switch_case_interface_methods_from_expr(expr, env, on_case);
            }
        }
        crate::ast::Stmt::SelectStmt(select_stmt) => {
            type_switch_case_interface_methods_from_block(&select_stmt.body, env, on_case);
        }
        crate::ast::Stmt::SendStmt(send_stmt) => {
            type_switch_case_interface_methods_from_expr(&send_stmt.chan, env, on_case);
            type_switch_case_interface_methods_from_expr(&send_stmt.value, env, on_case);
        }
        crate::ast::Stmt::SwitchStmt(switch_stmt) => {
            if let Some(init) = switch_stmt.init.as_deref() {
                type_switch_case_interface_methods_from_stmt(init, env, on_case);
            }
            if let Some(tag) = &switch_stmt.tag {
                type_switch_case_interface_methods_from_expr(tag, env, on_case);
            }
            type_switch_case_interface_methods_from_block(&switch_stmt.body, env, on_case);
        }
        crate::ast::Stmt::TypeSwitchStmt(type_switch) => {
            if let Some(init) = type_switch.init.as_deref() {
                type_switch_case_interface_methods_from_stmt(init, env, on_case);
            }
            type_switch_case_interface_methods_from_stmt(&type_switch.assign, env, on_case);
            let guard_methods = type_switch_guard_interface_methods(type_switch, env);
            for stmt in &type_switch.body.list {
                let crate::ast::Stmt::CaseClause(case_clause) = stmt else {
                    type_switch_case_interface_methods_from_stmt(stmt, env, on_case);
                    continue;
                };
                if let Some(methods) = guard_methods.as_deref()
                    && let Some(exprs) = &case_clause.list
                {
                    for expr in exprs {
                        let Some((case_type, include_pointer_methods)) =
                            type_switch_case_named_type(expr)
                        else {
                            continue;
                        };
                        if env.named_type_implements_methods(
                            case_type,
                            methods,
                            include_pointer_methods,
                        ) {
                            on_case(case_type, methods);
                        }
                    }
                }
                for stmt in &case_clause.body {
                    type_switch_case_interface_methods_from_stmt(stmt, env, on_case);
                }
            }
        }
    }
}

fn type_switch_case_interface_methods_from_expr<'a>(
    expr: &'a crate::ast::Expr<'a>,
    env: &TypeEnv,
    on_case: &mut impl FnMut(&str, &[String]),
) {
    match expr {
        crate::ast::Expr::ArrayType(array) => {
            if let Some(len) = array.len.as_deref() {
                type_switch_case_interface_methods_from_expr(len, env, on_case);
            }
            type_switch_case_interface_methods_from_expr(&array.elt, env, on_case);
        }
        crate::ast::Expr::BasicLit(_) | crate::ast::Expr::Ident(_) => {}
        crate::ast::Expr::BinaryExpr(binary) => {
            type_switch_case_interface_methods_from_expr(&binary.x, env, on_case);
            type_switch_case_interface_methods_from_expr(&binary.y, env, on_case);
        }
        crate::ast::Expr::CallExpr(call) => {
            type_switch_case_interface_methods_from_call(call, env, on_case);
        }
        crate::ast::Expr::ChanType(chan) => {
            type_switch_case_interface_methods_from_expr(&chan.value, env, on_case);
        }
        crate::ast::Expr::CompositeLit(composite) => {
            if let Some(type_expr) = composite.type_.as_deref() {
                type_switch_case_interface_methods_from_expr(type_expr, env, on_case);
            }
            for expr in composite.elts.as_deref().unwrap_or(&[]) {
                type_switch_case_interface_methods_from_expr(expr, env, on_case);
            }
        }
        crate::ast::Expr::Ellipsis(ellipsis) => {
            if let Some(elt) = ellipsis.elt.as_deref() {
                type_switch_case_interface_methods_from_expr(elt, env, on_case);
            }
        }
        crate::ast::Expr::FuncLit(func_lit) => {
            type_switch_case_interface_methods_from_block(&func_lit.body, env, on_case);
        }
        crate::ast::Expr::FuncType(_)
        | crate::ast::Expr::InterfaceType(_)
        | crate::ast::Expr::MapType(_)
        | crate::ast::Expr::StructType(_) => {}
        crate::ast::Expr::IndexExpr(index) => {
            type_switch_case_interface_methods_from_expr(&index.x, env, on_case);
            type_switch_case_interface_methods_from_expr(&index.index, env, on_case);
        }
        crate::ast::Expr::IndexListExpr(index) => {
            type_switch_case_interface_methods_from_expr(&index.x, env, on_case);
            for expr in &index.indices {
                type_switch_case_interface_methods_from_expr(expr, env, on_case);
            }
        }
        crate::ast::Expr::KeyValueExpr(key_value) => {
            type_switch_case_interface_methods_from_expr(&key_value.key, env, on_case);
            type_switch_case_interface_methods_from_expr(&key_value.value, env, on_case);
        }
        crate::ast::Expr::ParenExpr(paren) => {
            type_switch_case_interface_methods_from_expr(&paren.x, env, on_case);
        }
        crate::ast::Expr::SelectorExpr(selector) => {
            type_switch_case_interface_methods_from_expr(&selector.x, env, on_case);
        }
        crate::ast::Expr::SliceExpr(slice) => {
            type_switch_case_interface_methods_from_expr(&slice.x, env, on_case);
            if let Some(low) = slice.low.as_deref() {
                type_switch_case_interface_methods_from_expr(low, env, on_case);
            }
            if let Some(high) = slice.high.as_deref() {
                type_switch_case_interface_methods_from_expr(high, env, on_case);
            }
            if let Some(max) = slice.max.as_deref() {
                type_switch_case_interface_methods_from_expr(max, env, on_case);
            }
        }
        crate::ast::Expr::StarExpr(star) => {
            type_switch_case_interface_methods_from_expr(&star.x, env, on_case);
        }
        crate::ast::Expr::TypeAssertExpr(type_assert) => {
            type_switch_case_interface_methods_from_expr(&type_assert.x, env, on_case);
            if let Some(type_expr) = type_assert.type_.as_deref() {
                type_switch_case_interface_methods_from_expr(type_expr, env, on_case);
            }
        }
        crate::ast::Expr::UnaryExpr(unary) => {
            type_switch_case_interface_methods_from_expr(&unary.x, env, on_case);
        }
    }
}

fn type_switch_case_interface_methods_from_call<'a>(
    call: &'a crate::ast::CallExpr<'a>,
    env: &TypeEnv,
    on_case: &mut impl FnMut(&str, &[String]),
) {
    type_switch_case_interface_methods_from_expr(&call.fun, env, on_case);
    for expr in call.args.as_deref().unwrap_or(&[]) {
        type_switch_case_interface_methods_from_expr(expr, env, on_case);
    }
}

fn type_switch_guard_interface_methods(
    type_switch: &crate::ast::TypeSwitchStmt<'_>,
    env: &TypeEnv,
) -> Option<Vec<String>> {
    let guard = type_switch_guard_operand(&type_switch.assign)?;
    let guard_type = env.resolve_alias(&crate::compiler::typeinfer::GoType::infer_expr(guard, env));
    let crate::compiler::typeinfer::GoType::Named(name) = guard_type else {
        return None;
    };
    env.is_interface(&name)
        .then(|| env.get_interface_methods(&name))
        .flatten()
        .filter(|methods| !methods.is_empty())
}

fn type_switch_guard_operand<'a>(
    stmt: &'a crate::ast::Stmt<'a>,
) -> Option<&'a crate::ast::Expr<'a>> {
    match stmt {
        crate::ast::Stmt::ExprStmt(expr) => type_switch_guard_operand_expr(&expr.x),
        crate::ast::Stmt::AssignStmt(assign) => {
            assign.rhs.first().and_then(type_switch_guard_operand_expr)
        }
        _ => None,
    }
}

fn type_switch_guard_operand_expr<'a>(
    expr: &'a crate::ast::Expr<'a>,
) -> Option<&'a crate::ast::Expr<'a>> {
    match expr {
        crate::ast::Expr::ParenExpr(paren) => type_switch_guard_operand_expr(&paren.x),
        crate::ast::Expr::TypeAssertExpr(assert) if assert.type_.is_none() => Some(&assert.x),
        _ => None,
    }
}

fn type_switch_case_named_type<'a>(expr: &'a crate::ast::Expr<'a>) -> Option<(&'a str, bool)> {
    match expr {
        crate::ast::Expr::Ident(ident) => Some((ident.name, false)),
        crate::ast::Expr::ParenExpr(paren) => type_switch_case_named_type(&paren.x),
        crate::ast::Expr::StarExpr(star) => {
            let (name, _) = type_switch_case_named_type(&star.x)?;
            Some((name, true))
        }
        _ => None,
    }
}

fn package_top_level_names(parsed_files: &[(&str, crate::ast::File<'_>)]) -> HashSet<String> {
    parsed_files
        .iter()
        .flat_map(|(_, file)| file.decls.iter())
        .flat_map(decl_names)
        .collect()
}

fn decl_names(decl: &crate::ast::Decl<'_>) -> Vec<String> {
    match decl {
        crate::ast::Decl::FuncDecl(func) if func.recv.is_none() => {
            vec![func.name.name.to_string()]
        }
        crate::ast::Decl::FuncDecl(func) => receiver_method_name(func).into_iter().collect(),
        crate::ast::Decl::GenDecl(gen_decl) => gen_decl
            .specs
            .iter()
            .flat_map(spec_names)
            .collect::<Vec<_>>(),
    }
}

fn spec_names(spec: &crate::ast::Spec<'_>) -> Vec<String> {
    match spec {
        crate::ast::Spec::ImportSpec(_) => Vec::new(),
        crate::ast::Spec::TypeSpec(type_spec) => type_spec
            .name
            .as_ref()
            .map(|name| vec![name.name.to_string()])
            .unwrap_or_default(),
        crate::ast::Spec::ValueSpec(value_spec) => value_spec
            .names
            .iter()
            .map(|name| name.name.to_string())
            .collect(),
    }
}

fn decl_is_reachable(decl: &crate::ast::Decl<'_>, reachable: &HashSet<String>) -> bool {
    match decl {
        crate::ast::Decl::FuncDecl(func) => {
            if func_decl_is_package_init(func) {
                true
            } else if func.recv.is_none() {
                reachable.contains(func.name.name)
            } else {
                receiver_method_name(func).is_some_and(|name| reachable.contains(&name))
            }
        }
        crate::ast::Decl::GenDecl(gen_decl) => gen_decl
            .specs
            .iter()
            .any(|spec| spec_names(spec).iter().any(|name| reachable.contains(name))),
    }
}

fn func_decl_is_package_init(func: &crate::ast::FuncDecl<'_>) -> bool {
    func.recv.is_none() && func.name.name == "init"
}

fn receiver_type_name(func: &crate::ast::FuncDecl<'_>) -> Option<String> {
    func.recv
        .as_ref()
        .and_then(|recv| recv.list.first())
        .and_then(|field| field.type_.as_ref())
        .and_then(named_type_from_expr)
}

fn receiver_method_name(func: &crate::ast::FuncDecl<'_>) -> Option<String> {
    receiver_type_name(func).map(|receiver| format!("{receiver}::{}", func.name.name))
}

fn named_type_from_expr(expr: &crate::ast::Expr<'_>) -> Option<String> {
    match expr {
        crate::ast::Expr::Ident(ident) => Some(ident.name.to_string()),
        crate::ast::Expr::StarExpr(star) => named_type_from_expr(&star.x),
        crate::ast::Expr::ParenExpr(paren) => named_type_from_expr(&paren.x),
        crate::ast::Expr::IndexExpr(index) => named_type_from_expr(&index.x),
        crate::ast::Expr::IndexListExpr(index) => named_type_from_expr(&index.x),
        _ => None,
    }
}

fn filter_file_to_reachable<'a>(
    mut file: crate::ast::File<'a>,
    reachable: &HashSet<String>,
) -> crate::ast::File<'a> {
    file.decls = file
        .decls
        .into_iter()
        .filter_map(|decl| filter_decl_to_reachable(decl, reachable))
        .collect();
    file
}

fn filter_decl_to_reachable<'a>(
    decl: crate::ast::Decl<'a>,
    reachable: &HashSet<String>,
) -> Option<crate::ast::Decl<'a>> {
    match decl {
        crate::ast::Decl::FuncDecl(func) => {
            let keep = if func_decl_is_package_init(&func) {
                true
            } else if func.recv.is_none() {
                reachable.contains(func.name.name)
            } else {
                receiver_method_name(&func).is_some_and(|name| reachable.contains(&name))
            };
            keep.then_some(crate::ast::Decl::FuncDecl(func))
        }
        crate::ast::Decl::GenDecl(mut gen_decl) => {
            if gen_decl.tok == crate::token::Token::IMPORT {
                return Some(crate::ast::Decl::GenDecl(gen_decl));
            }
            gen_decl
                .specs
                .retain(|spec| spec_names(spec).iter().any(|name| reachable.contains(name)));
            (!gen_decl.specs.is_empty()).then_some(crate::ast::Decl::GenDecl(gen_decl))
        }
    }
}

fn file_has_compilable_decl(file: &crate::ast::File<'_>) -> bool {
    file.decls.iter().any(|decl| {
        !matches!(
            decl,
            crate::ast::Decl::GenDecl(gen_decl) if gen_decl.tok == crate::token::Token::IMPORT
        )
    })
}

fn refs_from_decl(decl: &crate::ast::Decl<'_>, refs: &mut HashSet<String>) {
    match decl {
        crate::ast::Decl::FuncDecl(func) => refs_from_func_decl(func, refs),
        crate::ast::Decl::GenDecl(gen_decl) => refs_from_gen_decl(gen_decl, refs),
    }
}

fn refs_from_gen_decl(gen_decl: &crate::ast::GenDecl<'_>, refs: &mut HashSet<String>) {
    for spec in &gen_decl.specs {
        refs_from_spec(spec, refs);
    }
}

fn refs_from_spec(spec: &crate::ast::Spec<'_>, refs: &mut HashSet<String>) {
    match spec {
        crate::ast::Spec::ImportSpec(_) => {}
        crate::ast::Spec::TypeSpec(type_spec) => {
            refs_from_field_list(type_spec.type_params.as_ref(), refs);
            refs_from_expr(&type_spec.type_, refs);
        }
        crate::ast::Spec::ValueSpec(value_spec) => {
            if let Some(type_expr) = &value_spec.type_ {
                refs_from_expr(type_expr, refs);
            }
            refs_from_exprs(value_spec.values.as_deref().unwrap_or(&[]), refs);
        }
    }
}

fn refs_from_func_decl(func: &crate::ast::FuncDecl<'_>, refs: &mut HashSet<String>) {
    refs_from_field_list(func.recv.as_ref(), refs);
    refs_from_func_type(&func.type_, refs);
    if let Some(body) = &func.body {
        refs_from_block(body, refs);
    }
}

fn refs_from_func_type(func_type: &crate::ast::FuncType<'_>, refs: &mut HashSet<String>) {
    refs_from_field_list(func_type.type_params.as_ref(), refs);
    refs_from_field_list(Some(&func_type.params), refs);
    refs_from_field_list(func_type.results.as_ref(), refs);
}

fn refs_from_field_list(fields: Option<&crate::ast::FieldList<'_>>, refs: &mut HashSet<String>) {
    let Some(fields) = fields else {
        return;
    };
    for field in &fields.list {
        if let Some(type_expr) = &field.type_ {
            refs_from_expr(type_expr, refs);
        }
    }
}

fn refs_from_block(block: &crate::ast::BlockStmt<'_>, refs: &mut HashSet<String>) {
    for stmt in &block.list {
        refs_from_stmt(stmt, refs);
    }
}

fn refs_from_stmt(stmt: &crate::ast::Stmt<'_>, refs: &mut HashSet<String>) {
    match stmt {
        crate::ast::Stmt::AssignStmt(assign) => {
            refs_from_exprs(&assign.lhs, refs);
            refs_from_exprs(&assign.rhs, refs);
        }
        crate::ast::Stmt::BlockStmt(block) => refs_from_block(block, refs),
        crate::ast::Stmt::BranchStmt(_) | crate::ast::Stmt::EmptyStmt(_) => {}
        crate::ast::Stmt::CaseClause(case_clause) => {
            refs_from_exprs(case_clause.list.as_deref().unwrap_or(&[]), refs);
            for stmt in &case_clause.body {
                refs_from_stmt(stmt, refs);
            }
        }
        crate::ast::Stmt::CommClause(comm_clause) => {
            if let Some(comm) = comm_clause.comm.as_deref() {
                refs_from_stmt(comm, refs);
            }
            for stmt in &comm_clause.body {
                refs_from_stmt(stmt, refs);
            }
        }
        crate::ast::Stmt::DeclStmt(decl_stmt) => refs_from_gen_decl(&decl_stmt.decl, refs),
        crate::ast::Stmt::DeferStmt(defer_stmt) => refs_from_call(&defer_stmt.call, refs),
        crate::ast::Stmt::ExprStmt(expr_stmt) => refs_from_expr(&expr_stmt.x, refs),
        crate::ast::Stmt::ForStmt(for_stmt) => {
            if let Some(init) = for_stmt.init.as_deref() {
                refs_from_stmt(init, refs);
            }
            if let Some(cond) = &for_stmt.cond {
                refs_from_expr(cond, refs);
            }
            if let Some(post) = for_stmt.post.as_deref() {
                refs_from_stmt(post, refs);
            }
            refs_from_block(&for_stmt.body, refs);
        }
        crate::ast::Stmt::GoStmt(go_stmt) => refs_from_call(&go_stmt.call, refs),
        crate::ast::Stmt::IfStmt(if_stmt) => {
            if let Some(init) = if_stmt.init.as_ref().as_ref() {
                refs_from_stmt(init, refs);
            }
            refs_from_expr(&if_stmt.cond, refs);
            refs_from_block(&if_stmt.body, refs);
            if let Some(else_stmt) = if_stmt.else_.as_ref().as_ref() {
                refs_from_stmt(else_stmt, refs);
            }
        }
        crate::ast::Stmt::IncDecStmt(inc_dec) => refs_from_expr(&inc_dec.x, refs),
        crate::ast::Stmt::LabeledStmt(labeled) => refs_from_stmt(&labeled.stmt, refs),
        crate::ast::Stmt::RangeStmt(range) => {
            if let Some(key) = &range.key {
                refs_from_expr(key, refs);
            }
            if let Some(value) = &range.value {
                refs_from_expr(value, refs);
            }
            refs_from_expr(&range.x, refs);
            refs_from_block(&range.body, refs);
        }
        crate::ast::Stmt::ReturnStmt(return_stmt) => refs_from_exprs(&return_stmt.results, refs),
        crate::ast::Stmt::SelectStmt(select_stmt) => refs_from_block(&select_stmt.body, refs),
        crate::ast::Stmt::SendStmt(send_stmt) => {
            refs_from_expr(&send_stmt.chan, refs);
            refs_from_expr(&send_stmt.value, refs);
        }
        crate::ast::Stmt::SwitchStmt(switch_stmt) => {
            if let Some(init) = switch_stmt.init.as_deref() {
                refs_from_stmt(init, refs);
            }
            if let Some(tag) = &switch_stmt.tag {
                refs_from_expr(tag, refs);
            }
            refs_from_block(&switch_stmt.body, refs);
        }
        crate::ast::Stmt::TypeSwitchStmt(type_switch) => {
            if let Some(init) = type_switch.init.as_deref() {
                refs_from_stmt(init, refs);
            }
            refs_from_stmt(&type_switch.assign, refs);
            refs_from_block(&type_switch.body, refs);
        }
    }
}

fn refs_from_call(call: &crate::ast::CallExpr<'_>, refs: &mut HashSet<String>) {
    refs_from_expr(&call.fun, refs);
    refs_from_exprs(call.args.as_deref().unwrap_or(&[]), refs);
}

fn refs_from_exprs(exprs: &[crate::ast::Expr<'_>], refs: &mut HashSet<String>) {
    for expr in exprs {
        refs_from_expr(expr, refs);
    }
}

fn refs_from_expr(expr: &crate::ast::Expr<'_>, refs: &mut HashSet<String>) {
    match expr {
        crate::ast::Expr::ArrayType(array) => {
            if let Some(len) = array.len.as_deref() {
                refs_from_expr(len, refs);
            }
            refs_from_expr(&array.elt, refs);
        }
        crate::ast::Expr::BasicLit(_) => {}
        crate::ast::Expr::BinaryExpr(binary) => {
            refs_from_expr(&binary.x, refs);
            refs_from_expr(&binary.y, refs);
        }
        crate::ast::Expr::CallExpr(call) => refs_from_call(call, refs),
        crate::ast::Expr::ChanType(chan) => refs_from_expr(&chan.value, refs),
        crate::ast::Expr::CompositeLit(composite) => {
            if let Some(type_expr) = composite.type_.as_deref() {
                refs_from_expr(type_expr, refs);
            }
            refs_from_exprs(composite.elts.as_deref().unwrap_or(&[]), refs);
        }
        crate::ast::Expr::Ellipsis(ellipsis) => {
            if let Some(elt) = ellipsis.elt.as_deref() {
                refs_from_expr(elt, refs);
            }
        }
        crate::ast::Expr::FuncLit(func_lit) => {
            refs_from_func_type(&func_lit.type_, refs);
            refs_from_block(&func_lit.body, refs);
        }
        crate::ast::Expr::FuncType(func_type) => refs_from_func_type(func_type, refs),
        crate::ast::Expr::Ident(ident) => {
            if ident.name != "_" {
                refs.insert(ident.name.to_string());
            }
        }
        crate::ast::Expr::IndexExpr(index) => {
            refs_from_expr(&index.x, refs);
            refs_from_expr(&index.index, refs);
        }
        crate::ast::Expr::IndexListExpr(index) => {
            refs_from_expr(&index.x, refs);
            refs_from_exprs(&index.indices, refs);
        }
        crate::ast::Expr::InterfaceType(interface) => {
            refs_from_field_list(interface.methods.as_ref(), refs);
        }
        crate::ast::Expr::KeyValueExpr(key_value) => {
            refs_from_expr(&key_value.key, refs);
            refs_from_expr(&key_value.value, refs);
        }
        crate::ast::Expr::MapType(map) => {
            refs_from_expr(&map.key, refs);
            refs_from_expr(&map.value, refs);
        }
        crate::ast::Expr::ParenExpr(paren) => refs_from_expr(&paren.x, refs),
        crate::ast::Expr::SelectorExpr(selector) => refs_from_expr(&selector.x, refs),
        crate::ast::Expr::SliceExpr(slice) => {
            refs_from_expr(&slice.x, refs);
            if let Some(low) = slice.low.as_deref() {
                refs_from_expr(low, refs);
            }
            if let Some(high) = slice.high.as_deref() {
                refs_from_expr(high, refs);
            }
            if let Some(max) = slice.max.as_deref() {
                refs_from_expr(max, refs);
            }
        }
        crate::ast::Expr::StarExpr(star) => refs_from_expr(&star.x, refs),
        crate::ast::Expr::StructType(struct_type) => {
            refs_from_field_list(struct_type.fields.as_ref(), refs);
        }
        crate::ast::Expr::TypeAssertExpr(type_assert) => {
            refs_from_expr(&type_assert.x, refs);
            if let Some(type_expr) = type_assert.type_.as_deref() {
                refs_from_expr(type_expr, refs);
            }
        }
        crate::ast::Expr::UnaryExpr(unary) => refs_from_expr(&unary.x, refs),
    }
}

fn method_refs_from_decl(decl: &crate::ast::Decl<'_>, env: &TypeEnv, refs: &mut HashSet<String>) {
    let mut env = env.clone();
    match decl {
        crate::ast::Decl::FuncDecl(func) => {
            seed_func_decl_method_ref_bindings(func, &mut env);
            if let Some(body) = &func.body {
                method_refs_from_block(body, &mut env, refs);
            }
        }
        crate::ast::Decl::GenDecl(gen_decl) => {
            method_refs_from_gen_decl(gen_decl, &mut env, refs);
        }
    }
}

fn seed_func_decl_method_ref_bindings(func: &crate::ast::FuncDecl<'_>, env: &mut TypeEnv) {
    seed_field_list_method_ref_bindings(func.recv.as_ref(), env);
    seed_field_list_method_ref_bindings(Some(&func.type_.params), env);
    seed_field_list_method_ref_bindings(func.type_.results.as_ref(), env);
}

fn seed_field_list_method_ref_bindings(
    fields: Option<&crate::ast::FieldList<'_>>,
    env: &mut TypeEnv,
) {
    let Some(fields) = fields else {
        return;
    };
    for field in &fields.list {
        let Some(type_expr) = &field.type_ else {
            continue;
        };
        let Some(names) = field.names.as_ref() else {
            continue;
        };
        let ty = crate::compiler::typeinfer::GoType::from_expr(type_expr);
        for name in names {
            if name.name != "_" {
                env.set_var(name.name, ty.clone());
            }
        }
    }
}

fn method_refs_from_gen_decl(
    gen_decl: &crate::ast::GenDecl<'_>,
    env: &mut TypeEnv,
    refs: &mut HashSet<String>,
) {
    for spec in &gen_decl.specs {
        let crate::ast::Spec::ValueSpec(value_spec) = spec else {
            continue;
        };
        if let Some(values) = value_spec.values.as_deref() {
            method_refs_from_exprs(values, env, refs);
        }
        if let Some(type_expr) = &value_spec.type_ {
            let ty = crate::compiler::typeinfer::GoType::from_expr(type_expr);
            for name in &value_spec.names {
                if name.name != "_" {
                    env.set_var(name.name, ty.clone());
                }
            }
        } else if let Some(values) = value_spec.values.as_deref() {
            for (name, value) in value_spec.names.iter().zip(values.iter()) {
                if name.name != "_" {
                    let ty = crate::compiler::typeinfer::GoType::infer_expr(value, env);
                    env.set_var(name.name, ty);
                }
            }
        }
    }
}

fn method_refs_from_block(
    block: &crate::ast::BlockStmt<'_>,
    env: &mut TypeEnv,
    refs: &mut HashSet<String>,
) {
    let mut block_env = env.clone();
    for stmt in &block.list {
        method_refs_from_stmt(stmt, &mut block_env, refs);
    }
}

fn method_refs_from_stmt(
    stmt: &crate::ast::Stmt<'_>,
    env: &mut TypeEnv,
    refs: &mut HashSet<String>,
) {
    match stmt {
        crate::ast::Stmt::AssignStmt(assign) => {
            method_refs_from_exprs(&assign.lhs, env, refs);
            method_refs_from_exprs(&assign.rhs, env, refs);
            if assign.tok == crate::token::Token::DEFINE && assign.lhs.len() == assign.rhs.len() {
                for (lhs, rhs) in assign.lhs.iter().zip(assign.rhs.iter()) {
                    let crate::ast::Expr::Ident(ident) = lhs else {
                        continue;
                    };
                    if ident.name != "_" {
                        let ty = crate::compiler::typeinfer::GoType::infer_expr(rhs, env);
                        env.set_var(ident.name, ty);
                    }
                }
            }
        }
        crate::ast::Stmt::BlockStmt(block) => method_refs_from_block(block, env, refs),
        crate::ast::Stmt::BranchStmt(_) | crate::ast::Stmt::EmptyStmt(_) => {}
        crate::ast::Stmt::CaseClause(case_clause) => {
            method_refs_from_exprs(case_clause.list.as_deref().unwrap_or(&[]), env, refs);
            let mut case_env = env.clone();
            for stmt in &case_clause.body {
                method_refs_from_stmt(stmt, &mut case_env, refs);
            }
        }
        crate::ast::Stmt::CommClause(comm_clause) => {
            let mut comm_env = env.clone();
            if let Some(comm) = comm_clause.comm.as_deref() {
                method_refs_from_stmt(comm, &mut comm_env, refs);
            }
            for stmt in &comm_clause.body {
                method_refs_from_stmt(stmt, &mut comm_env, refs);
            }
        }
        crate::ast::Stmt::DeclStmt(decl_stmt) => {
            method_refs_from_gen_decl(&decl_stmt.decl, env, refs);
        }
        crate::ast::Stmt::DeferStmt(defer_stmt) => {
            method_refs_from_call(&defer_stmt.call, env, refs);
        }
        crate::ast::Stmt::ExprStmt(expr_stmt) => method_refs_from_expr(&expr_stmt.x, env, refs),
        crate::ast::Stmt::ForStmt(for_stmt) => {
            let mut loop_env = env.clone();
            if let Some(init) = for_stmt.init.as_deref() {
                method_refs_from_stmt(init, &mut loop_env, refs);
            }
            if let Some(cond) = &for_stmt.cond {
                method_refs_from_expr(cond, &mut loop_env, refs);
            }
            if let Some(post) = for_stmt.post.as_deref() {
                method_refs_from_stmt(post, &mut loop_env, refs);
            }
            method_refs_from_block(&for_stmt.body, &mut loop_env, refs);
        }
        crate::ast::Stmt::GoStmt(go_stmt) => method_refs_from_call(&go_stmt.call, env, refs),
        crate::ast::Stmt::IfStmt(if_stmt) => {
            let mut if_env = env.clone();
            if let Some(init) = if_stmt.init.as_ref().as_ref() {
                method_refs_from_stmt(init, &mut if_env, refs);
            }
            method_refs_from_expr(&if_stmt.cond, &mut if_env, refs);
            method_refs_from_block(&if_stmt.body, &mut if_env, refs);
            if let Some(else_stmt) = if_stmt.else_.as_ref().as_ref() {
                method_refs_from_stmt(else_stmt, &mut if_env, refs);
            }
        }
        crate::ast::Stmt::IncDecStmt(inc_dec) => method_refs_from_expr(&inc_dec.x, env, refs),
        crate::ast::Stmt::LabeledStmt(labeled) => method_refs_from_stmt(&labeled.stmt, env, refs),
        crate::ast::Stmt::RangeStmt(range) => {
            method_refs_from_expr(&range.x, env, refs);
            let mut range_env = env.clone();
            if range.tok == Some(crate::token::Token::DEFINE) {
                seed_range_method_ref_bindings(range, &mut range_env, env);
            }
            if let Some(key) = &range.key {
                method_refs_from_expr(key, &mut range_env, refs);
            }
            if let Some(value) = &range.value {
                method_refs_from_expr(value, &mut range_env, refs);
            }
            method_refs_from_block(&range.body, &mut range_env, refs);
        }
        crate::ast::Stmt::ReturnStmt(return_stmt) => {
            method_refs_from_exprs(&return_stmt.results, env, refs)
        }
        crate::ast::Stmt::SelectStmt(select_stmt) => {
            method_refs_from_block(&select_stmt.body, env, refs)
        }
        crate::ast::Stmt::SendStmt(send_stmt) => {
            method_refs_from_expr(&send_stmt.chan, env, refs);
            method_refs_from_expr(&send_stmt.value, env, refs);
        }
        crate::ast::Stmt::SwitchStmt(switch_stmt) => {
            let mut switch_env = env.clone();
            if let Some(init) = switch_stmt.init.as_deref() {
                method_refs_from_stmt(init, &mut switch_env, refs);
            }
            if let Some(tag) = &switch_stmt.tag {
                method_refs_from_expr(tag, &mut switch_env, refs);
            }
            method_refs_from_block(&switch_stmt.body, &mut switch_env, refs);
        }
        crate::ast::Stmt::TypeSwitchStmt(type_switch) => {
            let mut switch_env = env.clone();
            if let Some(init) = type_switch.init.as_deref() {
                method_refs_from_stmt(init, &mut switch_env, refs);
            }
            method_refs_from_stmt(&type_switch.assign, &mut switch_env, refs);
            method_refs_from_block(&type_switch.body, &mut switch_env, refs);
        }
    }
}

fn seed_range_method_ref_bindings(
    range: &crate::ast::RangeStmt<'_>,
    range_env: &mut TypeEnv,
    outer_env: &TypeEnv,
) {
    let container_ty = outer_env.resolve_alias(&crate::compiler::typeinfer::GoType::infer_expr(
        &range.x, outer_env,
    ));
    let (key_ty, value_ty) = match container_ty {
        crate::compiler::typeinfer::GoType::Slice(elem)
        | crate::compiler::typeinfer::GoType::Array(elem) => {
            (crate::compiler::typeinfer::GoType::Int, *elem)
        }
        crate::compiler::typeinfer::GoType::Map(key, value) => (*key, *value),
        crate::compiler::typeinfer::GoType::String => (
            crate::compiler::typeinfer::GoType::Int,
            crate::compiler::typeinfer::GoType::Int32,
        ),
        _ => (
            crate::compiler::typeinfer::GoType::Unknown,
            crate::compiler::typeinfer::GoType::Unknown,
        ),
    };
    if let Some(crate::ast::Expr::Ident(ident)) = &range.key
        && ident.name != "_"
    {
        range_env.set_var(ident.name, key_ty);
    }
    if let Some(crate::ast::Expr::Ident(ident)) = &range.value
        && ident.name != "_"
    {
        range_env.set_var(ident.name, value_ty);
    }
}

fn method_refs_from_call(
    call: &crate::ast::CallExpr<'_>,
    env: &mut TypeEnv,
    refs: &mut HashSet<String>,
) {
    if let crate::ast::Expr::SelectorExpr(selector) = call.fun.as_ref()
        && let Some(receiver) = receiver_name_for_method_expr(&selector.x, env)
    {
        refs.insert(format!("{receiver}::{}", selector.sel.name));
    }
    method_refs_from_expr(&call.fun, env, refs);
    method_refs_from_exprs(call.args.as_deref().unwrap_or(&[]), env, refs);
}

fn method_refs_from_exprs(
    exprs: &[crate::ast::Expr<'_>],
    env: &mut TypeEnv,
    refs: &mut HashSet<String>,
) {
    for expr in exprs {
        method_refs_from_expr(expr, env, refs);
    }
}

fn method_refs_from_expr(
    expr: &crate::ast::Expr<'_>,
    env: &mut TypeEnv,
    refs: &mut HashSet<String>,
) {
    match expr {
        crate::ast::Expr::ArrayType(array) => {
            if let Some(len) = array.len.as_deref() {
                method_refs_from_expr(len, env, refs);
            }
            method_refs_from_expr(&array.elt, env, refs);
        }
        crate::ast::Expr::BasicLit(_) | crate::ast::Expr::Ident(_) => {}
        crate::ast::Expr::BinaryExpr(binary) => {
            method_refs_from_expr(&binary.x, env, refs);
            method_refs_from_expr(&binary.y, env, refs);
        }
        crate::ast::Expr::CallExpr(call) => method_refs_from_call(call, env, refs),
        crate::ast::Expr::ChanType(chan) => method_refs_from_expr(&chan.value, env, refs),
        crate::ast::Expr::CompositeLit(composite) => {
            if let Some(type_expr) = composite.type_.as_deref() {
                method_refs_from_expr(type_expr, env, refs);
            }
            method_refs_from_exprs(composite.elts.as_deref().unwrap_or(&[]), env, refs);
        }
        crate::ast::Expr::Ellipsis(ellipsis) => {
            if let Some(elt) = ellipsis.elt.as_deref() {
                method_refs_from_expr(elt, env, refs);
            }
        }
        crate::ast::Expr::FuncLit(func_lit) => {
            let mut func_env = env.clone();
            seed_field_list_method_ref_bindings(Some(&func_lit.type_.params), &mut func_env);
            seed_field_list_method_ref_bindings(func_lit.type_.results.as_ref(), &mut func_env);
            method_refs_from_block(&func_lit.body, &mut func_env, refs);
        }
        crate::ast::Expr::FuncType(_) | crate::ast::Expr::InterfaceType(_) => {}
        crate::ast::Expr::IndexExpr(index) => {
            method_refs_from_expr(&index.x, env, refs);
            method_refs_from_expr(&index.index, env, refs);
        }
        crate::ast::Expr::IndexListExpr(index) => {
            method_refs_from_expr(&index.x, env, refs);
            method_refs_from_exprs(&index.indices, env, refs);
        }
        crate::ast::Expr::KeyValueExpr(key_value) => {
            method_refs_from_expr(&key_value.key, env, refs);
            method_refs_from_expr(&key_value.value, env, refs);
        }
        crate::ast::Expr::MapType(map) => {
            method_refs_from_expr(&map.key, env, refs);
            method_refs_from_expr(&map.value, env, refs);
        }
        crate::ast::Expr::ParenExpr(paren) => method_refs_from_expr(&paren.x, env, refs),
        crate::ast::Expr::SelectorExpr(selector) => method_refs_from_expr(&selector.x, env, refs),
        crate::ast::Expr::SliceExpr(slice) => {
            method_refs_from_expr(&slice.x, env, refs);
            if let Some(low) = slice.low.as_deref() {
                method_refs_from_expr(low, env, refs);
            }
            if let Some(high) = slice.high.as_deref() {
                method_refs_from_expr(high, env, refs);
            }
            if let Some(max) = slice.max.as_deref() {
                method_refs_from_expr(max, env, refs);
            }
        }
        crate::ast::Expr::StarExpr(star) => method_refs_from_expr(&star.x, env, refs),
        crate::ast::Expr::StructType(struct_type) => {
            if let Some(fields) = struct_type.fields.as_ref() {
                for field in &fields.list {
                    if let Some(type_expr) = &field.type_ {
                        method_refs_from_expr(type_expr, env, refs);
                    }
                }
            }
        }
        crate::ast::Expr::TypeAssertExpr(type_assert) => {
            method_refs_from_expr(&type_assert.x, env, refs);
            if let Some(type_expr) = type_assert.type_.as_deref() {
                method_refs_from_expr(type_expr, env, refs);
            }
        }
        crate::ast::Expr::UnaryExpr(unary) => method_refs_from_expr(&unary.x, env, refs),
    }
}

fn receiver_name_for_method_expr(expr: &crate::ast::Expr<'_>, env: &TypeEnv) -> Option<String> {
    let ty = crate::compiler::typeinfer::GoType::infer_expr(expr, env);
    receiver_name_from_go_type(&ty)
}

fn receiver_name_from_go_type(ty: &crate::compiler::typeinfer::GoType) -> Option<String> {
    match ty {
        crate::compiler::typeinfer::GoType::Named(name)
        | crate::compiler::typeinfer::GoType::Interface(name) => Some(local_receiver_name(name)),
        crate::compiler::typeinfer::GoType::Pointer(inner) => receiver_name_from_go_type(inner),
        _ => None,
    }
}

fn local_receiver_name(name: &str) -> String {
    name.rsplit('.').next().unwrap_or(name).to_string()
}

fn value_refs_from_decl(decl: &crate::ast::Decl<'_>, refs: &mut HashSet<String>) {
    match decl {
        crate::ast::Decl::FuncDecl(func) => {
            if let Some(body) = &func.body {
                value_refs_from_block(body, refs);
            }
        }
        crate::ast::Decl::GenDecl(gen_decl) => {
            for spec in &gen_decl.specs {
                if let crate::ast::Spec::ValueSpec(value_spec) = spec {
                    value_refs_from_exprs(value_spec.values.as_deref().unwrap_or(&[]), refs);
                }
            }
        }
    }
}

fn value_field_refs_from_decl(
    decl: &crate::ast::Decl<'_>,
    value_reachable: &HashSet<String>,
    refs: &mut HashSet<String>,
) {
    let crate::ast::Decl::GenDecl(gen_decl) = decl else {
        return;
    };
    for spec in &gen_decl.specs {
        let crate::ast::Spec::TypeSpec(type_spec) = spec else {
            continue;
        };
        let Some(type_name) = type_spec.name.as_ref().map(|name| name.name) else {
            continue;
        };
        if !value_reachable.contains(type_name) {
            continue;
        }
        let crate::ast::Expr::StructType(struct_type) = &type_spec.type_ else {
            continue;
        };
        let Some(fields) = struct_type.fields.as_ref() else {
            continue;
        };
        for field in &fields.list {
            if let Some(type_expr) = &field.type_ {
                refs_from_expr(type_expr, refs);
            }
        }
    }
}

fn value_refs_from_block(block: &crate::ast::BlockStmt<'_>, refs: &mut HashSet<String>) {
    for stmt in &block.list {
        value_refs_from_stmt(stmt, refs);
    }
}

fn value_refs_from_stmt(stmt: &crate::ast::Stmt<'_>, refs: &mut HashSet<String>) {
    match stmt {
        crate::ast::Stmt::AssignStmt(assign) => {
            value_refs_from_exprs(&assign.lhs, refs);
            value_refs_from_exprs(&assign.rhs, refs);
        }
        crate::ast::Stmt::BlockStmt(block) => value_refs_from_block(block, refs),
        crate::ast::Stmt::BranchStmt(_) | crate::ast::Stmt::EmptyStmt(_) => {}
        crate::ast::Stmt::CaseClause(case_clause) => {
            value_refs_from_exprs(case_clause.list.as_deref().unwrap_or(&[]), refs);
            for stmt in &case_clause.body {
                value_refs_from_stmt(stmt, refs);
            }
        }
        crate::ast::Stmt::CommClause(comm_clause) => {
            if let Some(comm) = comm_clause.comm.as_deref() {
                value_refs_from_stmt(comm, refs);
            }
            for stmt in &comm_clause.body {
                value_refs_from_stmt(stmt, refs);
            }
        }
        crate::ast::Stmt::DeclStmt(decl_stmt) => {
            for spec in &decl_stmt.decl.specs {
                if let crate::ast::Spec::ValueSpec(value_spec) = spec {
                    value_refs_from_exprs(value_spec.values.as_deref().unwrap_or(&[]), refs);
                }
            }
        }
        crate::ast::Stmt::DeferStmt(defer_stmt) => value_refs_from_call(&defer_stmt.call, refs),
        crate::ast::Stmt::ExprStmt(expr_stmt) => value_refs_from_expr(&expr_stmt.x, refs),
        crate::ast::Stmt::ForStmt(for_stmt) => {
            if let Some(init) = for_stmt.init.as_deref() {
                value_refs_from_stmt(init, refs);
            }
            if let Some(cond) = &for_stmt.cond {
                value_refs_from_expr(cond, refs);
            }
            if let Some(post) = for_stmt.post.as_deref() {
                value_refs_from_stmt(post, refs);
            }
            value_refs_from_block(&for_stmt.body, refs);
        }
        crate::ast::Stmt::GoStmt(go_stmt) => value_refs_from_call(&go_stmt.call, refs),
        crate::ast::Stmt::IfStmt(if_stmt) => {
            if let Some(init) = if_stmt.init.as_ref().as_ref() {
                value_refs_from_stmt(init, refs);
            }
            value_refs_from_expr(&if_stmt.cond, refs);
            value_refs_from_block(&if_stmt.body, refs);
            if let Some(else_stmt) = if_stmt.else_.as_ref().as_ref() {
                value_refs_from_stmt(else_stmt, refs);
            }
        }
        crate::ast::Stmt::IncDecStmt(inc_dec) => value_refs_from_expr(&inc_dec.x, refs),
        crate::ast::Stmt::LabeledStmt(labeled) => value_refs_from_stmt(&labeled.stmt, refs),
        crate::ast::Stmt::RangeStmt(range) => {
            if let Some(key) = &range.key {
                value_refs_from_expr(key, refs);
            }
            if let Some(value) = &range.value {
                value_refs_from_expr(value, refs);
            }
            value_refs_from_expr(&range.x, refs);
            value_refs_from_block(&range.body, refs);
        }
        crate::ast::Stmt::ReturnStmt(return_stmt) => {
            value_refs_from_exprs(&return_stmt.results, refs)
        }
        crate::ast::Stmt::SelectStmt(select_stmt) => value_refs_from_block(&select_stmt.body, refs),
        crate::ast::Stmt::SendStmt(send_stmt) => {
            value_refs_from_expr(&send_stmt.chan, refs);
            value_refs_from_expr(&send_stmt.value, refs);
        }
        crate::ast::Stmt::SwitchStmt(switch_stmt) => {
            if let Some(init) = switch_stmt.init.as_deref() {
                value_refs_from_stmt(init, refs);
            }
            if let Some(tag) = &switch_stmt.tag {
                value_refs_from_expr(tag, refs);
            }
            value_refs_from_block(&switch_stmt.body, refs);
        }
        crate::ast::Stmt::TypeSwitchStmt(type_switch) => {
            if let Some(init) = type_switch.init.as_deref() {
                value_refs_from_stmt(init, refs);
            }
            value_refs_from_stmt(&type_switch.assign, refs);
            for stmt in &type_switch.body.list {
                if let crate::ast::Stmt::CaseClause(case_clause) = stmt {
                    for stmt in &case_clause.body {
                        value_refs_from_stmt(stmt, refs);
                    }
                }
            }
        }
    }
}

fn value_refs_from_call(call: &crate::ast::CallExpr<'_>, refs: &mut HashSet<String>) {
    value_refs_from_expr(&call.fun, refs);
    value_refs_from_exprs(call.args.as_deref().unwrap_or(&[]), refs);
}

fn value_refs_from_exprs(exprs: &[crate::ast::Expr<'_>], refs: &mut HashSet<String>) {
    for expr in exprs {
        value_refs_from_expr(expr, refs);
    }
}

fn value_refs_from_expr(expr: &crate::ast::Expr<'_>, refs: &mut HashSet<String>) {
    match expr {
        crate::ast::Expr::BasicLit(_) => {}
        crate::ast::Expr::BinaryExpr(binary) => {
            value_refs_from_expr(&binary.x, refs);
            value_refs_from_expr(&binary.y, refs);
        }
        crate::ast::Expr::CallExpr(call) => value_refs_from_call(call, refs),
        crate::ast::Expr::CompositeLit(composite) => {
            if let Some(type_expr) = composite.type_.as_deref() {
                refs_from_expr(type_expr, refs);
            }
            value_refs_from_exprs(composite.elts.as_deref().unwrap_or(&[]), refs);
        }
        crate::ast::Expr::Ellipsis(ellipsis) => {
            if let Some(elt) = ellipsis.elt.as_deref() {
                value_refs_from_expr(elt, refs);
            }
        }
        crate::ast::Expr::FuncLit(func_lit) => value_refs_from_block(&func_lit.body, refs),
        crate::ast::Expr::Ident(ident) => {
            if ident.name != "_" {
                refs.insert(ident.name.to_string());
            }
        }
        crate::ast::Expr::IndexExpr(index) => {
            value_refs_from_expr(&index.x, refs);
            value_refs_from_expr(&index.index, refs);
        }
        crate::ast::Expr::IndexListExpr(index) => {
            value_refs_from_expr(&index.x, refs);
            value_refs_from_exprs(&index.indices, refs);
        }
        crate::ast::Expr::KeyValueExpr(key_value) => {
            value_refs_from_expr(&key_value.key, refs);
            value_refs_from_expr(&key_value.value, refs);
        }
        crate::ast::Expr::ParenExpr(paren) => value_refs_from_expr(&paren.x, refs),
        crate::ast::Expr::SelectorExpr(selector) => value_refs_from_expr(&selector.x, refs),
        crate::ast::Expr::SliceExpr(slice) => {
            value_refs_from_expr(&slice.x, refs);
            if let Some(low) = slice.low.as_deref() {
                value_refs_from_expr(low, refs);
            }
            if let Some(high) = slice.high.as_deref() {
                value_refs_from_expr(high, refs);
            }
            if let Some(max) = slice.max.as_deref() {
                value_refs_from_expr(max, refs);
            }
        }
        crate::ast::Expr::StarExpr(star) => value_refs_from_expr(&star.x, refs),
        crate::ast::Expr::TypeAssertExpr(type_assert) => {
            value_refs_from_expr(&type_assert.x, refs);
        }
        crate::ast::Expr::UnaryExpr(unary) => value_refs_from_expr(&unary.x, refs),
        crate::ast::Expr::ArrayType(_)
        | crate::ast::Expr::ChanType(_)
        | crate::ast::Expr::FuncType(_)
        | crate::ast::Expr::InterfaceType(_)
        | crate::ast::Expr::MapType(_)
        | crate::ast::Expr::StructType(_) => {}
    }
}

fn package_import_renames(
    parsed_files: &[(&str, crate::ast::File<'_>)],
) -> BTreeMap<String, String> {
    let mut rewrites = BTreeMap::new();
    for (_, ast) in parsed_files {
        for import in ast.imports() {
            let import_path = import.path.value.trim_matches('"');
            if !is_known(import_path) {
                continue;
            }
            let mod_name = module_name(import_path);
            let Some(local_name) = import_local_name(import) else {
                continue;
            };
            if local_name != mod_name {
                rewrites.insert(local_name, mod_name);
            }
        }
    }
    rewrites
}

fn package_import_path_by_module(
    parsed_files: &[(&str, crate::ast::File<'_>)],
) -> HashMap<String, String> {
    let mut imports = HashMap::new();
    for (_, ast) in parsed_files {
        for import in ast.imports() {
            let import_path = import.path.value.trim_matches('"');
            if is_known(import_path) {
                imports.insert(module_name(import_path), import_path.to_string());
                if let Some(local_name) = import_local_name(import) {
                    imports.insert(local_name, import_path.to_string());
                }
            }
        }
    }
    imports
}

fn used_imports_from_items(
    items: &mut [syn::Item],
    import_path_by_module: &HashMap<String, String>,
) -> Vec<String> {
    use syn::visit_mut::VisitMut;

    struct UsedImportCollector<'a> {
        import_path_by_module: &'a HashMap<String, String>,
        used: HashSet<String>,
    }

    impl VisitMut for UsedImportCollector<'_> {
        fn visit_path_mut(&mut self, path: &mut syn::Path) {
            syn::visit_mut::visit_path_mut(self, path);
            if path.segments.len() < 2 {
                return;
            }
            let Some(first) = path.segments.first().map(|seg| seg.ident.to_string()) else {
                return;
            };
            if let Some(import_path) = self.import_path_by_module.get(&first) {
                self.used.insert(import_path.clone());
            }
        }
    }

    let mut collector = UsedImportCollector {
        import_path_by_module,
        used: HashSet::new(),
    };
    for item in items {
        collector.visit_item_mut(item);
    }

    let mut used: Vec<_> = collector.used.into_iter().collect();
    used.sort();
    used
}

fn import_local_name(import: &crate::ast::ImportSpec<'_>) -> Option<String> {
    if let Some(name) = &import.name {
        if name.name == "." || name.name == "_" {
            return None;
        }
        return Some(name.name.to_string());
    }

    let import_path = import.path.value.trim_matches('"');
    scan_type_env(import_path)
        .map(|(package_name, _)| package_name)
        .or_else(|| import_path.rsplit('/').next().map(str::to_string))
}

fn prefix_crate_paths(items: &mut [syn::Item], module_refs: &HashSet<String>) {
    use syn::visit_mut::VisitMut;

    struct CratePrefixer<'a> {
        module_refs: &'a HashSet<String>,
    }

    impl VisitMut for CratePrefixer<'_> {
        fn visit_path_mut(&mut self, path: &mut syn::Path) {
            syn::visit_mut::visit_path_mut(self, path);
            if path.leading_colon.is_some() || path.segments.len() < 2 {
                return;
            }
            let Some(first) = path.segments.first().map(|seg| seg.ident.to_string()) else {
                return;
            };
            if first == "builtin" || self.module_refs.contains(&first) {
                path.segments.insert(
                    0,
                    syn::PathSegment {
                        ident: syn::Ident::new("crate", proc_macro2::Span::mixed_site()),
                        arguments: syn::PathArguments::None,
                    },
                );
            }
        }
    }

    let mut prefixer = CratePrefixer { module_refs };
    for item in items.iter_mut() {
        prefixer.visit_item_mut(item);
    }
}

fn is_rust_keyword(value: &str) -> bool {
    matches!(
        value,
        "as" | "break"
            | "const"
            | "continue"
            | "crate"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "async"
            | "await"
            | "dyn"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::typeinfer::{GoType, TypeKind};
    use quote::ToTokens;

    #[test]
    fn refresh_top_level_vars_with_imports_promotes_imported_receiver_facts() {
        let common = crate::parser::parse_file(
            "common.go",
            r#"
package tar

import "internal/godebug"

var tarinsecurepath = godebug.New("tarinsecurepath")
"#,
        )
        .unwrap();
        let reader = crate::parser::parse_file(
            "reader.go",
            r#"
package tar

func read() string {
	return tarinsecurepath.Value()
}
"#,
        )
        .unwrap();
        let mut godebug_env = TypeEnv::new();
        godebug_env.set_type_kind("Setting", TypeKind::Struct);
        godebug_env.set_func(
            "New",
            vec![GoType::Pointer(Box::new(GoType::Named(
                "Setting".to_string(),
            )))],
        );
        godebug_env.set_func("Setting.Value", vec![GoType::String]);
        godebug_env.set_pointer_receiver_method("Setting.Value");
        let imported_type_envs = BTreeMap::from([(
            "internal/godebug".to_string(),
            crate::compiler::PackageFacts::new("godebug".to_string(), godebug_env),
        )]);
        let mut package_env = TypeEnv::new();
        let files = [&common, &reader];
        package_env.scan_files(&files);

        refresh_top_level_vars_with_imports(&mut package_env, &files, &imported_type_envs);

        assert_eq!(
            package_env.get_top_level_var("tarinsecurepath"),
            Some(GoType::Pointer(Box::new(GoType::Named(
                "godebug.Setting".to_string()
            ))))
        );
        assert!(package_env.has_func("godebug.Setting.Value"));
        assert!(package_env.method_has_pointer_receiver("godebug.Setting.Value"));
        assert!(!package_env.has_func("godebug.New"));
    }

    #[test]
    fn reachable_names_include_typed_local_and_named_slice_method_calls() {
        let file = crate::parser::parse_file(
            "pkg.go",
            r#"
package pkg

type parser struct{}

func (*parser) parseString() string {
	return ""
}

type sparseArray []byte

func (s sparseArray) entry() int {
	return 0
}

func root(s sparseArray) {
	var p parser
	_ = p.parseString()
	_ = s.entry()
}
"#,
        )
        .unwrap();
        let parsed = vec![("pkg.go", file)];
        let roots = HashSet::from(["root".to_string()]);

        let reachable = reachable_package_names(&parsed, &roots);

        assert!(reachable.contains("parser::parseString"), "{reachable:?}");
        assert!(reachable.contains("sparseArray::entry"), "{reachable:?}");
    }

    #[test]
    fn reachable_names_include_methods_called_on_method_return_locals() {
        let file = crate::parser::parse_file(
            "pkg.go",
            r#"
package pkg

type block []byte

func (b *block) toGNU() *headerGNU {
	return nil
}

type headerGNU []byte

func (h *headerGNU) sparse() sparseArray {
	return nil
}

type sparseArray []byte

func (s sparseArray) maxEntries() int {
	return 0
}

func root(blk *block) {
	s := blk.toGNU().sparse()
	_ = s.maxEntries()
}
"#,
        )
        .unwrap();
        let parsed = vec![("pkg.go", file)];
        let roots = HashSet::from(["root".to_string()]);

        let reachable = reachable_package_names(&parsed, &roots);

        assert!(reachable.contains("block::toGNU"), "{reachable:?}");
        assert!(reachable.contains("headerGNU::sparse"), "{reachable:?}");
        assert!(
            reachable.contains("sparseArray::maxEntries"),
            "{reachable:?}"
        );
    }

    #[test]
    fn dedupe_use_items_matches_use_trees_structurally() {
        let duplicate_group: syn::ItemUse =
            syn::parse_quote! { use crate::io::{Read, Write as W}; };
        let duplicate_private: syn::ItemUse = syn::parse_quote! { use crate::io::Read; };
        let distinct_public: syn::ItemUse = syn::parse_quote! { pub use crate::io::Read; };
        let mut items: Vec<syn::Item> = vec![
            syn::parse_quote! { use crate::io::{Read, Write as W}; },
            syn::parse_quote! { use crate::io::{Read, Write as W}; },
            syn::parse_quote! { use crate::io::Read; },
            syn::parse_quote! { fn keep() {} },
            syn::parse_quote! { use crate::io::Read; },
            syn::parse_quote! { pub use crate::io::Read; },
        ];

        dedupe_use_items(&mut items);

        assert_eq!(items.len(), 4);
        assert!(
            items
                .get(2)
                .is_some_and(|item| matches!(item, syn::Item::Fn(_)))
        );
        assert_eq!(matching_use_item_count(&items, &duplicate_group), 1);
        assert_eq!(matching_use_item_count(&items, &duplicate_private), 1);
        assert_eq!(matching_use_item_count(&items, &distinct_public), 1);
    }

    fn matching_use_item_count(items: &[syn::Item], expected: &syn::ItemUse) -> usize {
        items
            .iter()
            .filter_map(|item| match item {
                syn::Item::Use(item_use) => Some(item_use),
                _ => None,
            })
            .filter(|item_use| use_items_match(item_use, expected))
            .count()
    }

    #[test]
    fn scan_type_env_preserves_syscall_errno_constant_type()
    -> Result<(), Box<dyn std::error::Error>> {
        let (package_name, env) =
            scan_type_env("syscall").ok_or_else(|| std::io::Error::other("syscall type env"))?;

        assert_eq!(package_name, "syscall");
        assert_eq!(
            env.get_var("ENOENT"),
            Some(crate::compiler::typeinfer::GoType::Named(
                "Errno".to_string()
            ))
        );
        assert_eq!(
            env.get_field_type("Stat_t", "Atimespec"),
            crate::compiler::typeinfer::GoType::Named("Timespec".to_string())
        );
        assert_eq!(
            env.get_func_returns("Timespec.Unix"),
            vec![
                crate::compiler::typeinfer::GoType::Int64,
                crate::compiler::typeinfer::GoType::Int64,
            ]
        );
        Ok(())
    }

    #[test]
    fn reachable_names_include_instantiated_field_type_arguments()
    -> Result<(), Box<dyn std::error::Error>> {
        let file = crate::parser::parse_file(
            "fixture.go",
            r#"
                package fixture

                import "sync/atomic"

                type Matcher struct {
                    dedup atomic.Pointer[dedup]
                }

                type dedup struct {
                    recent [128][4]uint64
                }

                func (d *dedup) seenLossy(h uint64) bool {
                    cache := &d.recent[uint(h)%uint(len(d.recent))]
                    for i := 0; i < len(cache); i++ {
                    }
                    return false
                }
            "#,
        )?;
        let parsed_files = vec![("fixture.go", file)];
        let roots = HashSet::from(["Matcher".to_string()]);
        let reachable = reachable_package_names(&parsed_files, &roots);

        assert!(reachable.contains("Matcher"));
        assert!(reachable.contains("dedup"));
        Ok(())
    }

    #[test]
    fn reachable_names_include_package_vars_reached_from_transitive_functions()
    -> Result<(), Box<dyn std::error::Error>> {
        let entry_file = crate::parser::parse_file(
            "entry.go",
            r#"
                package fixture

                func Entry() bool {
                    return helper()
                }

                func helper() bool {
                    return !flag
                }
            "#,
        )?;
        let var_file = crate::parser::parse_file(
            "vars.go",
            r#"
                package fixture

                var flag = true
            "#,
        )?;
        let parsed_files = vec![("entry.go", entry_file), ("vars.go", var_file)];
        let roots = HashSet::from(["Entry".to_string()]);
        let reachable = reachable_package_names(&parsed_files, &roots);

        assert!(reachable.contains("Entry"), "{reachable:?}");
        assert!(reachable.contains("helper"), "{reachable:?}");
        assert!(reachable.contains("flag"), "{reachable:?}");
        Ok(())
    }

    #[test]
    fn reachable_names_include_type_switch_case_interface_methods()
    -> Result<(), Box<dyn std::error::Error>> {
        let file = crate::parser::parse_file(
            "fixture.go",
            r#"
                package fixture

                type I interface {
                    A() int
                    B() int
                }

                type T struct{}

                func (T) A() int { return 1 }
                func (T) B() int { return 2 }

                func Use(i I) int {
                    switch i.(type) {
                    case T:
                        return 1
                    default:
                        return 0
                    }
                }
            "#,
        )?;
        let parsed_files = vec![("fixture.go", file)];
        let roots = HashSet::from(["Use".to_string()]);
        let reachable = reachable_package_names(&parsed_files, &roots);

        for expected in ["I", "T", "T::A", "T::B"] {
            assert!(
                reachable.contains(expected),
                "{expected} missing from {reachable:?}"
            );
        }
        Ok(())
    }

    #[test]
    fn reachable_names_include_internal_strconv_appendfloat_package_var()
    -> Result<(), Box<dyn std::error::Error>> {
        let files =
            package_files("internal/strconv").ok_or_else(|| std::io::Error::other("files"))?;
        let mut parsed_files = Vec::new();
        for (filename, content) in files.iter() {
            parsed_files.push((*filename, crate::parser::parse_file(filename, content)?));
        }
        let roots = HashSet::from(["AppendFloat".to_string()]);
        let reachable = reachable_package_names(&parsed_files, &roots);

        assert!(reachable.contains("AppendFloat"), "{reachable:?}");
        assert!(reachable.contains("genericFtoa"), "{reachable:?}");
        assert!(reachable.contains("optimize"), "{reachable:?}");
        Ok(())
    }

    #[test]
    fn reachable_names_include_reflect_mapiter_copyval() -> Result<(), Box<dyn std::error::Error>> {
        let files = package_files("reflect").ok_or_else(|| std::io::Error::other("files"))?;
        let mut parsed_files = Vec::new();
        for (filename, content) in files.iter() {
            parsed_files.push((*filename, crate::parser::parse_file(filename, content)?));
        }
        let roots = HashSet::from(["MapIter".to_string()]);
        let reachable = reachable_package_names(&parsed_files, &roots);

        assert!(reachable.contains("MapIter"), "{reachable:?}");
        assert!(reachable.contains("copyVal"), "{reachable:?}");
        let module = resolve_with_roots("reflect", &roots)
            .ok_or_else(|| std::io::Error::other("resolve reflect"))?;
        let tokens = module.to_token_stream().to_string();
        assert!(tokens.contains("fn copyVal"), "{tokens}");
        Ok(())
    }

    #[test]
    fn reachable_names_include_context_interface_receiver_methods()
    -> Result<(), Box<dyn std::error::Error>> {
        let files = package_files("context").ok_or_else(|| std::io::Error::other("files"))?;
        let mut parsed_files = Vec::new();
        for (filename, content) in files.iter() {
            parsed_files.push((*filename, crate::parser::parse_file(filename, content)?));
        }
        let roots = HashSet::from([
            "Background".to_string(),
            "TODO".to_string(),
            "Context".to_string(),
            "Context::Deadline".to_string(),
            "Context::Done".to_string(),
            "Context::Err".to_string(),
            "Context::Value".to_string(),
            "WithValue".to_string(),
        ]);
        let reachable = reachable_package_names(&parsed_files, &roots);

        for expected in [
            "backgroundCtx",
            "todoCtx",
            "emptyCtx",
            "valueCtx",
            "cancelCtx",
            "timerCtx",
            "withoutCancelCtx",
            "withoutCancelCtx::Deadline",
            "withoutCancelCtx::Done",
            "withoutCancelCtx::Err",
            "withoutCancelCtx::Value",
        ] {
            assert!(
                reachable.contains(expected),
                "{expected} missing from {reachable:?}"
            );
        }
        let module = resolve_with_roots("context", &roots)
            .ok_or_else(|| std::io::Error::other("resolve context"))?;
        let tokens = module.to_token_stream().to_string();
        for expected in [
            "impl emptyCtx",
            "pub fn Deadline",
            "pub fn Done",
            "pub fn Err",
            "pub fn Value",
            "impl Context for backgroundCtx",
            "impl Context for todoCtx",
            "impl < '__gors : 'static > Context for crate :: builtin :: GorsPtr < valueCtx < '__gors > >",
            "fn value",
        ] {
            assert!(
                tokens.contains(expected),
                "{expected} missing from {tokens}"
            );
        }
        assert!(!tokens.contains("impl stringer for timerCtx"), "{tokens}");
        Ok(())
    }

    #[test]
    fn reachable_names_include_context_package_init_dependencies()
    -> Result<(), Box<dyn std::error::Error>> {
        let files = package_files("context").ok_or_else(|| std::io::Error::other("files"))?;
        let mut parsed_files = Vec::new();
        for (filename, content) in files.iter() {
            parsed_files.push((*filename, crate::parser::parse_file(filename, content)?));
        }
        let roots = HashSet::from(["WithCancel".to_string()]);
        let reachable = reachable_package_names(&parsed_files, &roots);

        for expected in ["init", "closedchan"] {
            assert!(
                reachable.contains(expected),
                "{expected} missing from {reachable:?}"
            );
        }

        let module = resolve_with_roots("context", &roots)
            .ok_or_else(|| std::io::Error::other("resolve context"))?;
        let tokens = module.to_token_stream().to_string();

        for expected in ["pub fn __gors_init", "closedchan", "close"] {
            assert!(
                tokens.contains(expected),
                "{expected} missing from {tokens}"
            );
        }
        Ok(())
    }

    #[test]
    fn resolve_roots_merge_multiple_package_init_functions()
    -> Result<(), Box<dyn std::error::Error>> {
        let roots = HashSet::from(["Args".to_string()]);
        let module =
            resolve_with_roots("os", &roots).ok_or_else(|| std::io::Error::other("resolve os"))?;
        let tokens = module.to_token_stream().to_string();

        assert_eq!(tokens.matches("pub fn __gors_init").count(), 1, "{tokens}");
        assert!(tokens.contains("runtime_args"), "{tokens}");
        Ok(())
    }

    #[test]
    fn runtime_gomaxprocs_resolves_as_runtime_primitive() -> Result<(), Box<dyn std::error::Error>>
    {
        let roots = HashSet::from(["GOMAXPROCS".to_string()]);
        let module = resolve_with_roots("runtime", &roots)
            .ok_or_else(|| std::io::Error::other("resolve runtime"))?;
        let tokens = module.to_token_stream().to_string();

        assert!(tokens.contains("pub fn GOMAXPROCS"));
        assert!(!tokens.contains("sched"));
        assert!(collect_resolved_imports("runtime", &roots).is_empty());
        Ok(())
    }

    #[test]
    fn resolve_roots_retain_package_vars_reached_through_functions()
    -> Result<(), Box<dyn std::error::Error>> {
        let roots = HashSet::from(["AppendFloat".to_string()]);
        let module = resolve_with_roots("internal/strconv", &roots)
            .ok_or_else(|| std::io::Error::other("resolve internal/strconv"))?;
        let tokens = module.to_token_stream().to_string();

        assert!(tokens.contains("pub fn AppendFloat"), "{tokens}");
        assert!(tokens.contains("fn genericFtoa"), "{tokens}");
        assert!(tokens.contains("static optimize_"), "{tokens}");
        Ok(())
    }

    #[test]
    fn resolve_roots_retain_private_syscall_helpers_reached_from_public_functions()
    -> Result<(), Box<dyn std::error::Error>> {
        let roots = HashSet::from([
            "Close".to_string(),
            "ENOENT".to_string(),
            "Open".to_string(),
            "O_RDONLY".to_string(),
            "Read".to_string(),
            "Seek".to_string(),
        ]);
        let module = resolve_with_roots("syscall", &roots)
            .ok_or_else(|| std::io::Error::other("resolve syscall"))?;
        let tokens = module.to_token_stream().to_string();

        assert!(tokens.contains("pub fn Close"), "{tokens}");
        assert!(tokens.contains("pub fn Open"), "{tokens}");
        assert!(tokens.contains("pub fn Read"), "{tokens}");
        assert!(tokens.contains("fn read"), "{tokens}");
        assert!(tokens.contains("pub fn Seek"), "{tokens}");
        assert!(tokens.contains("pub const ENOENT"), "{tokens}");
        assert!(tokens.contains("pub const O_RDONLY"), "{tokens}");
        assert!(tokens.contains("static errors"), "{tokens}");
        Ok(())
    }
}
