use super::{interface_hooks, syn_inspect};

pub(super) fn items(
    interface_ident: &syn::Ident,
    trait_items: &[syn::TraitItem],
) -> Vec<syn::Item> {
    let noop_ident = syn::Ident::new(
        &format!("__GorsNoop{interface_ident}"),
        proc_macro2::Span::mixed_site(),
    );
    let mut impl_methods = Vec::new();
    for trait_item in trait_items {
        let syn::TraitItem::Fn(trait_fn) = trait_item else {
            continue;
        };
        let sig = trait_fn.sig.clone();
        let block = if sig.ident == interface_hooks::AS_ANY_METHOD {
            syn::parse_quote!({ None })
        } else if sig.ident == interface_hooks::CLONE_BOX_METHOD {
            syn::parse_quote!({ Box::new(#noop_ident::default()) as Box<dyn #interface_ident> })
        } else if matches!(sig.output, syn::ReturnType::Default) {
            syn::parse_quote!({})
        } else {
            syn::parse_quote!({ crate::builtin::panic_value("called no-op interface method") })
        };
        impl_methods.push(syn::ImplItemFn {
            attrs: vec![],
            vis: syn::Visibility::Inherited,
            defaultness: None,
            sig,
            block,
        });
    }

    vec![
        syn::parse_quote! {
            #[derive(Clone, Default)]
            pub struct #noop_ident;
        },
        syn::parse_quote! {
            impl #interface_ident for #noop_ident {
                #(#impl_methods)*
            }
        },
    ]
}

pub(super) fn impl_item_for_signature(sig: syn::Signature) -> syn::ImplItem {
    let block = if sig.ident == interface_hooks::AS_ANY_METHOD {
        syn::parse_quote!({ None })
    } else if matches!(sig.output, syn::ReturnType::Default) {
        syn::parse_quote!({})
    } else {
        syn::parse_quote!({ crate::builtin::panic_value("called no-op interface method") })
    };
    syn::ImplItem::Fn(syn::ImplItemFn {
        attrs: vec![],
        vis: syn::Visibility::Inherited,
        defaultness: None,
        sig,
        block,
    })
}

pub(super) fn impl_items_for_trait_fns<'a>(
    methods: impl IntoIterator<Item = &'a syn::TraitItemFn>,
) -> Vec<syn::ImplItem> {
    methods
        .into_iter()
        .map(|trait_fn| impl_item_for_signature(trait_fn.sig.clone()))
        .collect()
}

pub(super) fn supertrait_impls(
    items: &[syn::Item],
    mut external_impl_items: impl FnMut(&str) -> Vec<syn::ImplItem>,
) -> Vec<syn::Item> {
    let trait_methods = syn_inspect::trait_method_fns(items);
    let trait_supertraits: std::collections::BTreeMap<String, Vec<syn::Path>> = items
        .iter()
        .filter_map(|item| {
            let syn::Item::Trait(item_trait) = item else {
                return None;
            };
            let supertraits = item_trait
                .supertraits
                .iter()
                .filter_map(|bound| match bound {
                    syn::TypeParamBound::Trait(trait_bound) => Some(trait_bound.path.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            Some((item_trait.ident.to_string(), supertraits))
        })
        .collect();
    let noop_names = items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Struct(item_struct)
                if item_struct.ident.to_string().starts_with("__GorsNoop") =>
            {
                Some(item_struct.ident.to_string())
            }
            _ => None,
        })
        .collect::<std::collections::BTreeSet<_>>();
    let mut out = Vec::new();

    for item in items {
        let syn::Item::Trait(item_trait) = item else {
            continue;
        };
        let noop_ident = syn::Ident::new(
            &format!("__GorsNoop{}", item_trait.ident),
            proc_macro2::Span::mixed_site(),
        );
        if !noop_names.contains(&noop_ident.to_string()) {
            continue;
        }
        let mut pending = trait_supertraits
            .get(&item_trait.ident.to_string())
            .cloned()
            .unwrap_or_default();
        let mut seen = std::collections::BTreeSet::new();
        while let Some(path) = pending.pop() {
            let Some(name) = path
                .segments
                .last()
                .map(|segment| segment.ident.to_string())
            else {
                continue;
            };
            let seen_name =
                interface_hooks::trait_path_interface_name(&path).unwrap_or_else(|| name.clone());
            if !seen.insert(seen_name) {
                continue;
            }
            if interface_hooks::trait_path_is_local(&path)
                && let Some(next) = trait_supertraits.get(&name)
            {
                pending.extend(next.iter().cloned());
            }
            let impl_items = if interface_hooks::trait_path_is_local(&path) {
                if !trait_methods.contains_key(&name) && !trait_supertraits.contains_key(&name) {
                    continue;
                }
                impl_items_for_trait_fns(
                    trait_methods
                        .get(&name)
                        .into_iter()
                        .flat_map(|methods| methods.iter()),
                )
            } else if let Some(interface_name) = interface_hooks::trait_path_interface_name(&path) {
                let items = external_impl_items(&interface_name);
                if items.is_empty() {
                    continue;
                }
                items
            } else {
                continue;
            };
            out.push(syn::parse_quote! {
                impl #path for #noop_ident {
                    #(#impl_items)*
                }
            });
        }
    }

    out
}
