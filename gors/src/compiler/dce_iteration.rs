use std::collections::{BTreeMap, HashSet};

use super::CompiledModule;
use super::external_roots::ExternalRootCollector;
use super::semantic_reachability::{
    SemanticReachabilityGraph, semantic_reachability_graph_enabled,
};

pub(super) struct DceIterationContext {
    module_names: HashSet<String>,
    semantic_graph: Option<SemanticReachabilityGraph>,
}

impl DceIterationContext {
    pub(super) fn new(modules: &BTreeMap<String, CompiledModule>, has_main: bool) -> Self {
        let module_names = modules
            .values()
            .filter(|module| !module.is_main)
            .map(|module| module.mod_name.clone())
            .collect();
        let semantic_graph = semantic_reachability_graph_enabled().then(|| {
            let semantic_graph = SemanticReachabilityGraph::from_modules(modules, has_main);
            debug_assert!(semantic_graph.has_consistent_local_edges());
            let _reachable_external_roots = semantic_graph.reachable_external_roots_by_module();
            semantic_graph
        });
        Self {
            module_names,
            semantic_graph,
        }
    }

    pub(super) fn module_names(&self) -> &HashSet<String> {
        &self.module_names
    }

    pub(super) fn external_root_collector(&self) -> ExternalRootCollector<'_> {
        ExternalRootCollector::with_semantic_audit(&self.module_names, self.semantic_graph.as_ref())
    }
}
