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
use quote::ToTokens;

pub(super) fn reachable_stdlib_items(
    items: &[syn::Item],
    roots: &std::collections::HashSet<String>,
    module_names: &std::collections::HashSet<String>,
) -> ReachableItems {
    let items_fingerprint = reachability_cache::items_fingerprint(items);
    reachable_stdlib_items_with_fingerprint(items, &items_fingerprint, roots, module_names)
}

pub(super) fn reachable_stdlib_items_with_fingerprint(
    items: &[syn::Item],
    items_fingerprint: &str,
    roots: &std::collections::HashSet<String>,
    module_names: &std::collections::HashSet<String>,
) -> ReachableItems {
    let cache_key = reachability_cache::cache_key_with_items_fingerprint(
        items_fingerprint,
        roots,
        module_names,
    );
    if let Some(entry) = reachability_cache::cached_items(&cache_key) {
        return entry;
    }

    let (entry, _) = compute_reachable_stdlib_items(items, roots, module_names);
    reachability_cache::store_items(cache_key, &entry);
    entry
}

fn compute_reachable_stdlib_items(
    items: &[syn::Item],
    roots: &std::collections::HashSet<String>,
    module_names: &std::collections::HashSet<String>,
) -> (ReachableItems, usize) {
    let mut names = roots.clone();
    let mut keep = std::collections::HashSet::new();
    let mut external_refs = std::collections::HashMap::new();
    let mut processed_states = vec![None; items.len()];
    let mut processed_state_count = 0;
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
        let names_before = names.len();
        expand_supertrait_names(&mut names, &trait_supertraits, &trait_methods);
        expand_supertrait_method_names(&mut names, &trait_supertraits);
        expand_top_level_receiver_method_names(&mut names, &top_level_types, &item_names);
        for (idx, (item, processed_state)) in
            items.iter().zip(processed_states.iter_mut()).enumerate()
        {
            let can_expand = reachable_item_can_expand(item, roots);
            if !can_expand && processed_state.is_some() {
                continue;
            }
            let Some(mut reachable_item) =
                reachable_item_for_names(item, &names, &item_names, &top_level_names, roots)
            else {
                continue;
            };
            keep.insert(idx);

            let state = if can_expand {
                reachable_item.to_token_stream().to_string()
            } else {
                String::new()
            };
            if processed_state.as_ref() == Some(&state) {
                continue;
            }
            *processed_state = Some(state);
            processed_state_count += 1;

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
                names.insert(name);
            }
            required_module_roots::merge_refs(&mut external_refs, refs);
        }
        if names.len() == names_before {
            break;
        }
    }

    let entry = ReachableItems {
        keep,
        refs: external_refs,
        names,
    };
    (entry, processed_state_count)
}

fn reachable_item_can_expand(item: &syn::Item, roots: &std::collections::HashSet<String>) -> bool {
    match item {
        syn::Item::Trait(item_trait) => !roots.contains(&item_trait.ident.to_string()),
        syn::Item::Impl(item_impl) => item_impl.trait_.is_none(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn reachable_method_body_retains_private_helper() {
        let roots = HashSet::from(["MapIter".to_string(), "MapIter::Key".to_string()]);
        let file: syn::File = syn::parse_quote! {
            pub struct MapIter;

            impl MapIter {
                pub fn Key(&self) -> isize {
                    copy_val(1)
                }
            }

            fn copy_val(value: isize) -> isize {
                value
            }

            fn dead_helper() -> isize {
                0
            }
        };
        let module_names = HashSet::new();
        let reachable = reachable_stdlib_items(&file.items, &roots, &module_names);

        assert!(
            reachable.names.contains("copy_val"),
            "{:?}",
            reachable.names
        );
        assert!(
            reachable.keep.iter().any(|index| file
                .items
                .get(*index)
                .is_some_and(|item| item_named(item, "copy_val"))),
            "reachable names: {:?}",
            reachable.names
        );
        assert!(!reachable.names.contains("dead_helper"));
    }

    #[test]
    fn reachable_value_receiver_method_retains_method_called_through_promoted_self_cell() {
        let roots = HashSet::from(["Time".to_string(), "Time::Round".to_string()]);
        let file: syn::File = syn::parse_quote! {
            #[derive(Clone)]
            pub struct Time;

            impl Time {
                pub fn Add(self, delta: isize) -> Time {
                    let _ = delta;
                    self
                }

                pub fn Round(self, delta: isize) -> Time {
                    let receiver = std::sync::Arc::new(std::sync::Mutex::new(self));
                    (|| (((*receiver.lock().unwrap()).clone()).Add(delta)))()
                }

                pub fn Dead(self) -> Time {
                    self
                }
            }
        };
        let module_names = HashSet::new();

        let reachable = reachable_stdlib_items(&file.items, &roots, &module_names);

        assert!(
            reachable.names.contains("Time::Add"),
            "{:?}",
            reachable.names
        );
        assert!(!reachable.names.contains("Time::Dead"));
    }

    #[test]
    fn reachable_method_retains_method_called_through_wrapped_scoped_local() {
        let roots = HashSet::from(["root".to_string()]);
        let file: syn::File = syn::parse_quote! {
            #[derive(Clone)]
            pub struct Value;

            impl Value {
                pub fn Read(self) -> isize {
                    1
                }

                pub fn Dead(self) -> isize {
                    0
                }
            }

            pub fn root(input: Value) -> isize {
                let local = input;
                let wrapped = std::sync::Arc::new(std::sync::Mutex::new(local));
                (((*wrapped.lock().unwrap()).clone()).Read())
            }
        };
        let module_names = HashSet::new();

        let reachable = reachable_stdlib_items(&file.items, &roots, &module_names);

        assert!(
            reachable.names.contains("Value::Read"),
            "{:?}",
            reachable.names
        );
        assert!(!reachable.names.contains("Value::Dead"));
    }

    #[test]
    fn reachable_tuple_receivers_follow_block_and_iife_results() {
        let roots = HashSet::from(["root".to_string()]);
        let file: syn::File = syn::parse_quote! {
            pub struct Format;

            impl Format {
                pub fn FromIife(&self) {}

                pub fn FromBlock(&self) {}

                pub fn Dead(&self) {}
            }

            fn pair() -> (Format, ()) {
                (Format, ())
            }

            pub fn root() {
                let (from_iife, _) = (|| { pair() })();
                from_iife.FromIife();

                let (from_block, _) = {
                    let _before_tail = ();
                    pair()
                };
                from_block.FromBlock();
            }
        };
        let module_names = HashSet::new();

        let reachable = reachable_stdlib_items(&file.items, &roots, &module_names);

        assert!(
            reachable.names.contains("Format::FromIife"),
            "{:?}",
            reachable.names
        );
        assert!(
            reachable.names.contains("Format::FromBlock"),
            "{:?}",
            reachable.names
        );
        assert!(!reachable.names.contains("Format::Dead"));
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

    #[test]
    fn reachable_items_process_each_unchanged_item_state_once() {
        let file: syn::File = syn::parse_quote! {
            fn third() {}

            fn second() {
                third();
            }

            fn first() {
                second();
            }
        };
        let roots = HashSet::from(["first".to_string()]);
        let module_names = HashSet::new();

        let (reachable, processed_state_count) =
            compute_reachable_stdlib_items(&file.items, &roots, &module_names);

        assert_eq!(
            reachable.names,
            HashSet::from([
                "first".to_string(),
                "second".to_string(),
                "third".to_string(),
            ])
        );
        assert_eq!(reachable.keep, HashSet::from([0, 1, 2]));
        assert_eq!(processed_state_count, 3);
    }

    #[test]
    fn boxed_trait_object_cast_from_struct_literal_roots_concrete_impl() {
        let file: syn::File = syn::parse_quote! {
            pub trait Entry {
                fn Name(&mut self) -> String;
            }

            pub struct entryInfo {
                name: String,
            }

            impl Entry for entryInfo {
                fn Name(&mut self) -> String {
                    self.name.clone()
                }
            }

            pub fn wrap(name: String) -> Box<dyn Entry> {
                Box::new(entryInfo { name }) as Box<dyn Entry>
            }
        };
        let roots = HashSet::from(["wrap".to_string()]);
        let module_names = HashSet::new();

        let reachable = reachable_stdlib_items(&file.items, &roots, &module_names);

        assert!(
            reachable.names.contains("entryInfo"),
            "names={:?}",
            reachable.names,
        );
        assert!(
            reachable.names.contains(
                &super::super::item_reachability::trait_impl_reachability_name(
                    "Entry",
                    "entryInfo",
                ),
            ),
            "names={:?}",
            reachable.names,
        );
        assert!(
            reachable.keep.iter().any(|index| matches!(
                file.items.get(*index),
                Some(syn::Item::Impl(item_impl))
                    if item_impl.trait_.as_ref().is_some_and(|(_, path, _)| {
                        path.segments.last().is_some_and(|segment| segment.ident == "Entry")
                    })
            )),
            "names={:?}, keep={:?}",
            reachable.names,
            reachable.keep,
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
