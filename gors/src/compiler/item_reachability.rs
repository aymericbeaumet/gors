use super::syn_inspect::{
    item_macro_name, item_name, macro_token_item_names, named_self_type, path_mentions_name,
    path_starts_with, self_type_reachability_names, type_mentions_name,
};

pub(super) fn reachable_item_for_names(
    item: &syn::Item,
    names: &std::collections::HashSet<String>,
    item_names: &std::collections::HashSet<String>,
    top_level_names: &std::collections::HashSet<String>,
    roots: &std::collections::HashSet<String>,
) -> Option<syn::Item> {
    if matches!(item, syn::Item::Use(_)) {
        return Some(item.clone());
    }

    if let syn::Item::Trait(item_trait) = item {
        let trait_name = item_trait.ident.to_string();
        if !names.contains(&trait_name) {
            return None;
        }
        if is_ambient_trait_name(&trait_name) {
            return Some(item.clone());
        }
        if roots.contains(&trait_name) {
            let mut preserved = item_trait.clone();
            super::generated_attrs::allow_dead_code(&mut preserved.attrs);
            return Some(syn::Item::Trait(preserved));
        }
        let mut filtered = item_trait.clone();
        filtered.items.retain(|trait_item| match trait_item {
            syn::TraitItem::Fn(func) => {
                let name = func.sig.ident.to_string();
                super::interface_hooks::is_runtime_hook(&name)
                    || trait_item_name_reachable(&trait_name, &name, names)
            }
            syn::TraitItem::Const(konst) => {
                trait_item_name_reachable(&trait_name, &konst.ident.to_string(), names)
            }
            syn::TraitItem::Type(ty) => {
                trait_item_name_reachable(&trait_name, &ty.ident.to_string(), names)
            }
            syn::TraitItem::Macro(item_macro) => item_macro
                .mac
                .path
                .segments
                .last()
                .is_some_and(|seg| names.contains(&seg.ident.to_string())),
            _ => false,
        });
        return Some(syn::Item::Trait(filtered));
    }

    if let syn::Item::Macro(item_macro) = item {
        let name = item_macro_name(item_macro);
        let token_names = macro_token_item_names(&item_macro.mac.tokens, item_names);
        return (name.as_ref().is_some_and(|name| names.contains(name))
            || token_names.iter().any(|name| names.contains(name)))
        .then(|| item.clone());
    }

    if item_name(item).is_some_and(|name| names.contains(&name)) {
        return Some(item.clone());
    }

    let syn::Item::Impl(item_impl) = item else {
        return None;
    };

    let trait_reachable = item_impl.trait_.as_ref().is_some_and(|(_, path, _)| {
        path.segments.last().is_some_and(|seg| {
            let name = seg.ident.to_string();
            names.contains(&name) && !is_ambient_trait_name(&name)
        }) || path_mentions_name(path, names)
    });
    let self_name = named_self_type(&item_impl.self_ty);
    let self_names = self_type_reachability_names(&item_impl.self_ty);
    let self_reachable = type_mentions_name(&item_impl.self_ty, names)
        || self_names.iter().any(|name| {
            names
                .iter()
                .any(|root| root.starts_with(&format!("{name}::")))
        });

    if trait_reachable {
        let trait_name = item_impl
            .trait_
            .as_ref()
            .and_then(|(_, path, _)| path.segments.last())
            .map(|seg| seg.ident.to_string());
        if let Some(self_name) = named_self_type(&item_impl.self_ty)
            && trait_name
                .as_ref()
                .is_none_or(|trait_name| !roots.contains(trait_name))
            && item_names.contains(&self_name)
            && !names.contains(&self_name)
        {
            return None;
        }
        if let Some(trait_name) = trait_name {
            if is_ambient_trait_name(&trait_name) {
                return Some(item.clone());
            }
            if item_impl.trait_.as_ref().is_some_and(|(_, path, _)| {
                qualified_external_trait_path(path, &trait_name, top_level_names)
            }) {
                let mut preserved = item_impl.clone();
                super::generated_attrs::allow_dead_code(&mut preserved.attrs);
                return Some(syn::Item::Impl(preserved));
            }
            if roots.contains(&trait_name) {
                let mut preserved = item_impl.clone();
                super::generated_attrs::allow_dead_code(&mut preserved.attrs);
                return Some(syn::Item::Impl(preserved));
            }
            let mut filtered = item_impl.clone();
            filtered.items.retain(|impl_item| match impl_item {
                syn::ImplItem::Fn(func) => {
                    let name = func.sig.ident.to_string();
                    super::interface_hooks::is_runtime_hook(&name)
                        || trait_item_name_reachable(&trait_name, &name, names)
                }
                syn::ImplItem::Const(konst) => {
                    trait_item_name_reachable(&trait_name, &konst.ident.to_string(), names)
                }
                syn::ImplItem::Type(ty) => {
                    trait_item_name_reachable(&trait_name, &ty.ident.to_string(), names)
                }
                syn::ImplItem::Macro(item_macro) => item_macro
                    .mac
                    .path
                    .segments
                    .last()
                    .is_some_and(|seg| names.contains(&seg.ident.to_string())),
                _ => false,
            });
            return Some(syn::Item::Impl(filtered));
        }
        return Some(syn::Item::Impl(item_impl.clone()));
    }
    if !self_reachable {
        return None;
    }
    if item_impl.trait_.is_some() {
        if self_name
            .as_deref()
            .is_some_and(super::interface_hooks::is_noop_type_name)
        {
            return Some(item.clone());
        }
        if let Some((_, path, _)) = &item_impl.trait_
            && let Some(trait_name) = path.segments.last().map(|seg| seg.ident.to_string())
            && !trait_impl_can_follow_self_reachability(path, &trait_name, names, top_level_names)
        {
            return None;
        }
        return Some(item.clone());
    }

    let mut filtered = item_impl.clone();
    filtered.items.retain(|impl_item| match impl_item {
        syn::ImplItem::Fn(func) => {
            impl_item_name_reachable(&self_names, &func.sig.ident.to_string(), names)
        }
        syn::ImplItem::Const(konst) => {
            impl_item_name_reachable(&self_names, &konst.ident.to_string(), names)
        }
        syn::ImplItem::Type(ty) => {
            impl_item_name_reachable(&self_names, &ty.ident.to_string(), names)
        }
        syn::ImplItem::Macro(item_macro) => item_macro
            .mac
            .path
            .segments
            .last()
            .is_some_and(|seg| names.contains(&seg.ident.to_string())),
        _ => false,
    });

    (!filtered.items.is_empty()).then(|| syn::Item::Impl(filtered))
}

fn impl_item_name_reachable(
    self_names: &[String],
    item_name: &str,
    names: &std::collections::HashSet<String>,
) -> bool {
    names.contains(item_name)
        || self_names
            .iter()
            .any(|self_name| names.contains(&impl_method_reachability_name(self_name, item_name)))
}

fn trait_item_name_reachable(
    trait_name: &str,
    item_name: &str,
    names: &std::collections::HashSet<String>,
) -> bool {
    names.contains(item_name)
        || names.contains(&impl_method_reachability_name(trait_name, item_name))
}

fn qualified_external_trait_path(
    path: &syn::Path,
    trait_name: &str,
    top_level_names: &std::collections::HashSet<String>,
) -> bool {
    !top_level_names.contains(trait_name)
        && (path.leading_colon.is_some() || path.segments.len() > 1)
}

fn trait_impl_can_follow_self_reachability(
    path: &syn::Path,
    trait_name: &str,
    names: &std::collections::HashSet<String>,
    top_level_names: &std::collections::HashSet<String>,
) -> bool {
    if names.contains(trait_name)
        || names
            .iter()
            .any(|name| name.starts_with(&format!("{trait_name}::")))
    {
        return true;
    }
    if external_trait_impl_requires_explicit_reachability(path, trait_name) {
        return false;
    }
    !top_level_names.contains(trait_name)
}

fn external_trait_impl_requires_explicit_reachability(path: &syn::Path, trait_name: &str) -> bool {
    // Do not generalize this to all qualified external traits: generated
    // interface conversions for builtin errors and local packages still depend
    // on self-reachable external impls.
    trait_name == "Stringer" && path_starts_with(path, &["crate", "fmt"])
}

pub(super) fn impl_method_reachability_name(self_name: &str, method_name: &str) -> String {
    format!("{self_name}::{method_name}")
}

fn is_ambient_trait_name(name: &str) -> bool {
    matches!(
        name,
        "AsMut"
            | "AsRef"
            | "Clone"
            | "Copy"
            | "Debug"
            | "Default"
            | "Deref"
            | "DerefMut"
            | "Display"
            | "From"
            | "Append"
            | "BitcastFrom"
            | "ByteSeq"
            | "Cap"
            | "Clear"
            | "Complex64Value"
            | "Complex128Value"
            | "Imag"
            | "Len"
            | "Real"
            | "StringValue"
            | "__GorsReflectKindValue"
            | "comparable"
            | "Into"
            | "ToString"
    )
}
