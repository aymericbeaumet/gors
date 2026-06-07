use super::{
    item_reachability::reachable_item_for_names,
    reachability_cache::{self, ReachableItems},
    reachability_names::{
        expand_supertrait_method_names, expand_supertrait_names,
        expand_top_level_receiver_method_names, item_reachability_names, top_level_item_names,
        trait_method_names, trait_supertrait_names,
    },
    receiver_type_facts::{
        top_level_collection_element_types, top_level_item_field_types,
        top_level_item_return_types, top_level_item_tuple_return_types, top_level_item_types,
    },
    ref_collection::{RefCollectionContext, collect_refs_from_item},
    required_module_roots,
};

pub(super) fn reachable_stdlib_items(
    items: &[syn::Item],
    roots: &std::collections::HashSet<String>,
    module_names: &std::collections::HashSet<String>,
) -> ReachableItems {
    let cache_key = reachability_cache::cache_key(items, roots, module_names);
    if let Some(entry) = reachability_cache::cached_items(&cache_key) {
        return entry;
    }

    let mut names = roots.clone();
    let mut keep = std::collections::HashSet::new();
    let mut external_refs = std::collections::HashMap::new();
    let item_names = item_reachability_names(items);
    let top_level_names = top_level_item_names(items);
    let top_level_types = top_level_item_types(items, module_names);
    let top_level_field_types = top_level_item_field_types(items, module_names);
    let top_level_element_types = top_level_collection_element_types(items, module_names);
    let top_level_return_types = top_level_item_return_types(items, module_names);
    let top_level_tuple_return_types = top_level_item_tuple_return_types(items, module_names);
    let trait_supertraits = trait_supertrait_names(items);
    let trait_methods = trait_method_names(items);

    loop {
        let mut changed = false;
        changed |= expand_supertrait_names(&mut names, &trait_supertraits, &trait_methods);
        changed |= expand_supertrait_method_names(&mut names, &trait_supertraits);
        changed |=
            expand_top_level_receiver_method_names(&mut names, &top_level_types, &item_names);
        for (idx, item) in items.iter().enumerate() {
            let Some(mut reachable_item) =
                reachable_item_for_names(item, &names, &item_names, &top_level_names, roots)
            else {
                continue;
            };
            changed |= keep.insert(idx);

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
            let (local_names, refs) = collect_refs_from_item(&mut reachable_item, &context);
            for name in local_names {
                changed |= names.insert(name);
            }
            changed |= required_module_roots::merge_refs(&mut external_refs, refs);
        }
        if !changed {
            break;
        }
    }

    let entry = ReachableItems {
        keep,
        refs: external_refs,
        names,
    };
    reachability_cache::store_items(cache_key, &entry);
    entry
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn reachable_reflect_mapiter_key_retains_copyval() -> Result<(), Box<dyn std::error::Error>> {
        let roots = HashSet::from(["MapIter".to_string(), "MapIter::Key".to_string()]);
        let module = crate::resolve::resolve_with_roots("reflect", &roots)
            .ok_or_else(|| std::io::Error::other("resolve reflect"))?;
        let Some((_, items)) = module.content else {
            return Err(std::io::Error::other("reflect items").into());
        };
        let module_names = HashSet::from(["reflect".to_string()]);
        let reachable = reachable_stdlib_items(&items, &roots, &module_names);

        assert!(reachable.names.contains("copyVal"), "{:?}", reachable.names);
        assert!(
            reachable.keep.iter().any(|index| items
                .get(*index)
                .is_some_and(|item| item_named(item, "copyVal"))),
            "reachable names: {:?}",
            reachable.names
        );
        Ok(())
    }

    #[test]
    fn reachable_refs_follow_private_field_method_helpers_to_external_roots() {
        let file: syn::File = syn::parse_quote! {
            pub struct Header;

            pub struct Writer {
                hdr: Header,
            }

            impl Header {
                fn allowedFormats(&self) -> bool {
                    crate::reflect::DeepEqual(
                        Box::new(()) as Box<dyn std::any::Any>,
                        Box::new(()) as Box<dyn std::any::Any>,
                    )
                }
            }

            impl Writer {
                pub fn WriteHeader(mut tw: crate::builtin::GorsPtr<Self>) -> bool {
                    (|| {
                        ((((tw).lock().unwrap()).hdr).clone()).allowedFormats()
                    })()
                }
            }
        };
        let roots = HashSet::from(["Writer".to_string(), "Writer::WriteHeader".to_string()]);
        let module_names = HashSet::from(["reflect".to_string()]);

        let reachable = reachable_stdlib_items(&file.items, &roots, &module_names);

        assert!(
            reachable.names.contains("Header::allowedFormats"),
            "{:?}",
            reachable.names
        );
        assert!(
            reachable
                .refs
                .get("reflect")
                .is_some_and(|refs| refs.contains("DeepEqual")),
            "{:?}",
            reachable.refs
        );
    }

    fn item_named(item: &syn::Item, expected: &str) -> bool {
        match item {
            syn::Item::Fn(func) => func.sig.ident == expected,
            syn::Item::Const(konst) => konst.ident == expected,
            syn::Item::Static(static_item) => static_item.ident == expected,
            syn::Item::Struct(strukt) => strukt.ident == expected,
            syn::Item::Trait(trait_item) => trait_item.ident == expected,
            syn::Item::Type(type_item) => type_item.ident == expected,
            _ => false,
        }
    }
}
