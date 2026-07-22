use std::collections::{BTreeMap, HashMap, HashSet};

use super::CompiledModule;
use super::builtin_roots;
use super::external_roots::ExternalRootCollector;
use super::required_module_roots::RequiredModuleRoots;
use super::semantic_reachability::{
    SemanticReachabilityGraph, semantic_reachability_graph_enabled,
};

pub(super) struct DceIterationContext {
    module_names: HashSet<String>,
    item_fingerprints: HashMap<String, String>,
    semantic_graph: Option<SemanticReachabilityGraph>,
}

impl DceIterationContext {
    pub(super) fn new(modules: &BTreeMap<String, CompiledModule>, has_main: bool) -> Self {
        let module_names = modules
            .values()
            .filter(|module| !module.is_main)
            .map(|module| module.mod_name.clone())
            .collect();
        let item_fingerprints = modules
            .values()
            .map(|module| {
                (
                    module.import_path.clone(),
                    super::reachability_cache::items_fingerprint(&module.file.items),
                )
            })
            .collect();
        let semantic_graph = semantic_reachability_graph_enabled().then(|| {
            let semantic_graph = SemanticReachabilityGraph::from_modules(modules, has_main);
            debug_assert!(semantic_graph.has_consistent_local_edges());
            let _reachable_external_roots = semantic_graph.reachable_external_roots_by_module();
            semantic_graph
        });
        Self {
            module_names,
            item_fingerprints,
            semantic_graph,
        }
    }

    pub(super) fn module_names(&self) -> &HashSet<String> {
        &self.module_names
    }

    pub(super) fn external_root_collector(&self) -> ExternalRootCollector<'_> {
        ExternalRootCollector::with_semantic_audit(
            &self.module_names,
            &self.item_fingerprints,
            self.semantic_graph.as_ref(),
        )
    }
}

/// Discover the cross-module root closure without mutating generated items.
///
/// Both pre-DCE synthesis and DCE itself use this fixed point so dead source
/// items cannot create obligations and builtin expansion cannot drift between
/// planning and pruning.
pub(super) fn discover_required_module_roots(
    modules: &BTreeMap<String, CompiledModule>,
    has_main: bool,
) -> RequiredModuleRoots {
    let context = DceIterationContext::new(modules, has_main);
    let collector = context.external_root_collector();
    let mut required = RequiredModuleRoots::default();

    if let Some(main_module) = modules.get("__main__") {
        let roots = super::reachability_names::main_module_root_names(main_module, has_main);
        required.merge(collector.refs_from_reachable_module_roots(main_module, &roots));
    }

    let mut processed_roots = HashMap::new();
    loop {
        let mut changed = false;
        for module in modules.values().filter(|module| !module.is_main) {
            let Some(roots) = required.get(&module.mod_name) else {
                continue;
            };
            if roots.is_empty() {
                continue;
            }
            let expanded_roots;
            let roots = if module.mod_name == "builtin" {
                expanded_roots = builtin_roots::expand(roots);
                &expanded_roots
            } else {
                roots
            };
            if processed_roots
                .get(&module.import_path)
                .is_some_and(|processed| processed == roots)
            {
                continue;
            }
            let refs = collector.refs_from_reachable_module_roots(module, roots);
            processed_roots.insert(module.import_path.clone(), roots.clone());
            changed |= required.merge(refs);
        }
        if !changed {
            break;
        }
    }

    required
}
