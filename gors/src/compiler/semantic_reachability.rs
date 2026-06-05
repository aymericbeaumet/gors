use std::collections::{BTreeMap, BTreeSet, HashSet};

use super::item_reachability::{
    impl_method_reachability_name, reachable_item_for_names,
    trait_impl_can_follow_self_reachability,
};
use super::reachability_names::{
    expand_supertrait_method_names, expand_supertrait_names, exported_item_reachability_names,
    item_reachability_names, top_level_item_names, trait_method_names, trait_supertrait_names,
};
use super::receiver_type_facts::{
    ReceiverTypeMap, top_level_collection_element_types, top_level_item_field_types,
    top_level_item_return_types, top_level_item_tuple_return_types, top_level_item_types,
};
use super::ref_collection::RefCollectionContext;
use super::syn_inspect::{item_name, named_self_type, self_type_reachability_names};
use super::{CompiledModule, collect_refs_from_item, generated_attrs, interface_hooks};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct SemanticItemId {
    pub(super) module: String,
    pub(super) kind: SemanticItemKind,
    pub(super) name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum SemanticItemKind {
    Const,
    Function,
    Static,
    Type,
    Trait,
    ImplItem,
    TraitItem,
    Macro,
    SyntheticRoot,
}

#[derive(Debug, Clone, Default)]
pub(super) struct SemanticReachabilityNode {
    pub(super) local_refs: BTreeSet<SemanticItemId>,
    pub(super) external_refs: BTreeMap<String, BTreeSet<String>>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct SemanticReachabilityGraph {
    pub(super) nodes: BTreeMap<SemanticItemId, SemanticReachabilityNode>,
    pub(super) roots: BTreeSet<SemanticItemId>,
    preserved: BTreeSet<SemanticItemId>,
}

impl SemanticReachabilityGraph {
    pub(super) fn from_modules(modules: &BTreeMap<String, CompiledModule>, has_main: bool) -> Self {
        let module_names = modules
            .values()
            .filter(|module| !module.is_main)
            .map(|module| module.mod_name.clone())
            .collect::<HashSet<_>>();
        let mut graph = Self::default();

        for module in modules.values() {
            let item_names = item_reachability_names(&module.file.items);
            let top_level_types = top_level_item_types(&module.file.items, &module_names);
            for item in &module.file.items {
                let ids = semantic_item_ids_for_item(&module.mod_name, item);
                if item_has_preserve_attr(item) {
                    graph.preserved.extend(ids.iter().cloned());
                }
                for id in ids {
                    graph.nodes.entry(id).or_default();
                }
            }
            for id in semantic_synthetic_receiver_method_ids_for_module(
                &module.mod_name,
                &item_names,
                &top_level_types,
            ) {
                graph.nodes.entry(id).or_default();
            }
        }

        for module in modules.values() {
            let item_names = item_reachability_names(&module.file.items);
            let top_level_names = top_level_item_names(&module.file.items);
            let top_level_types = top_level_item_types(&module.file.items, &module_names);
            let synthetic_ids = semantic_synthetic_receiver_method_ids_for_module(
                &module.mod_name,
                &item_names,
                &top_level_types,
            );
            let mut name_index = semantic_item_name_index(&module.mod_name, &module.file.items);
            for id in &synthetic_ids {
                name_index
                    .entry(id.name.clone())
                    .or_default()
                    .insert(id.clone());
            }
            graph
                .roots
                .extend(semantic_root_ids_for_module(module, has_main, &name_index));

            let top_level_field_types =
                top_level_item_field_types(&module.file.items, &module_names);
            let top_level_element_types =
                top_level_collection_element_types(&module.file.items, &module_names);
            let top_level_return_types =
                top_level_item_return_types(&module.file.items, &module_names);
            let top_level_tuple_return_types =
                top_level_item_tuple_return_types(&module.file.items, &module_names);
            let trait_supertraits = trait_supertrait_names(&module.file.items);
            let trait_methods = trait_method_names(&module.file.items);
            let noop_trait_impl_ids =
                semantic_noop_trait_impl_ids_by_trait(&module.mod_name, &module.file.items);
            let self_reachable_trait_impl_ids = semantic_self_reachable_trait_impl_ids_by_self(
                &module.mod_name,
                &module.file.items,
                &top_level_names,
            );
            let context = RefCollectionContext {
                module_names: &module_names,
                item_names: &item_names,
                top_level_names: &top_level_names,
                top_level_types: &top_level_types,
                top_level_field_types: &top_level_field_types,
                top_level_element_types: &top_level_element_types,
                top_level_return_types: &top_level_return_types,
                top_level_tuple_return_types: &top_level_tuple_return_types,
            };

            for source_id in synthetic_ids {
                let expansion_ref_ids = semantic_expansion_ref_ids_for_name(
                    &source_id.name,
                    &name_index,
                    &trait_supertraits,
                    &trait_methods,
                    &item_names,
                    &top_level_types,
                );
                let node = graph.nodes.entry(source_id).or_default();
                node.local_refs.extend(expansion_ref_ids);
            }

            for item in &module.file.items {
                let source_ids = semantic_item_ids_for_item(&module.mod_name, item);
                if source_ids.is_empty() {
                    continue;
                }

                for source_id in source_ids {
                    let source_roots = HashSet::from([source_id.name.clone()]);
                    let source_names = semantic_source_reachability_names(item, &source_id);
                    let (local_ref_ids, external_refs) = reachable_item_for_names(
                        item,
                        &source_names,
                        &item_names,
                        &top_level_names,
                        &source_roots,
                    )
                    .map(|mut source_item| {
                        let (local_refs, external_refs) =
                            collect_refs_from_item(&mut source_item, &context);
                        let local_ref_ids =
                            semantic_local_ref_ids_for_names(&local_refs, &name_index);
                        let external_refs = external_refs
                            .into_iter()
                            .map(|(module, refs)| {
                                (module, refs.into_iter().collect::<BTreeSet<_>>())
                            })
                            .collect::<BTreeMap<_, _>>();
                        (local_ref_ids, external_refs)
                    })
                    .unwrap_or_default();
                    let expansion_ref_ids = semantic_expansion_ref_ids_for_name(
                        &source_id.name,
                        &name_index,
                        &trait_supertraits,
                        &trait_methods,
                        &item_names,
                        &top_level_types,
                    );
                    let self_trait_impl_ref_ids = (source_id.kind == SemanticItemKind::Type
                        && !interface_hooks::is_noop_type_name(&source_id.name))
                    .then(|| self_reachable_trait_impl_ids.get(&source_id.name))
                    .flatten();
                    let noop_trait_impl_ref_ids = (source_id.kind == SemanticItemKind::Trait)
                        .then(|| noop_trait_impl_ids.get(&source_id.name))
                        .flatten();
                    let node = graph.nodes.entry(source_id).or_default();
                    node.local_refs.extend(expansion_ref_ids);
                    if let Some(impl_ids) = self_trait_impl_ref_ids {
                        node.local_refs.extend(impl_ids.iter().cloned());
                    }
                    if let Some(impl_ids) = noop_trait_impl_ref_ids {
                        node.local_refs.extend(impl_ids.iter().cloned());
                    }
                    node.local_refs.extend(local_ref_ids.iter().cloned());
                    for (module, refs) in &external_refs {
                        node.external_refs
                            .entry(module.clone())
                            .or_default()
                            .extend(refs.iter().cloned());
                    }
                }
            }
        }

        graph
    }

    pub(super) fn has_consistent_local_edges(&self) -> bool {
        self.roots.iter().all(|root| self.nodes.contains_key(root))
            && self.nodes.values().all(|node| {
                node.local_refs
                    .iter()
                    .all(|target| self.nodes.contains_key(target))
            })
    }

    pub(super) fn reachable_external_roots_by_module(&self) -> BTreeMap<String, BTreeSet<String>> {
        self.external_roots_for_item_ids(self.reachable_item_ids())
    }

    pub(super) fn reachable_external_roots_for_module_roots(
        &self,
        module: &str,
        roots: &HashSet<String>,
    ) -> BTreeMap<String, BTreeSet<String>> {
        self.external_roots_for_item_ids(self.reachable_item_ids_for_module_roots(module, roots))
    }

    fn external_roots_for_item_ids(
        &self,
        item_ids: impl IntoIterator<Item = SemanticItemId>,
    ) -> BTreeMap<String, BTreeSet<String>> {
        let mut external_roots = BTreeMap::new();
        for item_id in item_ids {
            let Some(node) = self.nodes.get(&item_id) else {
                continue;
            };
            for (module, refs) in &node.external_refs {
                external_roots
                    .entry(module.clone())
                    .or_insert_with(BTreeSet::new)
                    .extend(refs.iter().cloned());
            }
        }
        external_roots
    }

    pub(super) fn reachable_item_ids(&self) -> BTreeSet<SemanticItemId> {
        self.reachable_item_ids_from(self.roots.iter().cloned())
    }

    pub(super) fn reachable_item_ids_for_module_roots(
        &self,
        module: &str,
        roots: &HashSet<String>,
    ) -> BTreeSet<SemanticItemId> {
        self.reachable_item_ids_from(self.item_ids_for_module_roots(module, roots))
    }

    fn item_ids_for_module_roots(
        &self,
        module: &str,
        roots: &HashSet<String>,
    ) -> BTreeSet<SemanticItemId> {
        self.nodes
            .keys()
            .filter(|id| id.module == module && roots.contains(&id.name))
            .cloned()
            .chain(
                self.preserved
                    .iter()
                    .filter(|id| id.module == module)
                    .cloned(),
            )
            .collect()
    }

    fn reachable_item_ids_from(
        &self,
        roots: impl IntoIterator<Item = SemanticItemId>,
    ) -> BTreeSet<SemanticItemId> {
        let mut reachable = BTreeSet::new();
        let mut pending = roots.into_iter().collect::<Vec<_>>();
        while let Some(item_id) = pending.pop() {
            if !reachable.insert(item_id.clone()) {
                continue;
            }
            let Some(node) = self.nodes.get(&item_id) else {
                continue;
            };
            pending.extend(node.local_refs.iter().cloned());
        }
        reachable
    }
}

fn semantic_expansion_ref_ids_for_name(
    name: &str,
    name_index: &BTreeMap<String, BTreeSet<SemanticItemId>>,
    trait_supertraits: &BTreeMap<String, Vec<String>>,
    trait_methods: &BTreeMap<String, Vec<String>>,
    item_names: &HashSet<String>,
    top_level_types: &ReceiverTypeMap,
) -> BTreeSet<SemanticItemId> {
    let mut expanded = HashSet::from([name.to_string()]);
    expand_supertrait_names(&mut expanded, trait_supertraits, trait_methods);
    expand_supertrait_method_names(&mut expanded, trait_supertraits);
    if let Some((value_name, method_name)) = name.split_once("::")
        && let Some(receiver_type) = top_level_types.get(value_name)
        && receiver_type.module.is_none()
    {
        let method_root = impl_method_reachability_name(&receiver_type.name, method_name);
        if item_names.contains(&method_root) {
            expanded.insert(method_root);
        }
    }
    expanded.remove(name);
    expanded
        .into_iter()
        .filter_map(|name| name_index.get(&name))
        .flat_map(|ids| ids.iter().cloned())
        .collect()
}

fn semantic_synthetic_receiver_method_ids_for_module(
    module: &str,
    item_names: &HashSet<String>,
    top_level_types: &ReceiverTypeMap,
) -> BTreeSet<SemanticItemId> {
    let mut ids = BTreeSet::new();
    for (value_name, receiver_type) in top_level_types {
        if receiver_type.module.is_some() {
            continue;
        }
        let receiver_prefix = format!("{}::", receiver_type.name);
        for item_name in item_names {
            let Some(method_name) = item_name.strip_prefix(&receiver_prefix) else {
                continue;
            };
            let synthetic_name = impl_method_reachability_name(value_name, method_name);
            if synthetic_name == *item_name {
                continue;
            }
            ids.insert(SemanticItemId {
                module: module.to_string(),
                kind: SemanticItemKind::SyntheticRoot,
                name: synthetic_name,
            });
        }
    }
    ids
}

pub(super) fn semantic_reachability_graph_enabled() -> bool {
    std::env::var_os("GORS_SEMANTIC_REACHABILITY_AUDIT").is_some()
}

fn semantic_item_name_index(
    module: &str,
    items: &[syn::Item],
) -> BTreeMap<String, BTreeSet<SemanticItemId>> {
    let mut index: BTreeMap<String, BTreeSet<SemanticItemId>> = BTreeMap::new();
    for item in items {
        for id in semantic_item_ids_for_item(module, item) {
            index.entry(id.name.clone()).or_default().insert(id);
        }
    }
    index
}

fn semantic_local_ref_ids_for_names(
    names: &HashSet<String>,
    name_index: &BTreeMap<String, BTreeSet<SemanticItemId>>,
) -> BTreeSet<SemanticItemId> {
    names
        .iter()
        .filter_map(|name| name_index.get(name).map(|ids| (name, ids)))
        .flat_map(|(name, ids)| {
            let has_precise_name = name.contains("::")
                || ids.iter().any(|id| {
                    !matches!(
                        id.kind,
                        SemanticItemKind::ImplItem | SemanticItemKind::TraitItem
                    )
                });
            ids.iter()
                .filter(move |id| {
                    has_precise_name
                        || !matches!(
                            id.kind,
                            SemanticItemKind::ImplItem | SemanticItemKind::TraitItem
                        )
                })
                .cloned()
        })
        .collect()
}

fn item_has_preserve_attr(item: &syn::Item) -> bool {
    match item {
        syn::Item::Impl(item_impl) => generated_attrs::attrs_preserve_for_dce(&item_impl.attrs),
        _ => false,
    }
}

fn semantic_self_reachable_trait_impl_ids_by_self(
    module: &str,
    items: &[syn::Item],
    top_level_names: &HashSet<String>,
) -> BTreeMap<String, BTreeSet<SemanticItemId>> {
    let mut ids_by_self = BTreeMap::new();
    for item in items {
        let syn::Item::Impl(item_impl) = item else {
            continue;
        };
        let Some((_, trait_path, _)) = &item_impl.trait_ else {
            continue;
        };
        let Some(trait_name) = trait_path
            .segments
            .last()
            .map(|segment| segment.ident.to_string())
        else {
            continue;
        };
        let impl_ids = semantic_item_ids_for_item(module, item);
        for self_name in self_type_reachability_names(&item_impl.self_ty) {
            let names = HashSet::from([self_name.clone()]);
            if trait_impl_can_follow_self_reachability(
                trait_path,
                &trait_name,
                &item_impl.self_ty,
                &names,
                top_level_names,
            ) {
                ids_by_self
                    .entry(self_name)
                    .or_insert_with(BTreeSet::new)
                    .extend(impl_ids.iter().cloned());
            }
        }
    }
    ids_by_self
}

fn semantic_noop_trait_impl_ids_by_trait(
    module: &str,
    items: &[syn::Item],
) -> BTreeMap<String, BTreeSet<SemanticItemId>> {
    let mut ids_by_trait = BTreeMap::new();
    for item in items {
        let syn::Item::Impl(item_impl) = item else {
            continue;
        };
        let Some(self_name) = named_self_type(&item_impl.self_ty) else {
            continue;
        };
        if !interface_hooks::is_noop_type_name(&self_name) {
            continue;
        }
        let Some((_, trait_path, _)) = &item_impl.trait_ else {
            continue;
        };
        let Some(trait_name) = trait_path
            .segments
            .last()
            .map(|segment| segment.ident.to_string())
        else {
            continue;
        };
        let self_prefix = format!("{self_name}::");
        let impl_ids = semantic_item_ids_for_item(module, item)
            .into_iter()
            .filter(|id| {
                id.name
                    .strip_prefix(&self_prefix)
                    .is_some_and(|member| !interface_hooks::is_runtime_hook(member))
            });
        ids_by_trait
            .entry(trait_name)
            .or_insert_with(BTreeSet::new)
            .extend(impl_ids);
    }
    ids_by_trait
}

fn semantic_source_reachability_names(
    item: &syn::Item,
    source_id: &SemanticItemId,
) -> HashSet<String> {
    let mut names = HashSet::from([source_id.name.clone()]);
    match (item, source_id.kind) {
        (syn::Item::Trait(item_trait), SemanticItemKind::TraitItem) => {
            let trait_name = item_trait.ident.to_string();
            let member_name = source_id
                .name
                .rsplit_once("::")
                .map(|(_, member)| member)
                .unwrap_or(&source_id.name);
            names.insert(trait_name.clone());
            names.insert(member_name.to_string());
            names.insert(impl_method_reachability_name(&trait_name, member_name));
        }
        (syn::Item::Impl(item_impl), SemanticItemKind::ImplItem) => {
            let member_name = source_id
                .name
                .rsplit_once("::")
                .map(|(_, member)| member)
                .unwrap_or(&source_id.name);
            names.insert(member_name.to_string());
            for self_name in self_type_reachability_names(&item_impl.self_ty) {
                names.insert(self_name.clone());
                names.insert(impl_method_reachability_name(&self_name, member_name));
            }
            if let Some((_, trait_path, _)) = &item_impl.trait_
                && let Some(trait_name) = trait_path
                    .segments
                    .last()
                    .map(|segment| segment.ident.to_string())
            {
                names.insert(trait_name.clone());
                names.insert(impl_method_reachability_name(&trait_name, member_name));
            }
        }
        _ => {}
    }
    names
}

fn semantic_item_ids_for_item(module: &str, item: &syn::Item) -> Vec<SemanticItemId> {
    let mut ids = Vec::new();
    if let Some(name) = item_name(item)
        && let Some(kind) = semantic_item_kind(item)
    {
        ids.push(SemanticItemId {
            module: module.to_string(),
            kind,
            name,
        });
    }

    match item {
        syn::Item::Impl(item_impl) => {
            let self_names = self_type_reachability_names(&item_impl.self_ty);
            for impl_item in &item_impl.items {
                let (kind, name) = match impl_item {
                    syn::ImplItem::Const(item) => {
                        (SemanticItemKind::ImplItem, item.ident.to_string())
                    }
                    syn::ImplItem::Fn(item) => {
                        (SemanticItemKind::ImplItem, item.sig.ident.to_string())
                    }
                    syn::ImplItem::Type(item) => {
                        (SemanticItemKind::ImplItem, item.ident.to_string())
                    }
                    _ => continue,
                };
                ids.push(SemanticItemId {
                    module: module.to_string(),
                    kind,
                    name: name.clone(),
                });
                for self_name in &self_names {
                    ids.push(SemanticItemId {
                        module: module.to_string(),
                        kind,
                        name: impl_method_reachability_name(self_name, &name),
                    });
                }
            }
        }
        syn::Item::Trait(item_trait) => {
            let trait_name = item_trait.ident.to_string();
            for trait_item in &item_trait.items {
                let (kind, name) = match trait_item {
                    syn::TraitItem::Const(item) => {
                        (SemanticItemKind::TraitItem, item.ident.to_string())
                    }
                    syn::TraitItem::Fn(item) => {
                        (SemanticItemKind::TraitItem, item.sig.ident.to_string())
                    }
                    syn::TraitItem::Type(item) => {
                        (SemanticItemKind::TraitItem, item.ident.to_string())
                    }
                    _ => continue,
                };
                ids.push(SemanticItemId {
                    module: module.to_string(),
                    kind,
                    name: name.clone(),
                });
                ids.push(SemanticItemId {
                    module: module.to_string(),
                    kind,
                    name: impl_method_reachability_name(&trait_name, &name),
                });
            }
        }
        _ => {}
    }
    ids
}

fn semantic_item_kind(item: &syn::Item) -> Option<SemanticItemKind> {
    match item {
        syn::Item::Const(_) => Some(SemanticItemKind::Const),
        syn::Item::Fn(_) => Some(SemanticItemKind::Function),
        syn::Item::Static(_) => Some(SemanticItemKind::Static),
        syn::Item::Trait(_) => Some(SemanticItemKind::Trait),
        syn::Item::Macro(_) => Some(SemanticItemKind::Macro),
        syn::Item::Enum(_) | syn::Item::Struct(_) | syn::Item::Type(_) | syn::Item::Union(_) => {
            Some(SemanticItemKind::Type)
        }
        _ => None,
    }
}

fn semantic_root_ids_for_module(
    module: &CompiledModule,
    has_main: bool,
    name_index: &BTreeMap<String, BTreeSet<SemanticItemId>>,
) -> Vec<SemanticItemId> {
    let root_names = if module.is_main && has_main {
        HashSet::from(["main".to_string()])
    } else if module.is_main {
        exported_item_reachability_names(&module.file.items)
    } else {
        HashSet::new()
    };
    root_names
        .iter()
        .filter_map(|name| name_index.get(name))
        .flat_map(|ids| ids.iter().cloned())
        .collect()
}
