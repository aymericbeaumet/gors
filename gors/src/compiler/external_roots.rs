use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use super::dce_reachability::{reachable_stdlib_items, reachable_stdlib_items_with_fingerprint};
use super::reachability_names::{item_reachability_names, top_level_item_names};
use super::receiver_type_facts::{
    top_level_collection_element_types, top_level_item_field_types, top_level_item_return_types,
    top_level_item_tuple_return_types, top_level_item_types,
};
use super::ref_collection::{RefCollectionContext, collect_refs_from_item};
use super::semantic_reachability::SemanticReachabilityGraph;
use super::{CompiledModule, required_module_roots};

pub(super) struct ExternalRootCollector<'a> {
    module_names: &'a HashSet<String>,
    item_fingerprints: Option<&'a HashMap<String, String>>,
    semantic_graph: Option<&'a SemanticReachabilityGraph>,
}

impl<'a> ExternalRootCollector<'a> {
    pub(super) fn new(module_names: &'a HashSet<String>) -> Self {
        Self {
            module_names,
            item_fingerprints: None,
            semantic_graph: None,
        }
    }

    pub(super) fn with_item_fingerprints(
        module_names: &'a HashSet<String>,
        item_fingerprints: &'a HashMap<String, String>,
    ) -> Self {
        Self {
            module_names,
            item_fingerprints: Some(item_fingerprints),
            semantic_graph: None,
        }
    }

    pub(super) fn with_semantic_audit(
        module_names: &'a HashSet<String>,
        item_fingerprints: &'a HashMap<String, String>,
        semantic_graph: Option<&'a SemanticReachabilityGraph>,
    ) -> Self {
        Self {
            module_names,
            item_fingerprints: Some(item_fingerprints),
            semantic_graph,
        }
    }

    pub(super) fn refs_from_items(&self, items: &[syn::Item]) -> HashMap<String, HashSet<String>> {
        collect_external_refs(items, self.module_names)
    }

    pub(super) fn module_refs_from_items(&self, items: &[syn::Item]) -> HashSet<String> {
        self.refs_from_items(items).into_keys().collect()
    }

    pub(super) fn refs_from_reachable_module_roots(
        &self,
        module: &CompiledModule,
        roots: &HashSet<String>,
    ) -> HashMap<String, HashSet<String>> {
        let reachable = self
            .item_fingerprints
            .and_then(|fingerprints| fingerprints.get(&module.import_path))
            .map_or_else(
                || reachable_stdlib_items(&module.file.items, roots, self.module_names),
                |fingerprint| {
                    reachable_stdlib_items_with_fingerprint(
                        &module.file.items,
                        fingerprint,
                        roots,
                        self.module_names,
                    )
                },
            );
        let refs = reachable.refs;
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
    let item_names = item_reachability_names(items);
    let top_level_names = top_level_item_names(items);
    let top_level_types = top_level_item_types(items, module_names);
    let top_level_field_types = top_level_item_field_types(items, module_names);
    let top_level_element_types = top_level_collection_element_types(items, module_names);
    let top_level_return_types = top_level_item_return_types(items, module_names);
    let top_level_tuple_return_types = top_level_item_tuple_return_types(items, module_names);
    for item in items {
        let mut item_clone = item.clone();
        let context = RefCollectionContext {
            module_names,
            item_names: &item_names,
            top_level_names: &top_level_names,
            top_level_types: &top_level_types,
            top_level_field_types: &top_level_field_types,
            top_level_element_types: &top_level_element_types,
            top_level_return_types: &top_level_return_types,
            top_level_tuple_return_types: &top_level_tuple_return_types,
        };
        let (_, refs) = collect_refs_from_item(&mut item_clone, &context);
        required_module_roots::merge_refs(&mut external_refs, refs);
    }
    external_refs
}
