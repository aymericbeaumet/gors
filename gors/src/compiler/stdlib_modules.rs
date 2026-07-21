use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};

#[cfg(any(
    all(feature = "parallel", not(target_family = "wasm")),
    all(feature = "wasm-threads", target_family = "wasm")
))]
use rayon::prelude::*;

use super::{
    CompiledModule, dce_pruning, dce_reachability::reachable_stdlib_items,
    external_roots::ExternalRootCollector, required_module_roots::RequiredModuleRoots,
};

pub(super) fn resolve_required_stdlib_modules(
    modules: &mut BTreeMap<String, CompiledModule>,
    roots: &[String],
    jobs: usize,
    can_parallelize: bool,
) {
    let init_root_mod_names = init_root_module_names(roots);
    let mut import_path_by_module: HashMap<String, String> = crate::resolve::list_packages()
        .into_iter()
        .map(|path| (crate::resolve::module_name(&path), path))
        .collect();
    import_path_by_module.remove("builtin");
    for path in roots {
        import_path_by_module
            .entry(crate::resolve::module_name(path))
            .or_insert_with(|| path.clone());
    }
    let mut stdlib_mod_names: HashSet<String> = import_path_by_module.keys().cloned().collect();
    for module in modules.values().filter(|module| module.is_stdlib) {
        stdlib_mod_names.insert(module.mod_name.clone());
    }

    let mut required = RequiredModuleRoots::default();
    for path in roots {
        required.insert_module(crate::resolve::module_name(path));
    }
    {
        let external_root_collector = ExternalRootCollector::new(&stdlib_mod_names);
        for module in modules.values().filter(|module| !module.is_stdlib) {
            required.merge(external_root_collector.refs_from_items(&module.file.items));
        }
    }

    let mut install_state = ResolvedModuleInstallState::new(modules);
    let mut processed_full_modules = HashSet::new();
    loop {
        let mut pending: Vec<(String, String, HashSet<String>)> = required
            .keys()
            .filter(|module_name| {
                required
                    .get(module_name.as_str())
                    .is_some_and(|roots| !roots.is_empty())
            })
            .filter(|module_name| {
                let Some(roots) = required.get(module_name.as_str()) else {
                    return false;
                };
                !install_state
                    .loaded_roots
                    .get(module_name.as_str())
                    .is_some_and(|loaded| roots.is_subset(loaded))
            })
            .filter_map(|module_name| {
                import_path_by_module.get(module_name).map(|path| {
                    (
                        module_name.clone(),
                        path.clone(),
                        required.cloned_or_default(module_name),
                    )
                })
            })
            .collect();
        pending.sort_by(|left, right| (&left.1, &left.0).cmp(&(&right.1, &right.0)));

        if pending.is_empty() {
            break;
        }

        let mut loaded_any = false;
        for resolved in resolve_pending_modules(pending, jobs, can_parallelize) {
            let ResolvedPendingModule {
                module_name,
                import_path,
                required_roots,
                source,
                dependencies,
            } = resolved;
            let Some(source) = source else {
                trace_stdlib_resolution(format_args!(
                    "[gors] stdlib {import_path} produced no Rust items"
                ));
                install_state
                    .loaded_roots
                    .insert(module_name, required_roots);
                continue;
            };
            let file = match syn::parse_str::<syn::File>(&source) {
                Ok(file) => file,
                Err(error) => {
                    trace_stdlib_resolution(format_args!(
                        "[gors] stdlib {import_path} cache payload did not parse: {error}"
                    ));
                    install_state
                        .loaded_roots
                        .insert(module_name, required_roots);
                    continue;
                }
            };

            for dep in dependencies {
                let dep_module = crate::resolve::module_name(&dep);
                stdlib_mod_names.insert(dep_module.clone());
                import_path_by_module.entry(dep_module).or_insert(dep);
            }

            install_resolved_module(
                modules,
                &mut install_state,
                module_name,
                import_path,
                required_roots,
                file,
            );
            loaded_any = true;
        }

        let external_root_collector = ExternalRootCollector::with_item_fingerprints(
            &stdlib_mod_names,
            &install_state.item_fingerprints,
        );
        let mut changed = false;
        for module in modules.values().filter(|module| module.is_stdlib) {
            let refs = if module.mod_name == "builtin" {
                if !processed_full_modules.insert(module.import_path.clone()) {
                    continue;
                }
                external_root_collector.refs_from_items(&module.file.items)
            } else if let Some(roots) = required.get(&module.mod_name) {
                let roots = roots_with_package_init(module, roots, &init_root_mod_names);
                if install_state
                    .processed_roots
                    .get(&module.import_path)
                    .is_some_and(|processed| processed == roots.as_ref())
                {
                    continue;
                }
                let refs = external_root_collector
                    .refs_from_reachable_module_roots(module, roots.as_ref());
                install_state
                    .processed_roots
                    .insert(module.import_path.clone(), roots.into_owned());
                refs
            } else {
                continue;
            };
            changed |= required.merge(refs);
        }

        if !loaded_any && !changed {
            break;
        }
    }
}

struct ResolvedModuleInstallState {
    item_fingerprints: HashMap<String, String>,
    loaded_roots: HashMap<String, HashSet<String>>,
    processed_roots: HashMap<String, HashSet<String>>,
}

impl ResolvedModuleInstallState {
    fn new(modules: &BTreeMap<String, CompiledModule>) -> Self {
        let item_fingerprints = modules
            .values()
            .map(|module| {
                (
                    module.import_path.clone(),
                    super::reachability_cache::items_fingerprint(&module.file.items),
                )
            })
            .collect();
        Self {
            item_fingerprints,
            loaded_roots: HashMap::new(),
            processed_roots: HashMap::new(),
        }
    }
}

fn install_resolved_module(
    modules: &mut BTreeMap<String, CompiledModule>,
    state: &mut ResolvedModuleInstallState,
    module_name: String,
    import_path: String,
    required_roots: HashSet<String>,
    file: syn::File,
) {
    let filename = format!("{}.rs", crate::resolve::module_name(&import_path));
    state.item_fingerprints.insert(
        import_path.clone(),
        super::reachability_cache::items_fingerprint(&file.items),
    );
    state
        .loaded_roots
        .insert(module_name.clone(), required_roots);

    // Required roots can grow while an older rooted source is being scanned.
    // That scan may record the expanded roots even though the newly reachable
    // items are not present until the resolver publishes its wider source on
    // the next iteration. Replacing the source therefore invalidates the
    // processed marker even when the required root set itself is unchanged.
    state.processed_roots.remove(&import_path);

    modules.insert(
        import_path.clone(),
        CompiledModule {
            mod_name: module_name,
            import_path,
            file,
            filename,
            content_hash: String::new(),
            is_main: false,
            is_stdlib: true,
        },
    );
}

struct ResolvedPendingModule {
    module_name: String,
    import_path: String,
    required_roots: HashSet<String>,
    source: Option<String>,
    dependencies: Vec<String>,
}

fn resolve_pending_module(
    (module_name, import_path, required_roots): &(String, String, HashSet<String>),
    file_jobs: usize,
) -> ResolvedPendingModule {
    trace_stdlib_resolution(format_args!(
        "[gors] resolve stdlib {import_path} as {module_name} with roots {}",
        format_reachability_roots(required_roots.iter())
    ));
    let source = crate::resolve::resolve_with_roots_and_options(
        import_path,
        required_roots,
        crate::compiler::CompileOptions::with_jobs(file_jobs),
    )
    .map(|module| {
        let items = module.content.map(|(_, items)| items).unwrap_or_default();
        prettyplease::unparse(&syn::File {
            shebang: None,
            attrs: vec![],
            items,
        })
    });
    let dependencies = crate::resolve::collect_resolved_imports(import_path, required_roots);
    ResolvedPendingModule {
        module_name: module_name.clone(),
        import_path: import_path.clone(),
        required_roots: required_roots.clone(),
        source,
        dependencies,
    }
}

fn resolve_pending_modules(
    pending: Vec<(String, String, HashSet<String>)>,
    jobs: usize,
    can_parallelize: bool,
) -> Vec<ResolvedPendingModule> {
    let uncached_tasks = if can_parallelize && jobs > 1 {
        pending
            .iter()
            .enumerate()
            .filter_map(|(index, task)| {
                let (_, import_path, roots) = task;
                (!crate::resolve::has_initialized_resolved_module(import_path, roots))
                    .then_some((index, task))
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    #[cfg(all(feature = "parallel", not(target_family = "wasm")))]
    if uncached_tasks.len() > 1 && rayon::current_thread_index().is_none() {
        let thread_count = jobs.min(uncached_tasks.len());
        if let Ok(pool) = rayon::ThreadPoolBuilder::new()
            .num_threads(thread_count)
            .thread_name(|index| format!("gors-package-{index}"))
            .build()
        {
            let resolved = pool.install(|| {
                uncached_tasks
                    .par_iter()
                    .map(|&(index, task)| (index, resolve_pending_module(task, 1)))
                    .collect()
            });
            return merge_parallel_resolved_modules(&pending, resolved, jobs);
        }
    }

    #[cfg(all(feature = "wasm-threads", target_family = "wasm"))]
    if uncached_tasks.len() > 1 {
        let resolved = uncached_tasks
            .par_iter()
            .map(|&(index, task)| (index, resolve_pending_module(task, 1)))
            .collect();
        return merge_parallel_resolved_modules(&pending, resolved, jobs);
    }

    let _ = uncached_tasks;
    let file_jobs = if can_parallelize { jobs } else { 1 };
    pending
        .iter()
        .map(|task| resolve_pending_module(task, file_jobs))
        .collect()
}

#[cfg(any(
    all(feature = "parallel", not(target_family = "wasm")),
    all(feature = "wasm-threads", target_family = "wasm")
))]
fn merge_parallel_resolved_modules(
    pending: &[(String, String, HashSet<String>)],
    parallel: Vec<(usize, ResolvedPendingModule)>,
    jobs: usize,
) -> Vec<ResolvedPendingModule> {
    let mut parallel = parallel.into_iter().peekable();
    pending
        .iter()
        .enumerate()
        .map(|(index, task)| {
            if parallel
                .peek()
                .is_some_and(|(parallel_index, _)| *parallel_index == index)
                && let Some((_, result)) = parallel.next()
            {
                return result;
            }
            resolve_pending_module(task, jobs)
        })
        .collect()
}

fn trace_stdlib_resolution(args: std::fmt::Arguments<'_>) {
    if std::env::var("GORS_STDLIB_TRACE").is_ok_and(|value| value == "1" || value == "true") {
        eprintln!("{args}");
    }
}

fn format_reachability_roots<'a>(roots: impl IntoIterator<Item = &'a String>) -> String {
    let mut roots: Vec<_> = roots.into_iter().map(String::as_str).collect();
    roots.sort_unstable();
    if roots.is_empty() {
        "<empty>".to_string()
    } else {
        roots.join(",")
    }
}

pub(super) fn prune_dependency_stdlib_modules(
    modules: &mut BTreeMap<String, CompiledModule>,
    roots: &[String],
) {
    let init_root_mod_names = init_root_module_names(roots);
    let stdlib_mod_names: HashSet<String> = modules
        .values()
        .filter(|module| module.is_stdlib)
        .map(|module| module.mod_name.clone())
        .collect();
    if stdlib_mod_names.is_empty() {
        return;
    }

    let root_mod_names: HashSet<String> = std::iter::once("builtin".to_string()).collect();
    let item_fingerprints = modules
        .values()
        .map(|module| {
            (
                module.import_path.clone(),
                super::reachability_cache::items_fingerprint(&module.file.items),
            )
        })
        .collect();
    let external_root_collector =
        ExternalRootCollector::with_item_fingerprints(&stdlib_mod_names, &item_fingerprints);
    let mut preserved_mod_names: HashSet<String> = root_mod_names.iter().cloned().collect();
    for module in modules.values().filter(|module| !module.is_stdlib) {
        preserved_mod_names
            .extend(external_root_collector.module_refs_from_items(&module.file.items));
    }
    trace_stdlib_resolution(format_args!("[gors] preserve stdlib roots: {}", {
        let mut names: Vec<_> = preserved_mod_names.iter().cloned().collect();
        names.sort();
        names.join(",")
    }));
    trace_stdlib_resolution(format_args!("[gors] stdlib modules: {}", {
        let mut names: Vec<_> = stdlib_mod_names.iter().cloned().collect();
        names.sort();
        names.join(",")
    }));

    let mut required = RequiredModuleRoots::default();
    for module in modules.values().filter(|module| !module.is_stdlib) {
        required.merge(external_root_collector.refs_from_items(&module.file.items));
    }
    for (module, roots) in required.iter() {
        trace_stdlib_resolution(format_args!(
            "[gors] prune stdlib {module} with roots {}",
            format_reachability_roots(roots.iter())
        ));
    }

    let mut processed_roots = HashMap::new();
    let mut processed_full_modules = HashSet::new();
    loop {
        let mut changed = false;
        for module in modules.values().filter(|module| module.is_stdlib) {
            let refs = if root_mod_names.contains(&module.mod_name) {
                if !processed_full_modules.insert(module.import_path.clone()) {
                    continue;
                }
                external_root_collector.refs_from_items(&module.file.items)
            } else if let Some(roots) = required.get(&module.mod_name) {
                let roots = roots_with_package_init(module, roots, &init_root_mod_names);
                if processed_roots
                    .get(&module.import_path)
                    .is_some_and(|processed| processed == roots.as_ref())
                {
                    continue;
                }
                let refs = external_root_collector
                    .refs_from_reachable_module_roots(module, roots.as_ref());
                processed_roots.insert(module.import_path.clone(), roots.into_owned());
                refs
            } else {
                continue;
            };
            changed |= required.merge(refs);
        }
        if !changed {
            break;
        }
    }

    let empty = HashSet::new();
    let removable: Vec<String> = modules
        .iter()
        .filter_map(|(key, module)| {
            if !module.is_stdlib || preserved_mod_names.contains(&module.mod_name) {
                return None;
            }
            if required.is_missing_or_empty(&module.mod_name) {
                Some(key.clone())
            } else {
                None
            }
        })
        .collect();
    for key in removable {
        modules.remove(&key);
    }

    for module in modules.values_mut().filter(|module| module.is_stdlib) {
        if root_mod_names.contains(&module.mod_name) {
            continue;
        }
        let roots = required.get_or_empty(&module.mod_name, &empty);
        let roots = roots_with_package_init(module, roots, &init_root_mod_names);
        let reachable =
            reachable_stdlib_items(&module.file.items, roots.as_ref(), &stdlib_mod_names);
        if reachable.keep.is_empty() {
            module.file.items.clear();
            module.content_hash = String::new();
            continue;
        }
        dce_pruning::retain_reachable_items(&mut module.file.items, roots.as_ref(), &reachable);
        module.content_hash = String::new();
    }
    prune_unreferenced_stdlib_modules(modules, &preserved_mod_names);
}

fn init_root_module_names(roots: &[String]) -> HashSet<String> {
    roots
        .iter()
        .map(|path| crate::resolve::module_name(path))
        .collect()
}

fn roots_with_package_init<'a>(
    module: &CompiledModule,
    roots: &'a HashSet<String>,
    init_root_mod_names: &HashSet<String>,
) -> Cow<'a, HashSet<String>> {
    if !init_root_mod_names.contains(&module.mod_name)
        || !module_has_nonempty_package_init(module)
        || roots.contains(crate::generated_names::PACKAGE_INIT_FN)
    {
        return Cow::Borrowed(roots);
    }

    let mut expanded = roots.clone();
    expanded.insert(crate::generated_names::PACKAGE_INIT_FN.to_string());
    Cow::Owned(expanded)
}

fn module_has_nonempty_package_init(module: &CompiledModule) -> bool {
    module.file.items.iter().any(|item| {
        matches!(
            item,
            syn::Item::Fn(func)
                if func.sig.ident == crate::generated_names::PACKAGE_INIT_FN
                    && !func.block.stmts.is_empty()
        )
    })
}

pub(super) fn prune_unreferenced_stdlib_modules(
    modules: &mut BTreeMap<String, CompiledModule>,
    preserved_mod_names: &std::collections::HashSet<String>,
) {
    loop {
        let stdlib_mod_names: HashSet<String> = modules
            .values()
            .filter(|module| module.is_stdlib)
            .map(|module| module.mod_name.clone())
            .collect();
        let external_root_collector = ExternalRootCollector::new(&stdlib_mod_names);
        let mut referenced = HashSet::new();
        for module in modules.values() {
            if module.mod_name == "builtin" {
                continue;
            }
            referenced.extend(external_root_collector.module_refs_from_items(&module.file.items));
        }
        referenced.insert("builtin".to_string());

        let removable: Vec<String> = modules
            .iter()
            .filter(|&(_key, module)| {
                module.is_stdlib
                    && !preserved_mod_names.contains(&module.mod_name)
                    && !referenced.contains(&module.mod_name)
            })
            .map(|(key, _module)| key.clone())
            .collect();
        if removable.is_empty() {
            break;
        }
        for key in removable {
            modules.remove(&key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wider_resolved_source_invalidates_roots_processed_against_prior_source()
    -> Result<(), &'static str> {
        let module_name = "wrapper".to_string();
        let import_path = "example/wrapper".to_string();
        let required_roots = HashSet::from([
            "Atoi".to_string(),
            "FormatInt".to_string(),
            "FormatUint".to_string(),
        ]);
        let old_file: syn::File = syn::parse_quote! {
            pub fn FormatInt() {
                crate::leaf::FormatInt();
            }
        };
        let mut modules = BTreeMap::from([(
            import_path.clone(),
            CompiledModule {
                mod_name: module_name.clone(),
                import_path: import_path.clone(),
                file: old_file,
                filename: "wrapper.rs".to_string(),
                content_hash: String::new(),
                is_main: false,
                is_stdlib: true,
            },
        )]);
        let mut install_state = ResolvedModuleInstallState::new(&modules);
        install_state.loaded_roots.insert(
            module_name.clone(),
            HashSet::from(["FormatInt".to_string()]),
        );

        // A downstream module can add roots before this module is reloaded.
        // The old source then appears processed for the wider root set even
        // though it does not contain the newly reachable items yet.
        install_state
            .processed_roots
            .insert(import_path.clone(), required_roots.clone());
        let wider_file: syn::File = syn::parse_quote! {
            pub fn Atoi() {
                crate::leaf::Atoi();
            }

            pub fn FormatInt() {
                crate::leaf::FormatInt();
            }

            pub fn FormatUint() {
                crate::leaf::FormatUint();
            }
        };

        install_resolved_module(
            &mut modules,
            &mut install_state,
            module_name.clone(),
            import_path.clone(),
            required_roots.clone(),
            wider_file,
        );

        assert_eq!(
            install_state.loaded_roots.get(&module_name),
            Some(&required_roots)
        );
        assert!(!install_state.processed_roots.contains_key(&import_path));

        let module_names = HashSet::from([module_name, "leaf".to_string()]);
        let collector = ExternalRootCollector::with_item_fingerprints(
            &module_names,
            &install_state.item_fingerprints,
        );
        let reloaded_module = modules.get(&import_path).ok_or("missing reloaded module")?;
        let refs = collector.refs_from_reachable_module_roots(reloaded_module, &required_roots);
        let leaf_roots = refs
            .get("leaf")
            .ok_or("missing leaf roots from wider source")?;
        assert!(leaf_roots.contains("Atoi"));
        assert!(leaf_roots.contains("FormatInt"));
        assert!(leaf_roots.contains("FormatUint"));
        Ok(())
    }
}
