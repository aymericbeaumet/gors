use super::syn_inspect::named_self_type;

pub(super) const AS_ANY_METHOD: &str = "__gors_as_any";
pub(super) const CLONE_BOX_METHOD: &str = "__gors_clone_box";

pub(super) fn is_runtime_hook(name: &str) -> bool {
    matches!(name, AS_ANY_METHOD | CLONE_BOX_METHOD)
}

pub(super) fn is_noop_type_name(name: &str) -> bool {
    name.starts_with("__GorsNoop")
}

pub(super) fn trait_path_is_local(path: &syn::Path) -> bool {
    path.leading_colon.is_none() && path.segments.len() == 1
}

pub(super) fn trait_path_interface_name(path: &syn::Path) -> Option<String> {
    let segments = path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>();
    match segments.as_slice() {
        [] => None,
        [single] => Some(single.clone()),
        [crate_name, rest @ ..] if crate_name == "crate" => Some(rest.join(".")),
        _ => Some(segments.join(".")),
    }
}

pub(super) fn clone_box_impl_item(trait_path: &syn::Path, can_clone_self: bool) -> syn::ImplItem {
    if can_clone_self {
        syn::parse_quote! {
            fn __gors_clone_box(&self) -> Box<dyn #trait_path> {
                Box::new(self.clone()) as Box<dyn #trait_path>
            }
        }
    } else {
        syn::parse_quote! {
            fn __gors_clone_box(&self) -> Box<dyn #trait_path> {
                crate::builtin::panic_value("cloned non-clone interface value")
            }
        }
    }
}

pub(super) fn add_missing_clone_hooks(items: &mut [syn::Item]) {
    let clone_box_traits = items
        .iter()
        .filter_map(|item| {
            let syn::Item::Trait(item_trait) = item else {
                return None;
            };
            let has_clone_box = item_trait.items.iter().any(|trait_item| {
                matches!(trait_item, syn::TraitItem::Fn(func) if func.sig.ident == CLONE_BOX_METHOD)
            });
            has_clone_box.then(|| item_trait.ident.to_string())
        })
        .collect::<std::collections::BTreeSet<_>>();
    if clone_box_traits.is_empty() {
        return;
    }

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
        if !clone_box_traits.contains(&trait_name) {
            continue;
        }
        if item_impl.items.iter().any(|impl_item| {
            matches!(impl_item, syn::ImplItem::Fn(func) if func.sig.ident == CLONE_BOX_METHOD)
        }) {
            continue;
        }

        let item = if named_self_type(&item_impl.self_ty)
            .as_deref()
            .is_some_and(is_noop_type_name)
        {
            syn::parse_quote! {
                fn __gors_clone_box(&self) -> Box<dyn #trait_path> {
                    Box::new(Self::default()) as Box<dyn #trait_path>
                }
            }
        } else {
            syn::parse_quote! {
                fn __gors_clone_box(&self) -> Box<dyn #trait_path> {
                    crate::builtin::panic_value("cloned non-clone interface value")
                }
            }
        };
        item_impl.items.push(item);
    }
}
