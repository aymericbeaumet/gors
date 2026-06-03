//! Go package resolver.
//!
//! This module resolves import paths to Go source packages, currently backed by
//! build-time generated metadata from the embedded Go SDK.

use std::collections::{BTreeMap, HashMap, HashSet};
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

    let mut package_type_env = TypeEnv::new();
    for (_, ast) in &parsed_files {
        package_type_env.scan_file(ast);
    }
    let mut imported_type_envs: BTreeMap<String, crate::compiler::PackageFacts> = BTreeMap::new();
    for (_, ast) in &parsed_files {
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
    for (_, ast) in &parsed_files {
        let mut inference_env = package_type_env.clone();
        crate::compiler::merge_import_type_envs(
            &mut inference_env,
            ast,
            &BTreeMap::new(),
            &imported_type_envs,
        );
        package_type_env.rescan_file_top_level_vars(ast, &inference_env);
    }

    let import_renames = package_import_renames(&parsed_files);
    let import_path_by_module = package_import_path_by_module(&parsed_files);
    let mut all_items: Vec<syn::Item> = Vec::new();
    let ast_refs: Vec<_> = parsed_files.iter().map(|(_, ast)| ast).collect();
    crate::compiler::set_borrow_pointer_arg_indices_for_files(&ast_refs);

    for (filename, ast) in parsed_files {
        let compiled = match compile_resolved_file(
            ast,
            &package_type_env,
            &imported_type_envs,
            &import_renames,
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
                        import_path,
                        filename,
                        content,
                        reachable_names.as_ref(),
                        &package_type_env,
                        &imported_type_envs,
                        &import_renames,
                        roots,
                    ));
                }
                continue;
            }
        };
        all_items.extend(compiled.items);
    }
    crate::compiler::clear_borrow_pointer_arg_indices();

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
        crate::compiler::compile_with_type_env_and_import_renames(
            ast,
            type_env,
            import_renames.clone(),
        )
    })
}

struct DeclRecoveryPlan {
    non_import_index: usize,
    label: String,
    split_specs: usize,
}

fn recover_resolved_file_items<'a>(
    import_path: &str,
    filename: &'a str,
    content: &'a str,
    reachable_names: Option<&HashSet<String>>,
    package_type_env: &TypeEnv,
    imported_type_envs: &BTreeMap<String, crate::compiler::PackageFacts>,
    import_renames: &BTreeMap<String, String>,
    roots: Option<&HashSet<String>>,
) -> Vec<syn::Item> {
    let plans = recovery_plans_for_file(filename, content, reachable_names);
    let mut items = Vec::new();
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
            package_type_env,
            imported_type_envs,
            import_renames,
            roots,
        ) {
            Ok(compiled) => {
                items.extend(compiled.items);
                continue;
            }
            Err(error) => {
                log_skip(format_args!(
                    "[gors] skip {import_path}/{filename} {}: compile error: {error}",
                    plan.label
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
                package_type_env,
                imported_type_envs,
                import_renames,
                roots,
            ) {
                Ok(compiled) => items.extend(compiled.items),
                Err(error) => {
                    log_skip(format_args!(
                        "[gors] skip {import_path}/{filename} {label}: compile error: {error}"
                    ));
                }
            }
        }
    }
    items
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

    for (filename, content) in files.iter() {
        let Ok(ast) = crate::parser::parse_file(filename, content) else {
            continue;
        };
        if package_name.is_none() {
            package_name = Some(ast.name.name.to_string());
        }
        env.scan_file(&ast);
    }

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
    let mut reachable: HashSet<String> = roots
        .iter()
        .filter(|name| top_names.contains(name.as_str()))
        .cloned()
        .collect();

    let mut changed = true;
    while changed {
        changed = false;
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
            }
        }
    }

    reachable
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
        crate::ast::Decl::FuncDecl(_) => Vec::new(),
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
            reachable.contains(func.name.name)
                || receiver_type_name(func).is_some_and(|name| reachable.contains(&name))
        }
        crate::ast::Decl::GenDecl(gen_decl) => gen_decl
            .specs
            .iter()
            .any(|spec| spec_names(spec).iter().any(|name| reachable.contains(name))),
    }
}

fn receiver_type_name(func: &crate::ast::FuncDecl<'_>) -> Option<String> {
    func.recv
        .as_ref()
        .and_then(|recv| recv.list.first())
        .and_then(|field| field.type_.as_ref())
        .and_then(named_type_from_expr)
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
            let keep = reachable.contains(func.name.name)
                || receiver_type_name(&func).is_some_and(|name| reachable.contains(&name));
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
    use quote::ToTokens;

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
        assert!(matches!(items[2], syn::Item::Fn(_)));
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
}
