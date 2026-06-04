use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use super::semantic_reachability::SemanticReachabilityGraph;
use super::{
    CompiledModule, RefCollectionContext, collect_refs_from_item, reachable_stdlib_items,
    required_module_roots,
};

pub(super) struct ExternalRootCollector<'a> {
    module_names: &'a HashSet<String>,
    semantic_graph: Option<&'a SemanticReachabilityGraph>,
}

impl<'a> ExternalRootCollector<'a> {
    pub(super) fn new(module_names: &'a HashSet<String>) -> Self {
        Self {
            module_names,
            semantic_graph: None,
        }
    }

    pub(super) fn with_semantic_audit(
        module_names: &'a HashSet<String>,
        semantic_graph: Option<&'a SemanticReachabilityGraph>,
    ) -> Self {
        Self {
            module_names,
            semantic_graph,
        }
    }

    pub(super) fn refs_from_items(&self, items: &[syn::Item]) -> HashMap<String, HashSet<String>> {
        collect_external_refs(items, self.module_names)
    }

    pub(super) fn module_refs_from_items(&self, items: &[syn::Item]) -> HashSet<String> {
        self.refs_from_items(items).into_keys().collect()
    }

    pub(super) fn refs_from_items_with_roots(
        &self,
        module: &str,
        roots: &HashSet<String>,
        items: &[syn::Item],
    ) -> HashMap<String, HashSet<String>> {
        let refs = self.refs_from_items(items);
        debug_assert_semantic_external_refs(self.semantic_graph, module, roots, &refs);
        refs
    }

    pub(super) fn refs_from_reachable_module_roots(
        &self,
        module: &CompiledModule,
        roots: &HashSet<String>,
    ) -> HashMap<String, HashSet<String>> {
        let refs = reachable_stdlib_items(&module.file.items, roots, self.module_names).refs;
        debug_assert_semantic_external_refs(self.semantic_graph, &module.mod_name, roots, &refs);
        refs
    }
}

fn debug_assert_semantic_external_refs(
    semantic_graph: Option<&SemanticReachabilityGraph>,
    module: &str,
    roots: &HashSet<String>,
    refs: &HashMap<String, HashSet<String>>,
) {
    let Some(semantic_graph) = semantic_graph else {
        return;
    };
    let semantic_refs = semantic_graph.reachable_external_roots_for_module_roots(module, roots);
    let token_refs = refs_to_btree(refs);
    debug_assert_eq!(
        token_refs, semantic_refs,
        "semantic reachability external refs mismatch for module {module} roots {roots:?}"
    );
}

fn refs_to_btree(refs: &HashMap<String, HashSet<String>>) -> BTreeMap<String, BTreeSet<String>> {
    refs.iter()
        .map(|(module, roots)| {
            (
                module.clone(),
                roots.iter().cloned().collect::<BTreeSet<_>>(),
            )
        })
        .collect()
}

pub(super) fn collect_external_refs(
    items: &[syn::Item],
    module_names: &HashSet<String>,
) -> HashMap<String, HashSet<String>> {
    let mut external_refs = HashMap::new();
    let empty_types = HashMap::new();
    let empty_field_types = HashMap::new();
    let empty_element_types = HashMap::new();
    let empty_return_types = HashMap::new();
    let empty_tuple_return_types = HashMap::new();
    for item in items {
        let mut item_clone = item.clone();
        let empty_item_names = HashSet::new();
        let empty_top_level_names = HashSet::new();
        let context = RefCollectionContext {
            module_names,
            item_names: &empty_item_names,
            top_level_names: &empty_top_level_names,
            top_level_types: &empty_types,
            top_level_field_types: &empty_field_types,
            top_level_element_types: &empty_element_types,
            top_level_return_types: &empty_return_types,
            top_level_tuple_return_types: &empty_tuple_return_types,
        };
        let (_, refs) = collect_refs_from_item(&mut item_clone, &context);
        required_module_roots::merge_refs(&mut external_refs, refs);
    }
    external_refs
}
