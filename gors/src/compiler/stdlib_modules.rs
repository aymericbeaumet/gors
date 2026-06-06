use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};

use super::{
    CompiledModule, dce_pruning, dce_reachability::reachable_stdlib_items,
    external_roots::ExternalRootCollector, required_module_roots::RequiredModuleRoots,
};

pub(super) fn resolve_required_stdlib_modules(
    modules: &mut BTreeMap<String, CompiledModule>,
    roots: &[String],
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

    let mut loaded_roots: HashMap<String, HashSet<String>> = HashMap::new();
    loop {
        let pending: Vec<(String, String)> = required
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
                !loaded_roots
                    .get(module_name.as_str())
                    .is_some_and(|loaded| roots.is_subset(loaded))
            })
            .filter_map(|module_name| {
                import_path_by_module
                    .get(module_name)
                    .map(|path| (module_name.clone(), path.clone()))
            })
            .collect();

        if pending.is_empty() {
            break;
        }

        let mut loaded_any = false;
        for (module_name, import_path) in pending {
            let required_roots = required.cloned_or_default(&module_name);
            trace_stdlib_resolution(format_args!(
                "[gors] resolve stdlib {import_path} as {module_name} with roots {}",
                format_reachability_roots(required_roots.iter())
            ));
            let items = if let Some(stdlib_mod) =
                crate::resolve::resolve_with_roots(&import_path, &required_roots)
            {
                match stdlib_mod.content {
                    Some((_, items)) => items,
                    None => vec![],
                }
            } else {
                trace_stdlib_resolution(format_args!(
                    "[gors] stdlib {import_path} produced no Rust items"
                ));
                loaded_roots.insert(module_name, required_roots);
                continue;
            };

            for dep in crate::resolve::collect_resolved_imports(&import_path, &required_roots) {
                let dep_module = crate::resolve::module_name(&dep);
                stdlib_mod_names.insert(dep_module.clone());
                import_path_by_module.entry(dep_module).or_insert(dep);
            }

            let filename = format!("{}.rs", crate::resolve::module_name(&import_path));
            let loaded_module_name = module_name.clone();
            modules.insert(
                import_path.clone(),
                CompiledModule {
                    mod_name: module_name,
                    import_path,
                    file: syn::File {
                        attrs: vec![],
                        items,
                        shebang: None,
                    },
                    filename,
                    content_hash: String::new(),
                    is_main: false,
                    is_stdlib: true,
                },
            );
            loaded_roots.insert(loaded_module_name, required_roots);
            loaded_any = true;
        }

        let external_root_collector = ExternalRootCollector::new(&stdlib_mod_names);
        let mut changed = false;
        for module in modules.values().filter(|module| module.is_stdlib) {
            let refs = if module.mod_name == "builtin" {
                external_root_collector.refs_from_items(&module.file.items)
            } else if let Some(roots) = required.get(&module.mod_name) {
                let roots = roots_with_package_init(module, roots, &init_root_mod_names);
                external_root_collector.refs_from_reachable_module_roots(module, roots.as_ref())
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
    let external_root_collector = ExternalRootCollector::new(&stdlib_mod_names);
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

    loop {
        let mut changed = false;
        for module in modules.values().filter(|module| module.is_stdlib) {
            let refs = if root_mod_names.contains(&module.mod_name) {
                external_root_collector.refs_from_items(&module.file.items)
            } else if let Some(roots) = required.get(&module.mod_name) {
                let roots = roots_with_package_init(module, roots, &init_root_mod_names);
                external_root_collector.refs_from_reachable_module_roots(module, roots.as_ref())
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
