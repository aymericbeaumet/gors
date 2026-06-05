use std::collections::{BTreeMap, BTreeSet, HashSet};

use super::item_reachability::impl_method_reachability_name;
use super::receiver_type_facts::ReceiverTypeMap;
use super::syn_inspect::{item_name, named_self_type, self_type_reachability_names};

pub(super) fn exported_item_reachability_names(items: &[syn::Item]) -> HashSet<String> {
    let mut roots = HashSet::new();
    for item in items {
        if let Some(name) = item_name(item)
            && is_go_exported_name(&name)
        {
            roots.insert(name);
        }
        if let syn::Item::Impl(item_impl) = item
            && let Some(self_name) = named_self_type(&item_impl.self_ty)
        {
            for impl_item in &item_impl.items {
                match impl_item {
                    syn::ImplItem::Const(item) if is_go_exported_name(&item.ident.to_string()) => {
                        roots.insert(impl_method_reachability_name(
                            &self_name,
                            &item.ident.to_string(),
                        ));
                    }
                    syn::ImplItem::Fn(item) if is_go_exported_name(&item.sig.ident.to_string()) => {
                        roots.insert(impl_method_reachability_name(
                            &self_name,
                            &item.sig.ident.to_string(),
                        ));
                    }
                    syn::ImplItem::Type(item) if is_go_exported_name(&item.ident.to_string()) => {
                        roots.insert(impl_method_reachability_name(
                            &self_name,
                            &item.ident.to_string(),
                        ));
                    }
                    _ => {}
                }
            }
        }
        if let syn::Item::Trait(item_trait) = item {
            let trait_name = item_trait.ident.to_string();
            for trait_item in &item_trait.items {
                if let syn::TraitItem::Fn(func) = trait_item {
                    let name = func.sig.ident.to_string();
                    roots.insert(name.clone());
                    roots.insert(impl_method_reachability_name(&trait_name, &name));
                }
            }
        }
    }
    roots
}

fn is_go_exported_name(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_uppercase)
}

pub(super) fn item_reachability_names(items: &[syn::Item]) -> HashSet<String> {
    let mut names = HashSet::new();
    for item in items {
        if let Some(name) = item_name(item) {
            names.insert(name);
        }
        if let syn::Item::Impl(item_impl) = item {
            let self_names = self_type_reachability_names(&item_impl.self_ty);
            for impl_item in &item_impl.items {
                match impl_item {
                    syn::ImplItem::Fn(func) => {
                        let name = func.sig.ident.to_string();
                        names.insert(name.clone());
                        for self_name in &self_names {
                            names.insert(impl_method_reachability_name(self_name, &name));
                        }
                    }
                    syn::ImplItem::Const(konst) => {
                        let name = konst.ident.to_string();
                        names.insert(name.clone());
                        for self_name in &self_names {
                            names.insert(impl_method_reachability_name(self_name, &name));
                        }
                    }
                    syn::ImplItem::Type(ty) => {
                        let name = ty.ident.to_string();
                        names.insert(name.clone());
                        for self_name in &self_names {
                            names.insert(impl_method_reachability_name(self_name, &name));
                        }
                    }
                    _ => {}
                }
            }
        }
        if let syn::Item::Trait(item_trait) = item {
            let trait_name = item_trait.ident.to_string();
            for trait_item in &item_trait.items {
                if let syn::TraitItem::Fn(func) = trait_item {
                    let name = func.sig.ident.to_string();
                    names.insert(name.clone());
                    names.insert(impl_method_reachability_name(&trait_name, &name));
                }
            }
        }
    }
    names
}

pub(super) fn top_level_item_names(items: &[syn::Item]) -> HashSet<String> {
    items.iter().filter_map(item_name).collect()
}

pub(super) fn trait_supertrait_names(items: &[syn::Item]) -> BTreeMap<String, Vec<String>> {
    items
        .iter()
        .filter_map(|item| {
            let syn::Item::Trait(item_trait) = item else {
                return None;
            };
            let supertraits = item_trait
                .supertraits
                .iter()
                .filter_map(|bound| match bound {
                    syn::TypeParamBound::Trait(trait_bound) => trait_bound
                        .path
                        .segments
                        .last()
                        .map(|segment| segment.ident.to_string()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            Some((item_trait.ident.to_string(), supertraits))
        })
        .collect()
}

pub(super) fn trait_method_names(items: &[syn::Item]) -> BTreeMap<String, Vec<String>> {
    items
        .iter()
        .filter_map(|item| {
            let syn::Item::Trait(item_trait) = item else {
                return None;
            };
            let methods = item_trait
                .items
                .iter()
                .filter_map(|trait_item| match trait_item {
                    syn::TraitItem::Fn(func) => Some(func.sig.ident.to_string()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            Some((item_trait.ident.to_string(), methods))
        })
        .collect()
}

pub(super) fn expand_supertrait_names(
    names: &mut HashSet<String>,
    supertraits: &BTreeMap<String, Vec<String>>,
    trait_methods: &BTreeMap<String, Vec<String>>,
) -> bool {
    let roots = names.iter().cloned().collect::<Vec<_>>();
    let mut changed = false;
    for trait_name in roots {
        let mut stack = supertraits.get(&trait_name).cloned().unwrap_or_default();
        let mut seen = BTreeSet::new();
        while let Some(supertrait) = stack.pop() {
            if !seen.insert(supertrait.clone()) {
                continue;
            }
            changed |= names.insert(supertrait.clone());
            for method in trait_methods.get(&supertrait).into_iter().flatten() {
                changed |= names.insert(impl_method_reachability_name(&supertrait, method));
            }
            if let Some(next) = supertraits.get(&supertrait) {
                stack.extend(next.iter().cloned());
            }
        }
    }
    changed
}

pub(super) fn expand_supertrait_method_names(
    names: &mut HashSet<String>,
    supertraits: &BTreeMap<String, Vec<String>>,
) -> bool {
    let mut changed = false;
    let roots = names
        .iter()
        .filter_map(|name| {
            name.split_once("::")
                .map(|(trait_name, method_name)| (trait_name.to_string(), method_name.to_string()))
        })
        .collect::<Vec<_>>();
    for (trait_name, method_name) in roots {
        let mut stack = supertraits.get(&trait_name).cloned().unwrap_or_default();
        let mut seen = BTreeSet::new();
        while let Some(supertrait) = stack.pop() {
            if !seen.insert(supertrait.clone()) {
                continue;
            }
            changed |= names.insert(impl_method_reachability_name(&supertrait, &method_name));
            if let Some(next) = supertraits.get(&supertrait) {
                stack.extend(next.iter().cloned());
            }
        }
    }
    changed
}

pub(super) fn expand_top_level_receiver_method_names(
    names: &mut HashSet<String>,
    top_level_types: &ReceiverTypeMap,
    item_names: &HashSet<String>,
) -> bool {
    let roots = names
        .iter()
        .filter_map(|name| {
            name.split_once("::")
                .map(|(value_name, method_name)| (value_name.to_string(), method_name.to_string()))
        })
        .collect::<Vec<_>>();
    let mut changed = false;
    for (value_name, receiver_type) in top_level_types {
        if receiver_type.module.is_some() || !names.contains(value_name) {
            continue;
        }
        for (root_value_name, method_name) in &roots {
            if root_value_name != value_name {
                continue;
            }
            let method_root = impl_method_reachability_name(&receiver_type.name, method_name);
            if item_names.contains(&method_root) {
                changed |= names.insert(method_root);
            }
        }
    }
    changed
}
