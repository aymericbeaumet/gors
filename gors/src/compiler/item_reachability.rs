use super::syn_inspect::{
    item_macro_name, item_name, macro_declared_item_names, macro_token_item_names, named_self_type,
    path_is, path_mentions_name, path_starts_with, self_type_reachability_names,
    type_mentions_name,
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
        let declared_names = macro_declared_item_names(&item_macro.mac.tokens);
        let token_names = macro_token_item_names(&item_macro.mac.tokens, item_names);
        return (name.as_ref().is_some_and(|name| names.contains(name))
            || declared_names.iter().any(|name| names.contains(name))
            || token_names.iter().any(|name| names.contains(name)))
        .then(|| item.clone());
    }

    if item_name(item).is_some_and(|name| names.contains(&name)) {
        return Some(item.clone());
    }

    let syn::Item::Impl(item_impl) = item else {
        return None;
    };
    if super::generated_attrs::attrs_preserve_for_dce(&item_impl.attrs)
        && !super::generated_attrs::attrs_mark_removable_interface_fallback(&item_impl.attrs)
    {
        return Some(item.clone());
    }

    let self_name = named_self_type(&item_impl.self_ty);
    let self_names = self_type_reachability_names(&item_impl.self_ty);
    let self_reachable = type_mentions_name(&item_impl.self_ty, names)
        || self_names.iter().any(|name| {
            names.contains(name)
                || names
                    .iter()
                    .any(|root| root.starts_with(&format!("{name}::")))
        });

    if let Some((_, path, _)) = &item_impl.trait_ {
        let trait_name = item_impl
            .trait_
            .as_ref()
            .and_then(|(_, path, _)| path.segments.last())
            .map(|seg| seg.ident.to_string());
        let Some(trait_name) = trait_name else {
            return self_reachable.then(|| syn::Item::Impl(item_impl.clone()));
        };
        let trait_reachability_name = if trait_path_requires_qualified_impl_root(path) {
            trait_path_reachability_name(path).unwrap_or_else(|| trait_name.clone())
        } else {
            trait_name.clone()
        };
        let impl_target_names = impl_target_reachability_names(&item_impl.self_ty);
        let trait_reachable = (names.contains(&trait_name) && !is_ambient_trait_name(&trait_name))
            || path_mentions_name(path, names);
        let explicit_impl_reachable = impl_target_names.iter().any(|self_name| {
            names.contains(&trait_impl_reachability_name(
                &trait_reachability_name,
                self_name,
            ))
        });
        let impl_member_reachable = item_impl.items.iter().any(|impl_item| {
            impl_item_member_name(impl_item).is_some_and(|member_name| {
                impl_item_name_reachable(&self_names, &member_name, names)
            })
        });
        let hook_only_impl = impl_items_are_only_runtime_hooks(&item_impl.items);
        let self_only_names = self_names
            .iter()
            .cloned()
            .collect::<std::collections::HashSet<_>>();
        let follows_self_reachability = self_reachable
            && trait_impl_can_follow_self_reachability(
                path,
                &trait_name,
                &item_impl.self_ty,
                &self_only_names,
                top_level_names,
            );
        let external_local_impl_reachable =
            super::generated_attrs::attrs_mark_external_local_interface_impl(&item_impl.attrs)
                && trait_reachable
                && self_reachable;
        let required_supertrait_impl_reachable = item_impl
            .attrs
            .iter()
            .filter_map(crate::generated_names::interface_impl_required_by_from_attr)
            .any(|composite_trait| {
                impl_target_names.iter().any(|self_name| {
                    names.contains(&trait_impl_reachability_name(&composite_trait, self_name))
                })
            });
        let keep_impl = explicit_impl_reachable
            || impl_member_reachable
            || follows_self_reachability
            || external_local_impl_reachable
            || required_supertrait_impl_reachable
            || (trait_reachable && is_ambient_trait_name(&trait_name))
            || (trait_reachable && item_impl.items.is_empty())
            || (trait_reachable && self_reachable && hook_only_impl)
            || (self_reachable && is_runtime_support_trait_name(&trait_name))
            || (trait_reachable && is_generated_box_trait_impl(&item_impl.self_ty))
            || (trait_reachable
                && qualified_external_trait_path(path, &trait_name, top_level_names));

        if self_name
            .as_deref()
            .is_some_and(super::interface_hooks::is_noop_type_name)
        {
            return Some(item.clone());
        }
        if keep_impl {
            if qualified_external_trait_path(path, &trait_name, top_level_names) {
                let mut preserved = item_impl.clone();
                super::generated_attrs::allow_dead_code(&mut preserved.attrs);
                return Some(syn::Item::Impl(preserved));
            }
            return Some(item.clone());
        }
        return None;
    }

    if !self_reachable {
        return None;
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

fn impl_items_are_only_runtime_hooks(items: &[syn::ImplItem]) -> bool {
    !items.is_empty()
        && items.iter().all(|item| {
            impl_item_member_name(item)
                .as_deref()
                .is_some_and(super::interface_hooks::is_runtime_hook)
        })
}

fn impl_item_member_name(item: &syn::ImplItem) -> Option<String> {
    match item {
        syn::ImplItem::Fn(func) => Some(func.sig.ident.to_string()),
        syn::ImplItem::Const(konst) => Some(konst.ident.to_string()),
        syn::ImplItem::Type(ty) => Some(ty.ident.to_string()),
        syn::ImplItem::Macro(item_macro) => item_macro
            .mac
            .path
            .segments
            .last()
            .map(|seg| seg.ident.to_string()),
        _ => None,
    }
}

fn impl_item_name_reachable(
    self_names: &[String],
    item_name: &str,
    names: &std::collections::HashSet<String>,
) -> bool {
    self_names
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

pub(super) fn trait_impl_can_follow_self_reachability(
    path: &syn::Path,
    trait_name: &str,
    self_ty: &syn::Type,
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
    if is_rust_operator_trait_path(path)
        || is_builtin_runtime_trait_path(path)
        || is_host_resource_trait_impl(path, self_ty)
        || is_generated_pointer_cell_impl(self_ty)
        || is_runtime_error_trait_path(path, trait_name)
    {
        return true;
    }
    !top_level_names.contains(trait_name)
        && path.leading_colon.is_none()
        && path.segments.len() == 1
}

fn is_generated_pointer_cell_impl(self_ty: &syn::Type) -> bool {
    match self_ty {
        syn::Type::Path(path) => path
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "GorsPtr"),
        syn::Type::Reference(reference) => is_generated_pointer_cell_impl(&reference.elem),
        _ => false,
    }
}

fn is_generated_box_trait_impl(self_ty: &syn::Type) -> bool {
    match self_ty {
        syn::Type::Path(path) => path
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "Box"),
        syn::Type::Reference(reference) => is_generated_box_trait_impl(&reference.elem),
        _ => false,
    }
}

fn is_runtime_error_trait_path(path: &syn::Path, trait_name: &str) -> bool {
    matches!(trait_name, "error" | "Error")
        && (path_is(path, &["crate", "builtin", "error"])
            || path_is(path, &["std", "error", "Error"]))
}

fn is_rust_operator_trait_path(path: &syn::Path) -> bool {
    path.segments.len() == 3 && path_starts_with(path, &["std", "ops"])
}

fn is_builtin_runtime_trait_path(path: &syn::Path) -> bool {
    path.segments.len() == 3 && path_starts_with(path, &["crate", "builtin"])
}

fn is_host_resource_trait_impl(path: &syn::Path, self_ty: &syn::Type) -> bool {
    named_self_type(self_ty).as_deref() == Some("File")
        && path.segments.len() == 3
        && path_starts_with(path, &["crate", "io"])
}

pub(super) fn impl_method_reachability_name(self_name: &str, method_name: &str) -> String {
    format!("{self_name}::{method_name}")
}

pub(super) fn trait_impl_reachability_name(trait_name: &str, self_name: &str) -> String {
    format!("impl {trait_name} for {self_name}")
}

pub(super) fn qualified_trait_impl_reachability_name(
    trait_module: &str,
    trait_name: &str,
    self_name: &str,
) -> String {
    format!("impl {trait_module}::{trait_name} for {self_name}")
}

pub(super) fn trait_path_reachability_name(path: &syn::Path) -> Option<String> {
    let segments = path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .filter(|segment| segment != "crate")
        .collect::<Vec<_>>();
    (!segments.is_empty()).then(|| segments.join("::"))
}

fn trait_path_requires_qualified_impl_root(path: &syn::Path) -> bool {
    let segments = path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .filter(|segment| segment != "crate")
        .collect::<Vec<_>>();
    segments.len() > 1
        && !matches!(
            segments.first().map(String::as_str),
            Some("std" | "core" | "alloc")
        )
}

fn impl_target_reachability_names(ty: &syn::Type) -> Vec<String> {
    let Some(name) = named_self_type(ty) else {
        return Vec::new();
    };
    let mut direct = ty;
    let mut borrowed_mutably = false;
    while let syn::Type::Reference(reference) = direct {
        borrowed_mutably |= reference.mutability.is_some();
        direct = &reference.elem;
    }
    if let syn::Type::Path(path) = direct
        && path
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "GorsPtr")
    {
        return vec![format!("GorsPtr<{name}>")];
    }
    if borrowed_mutably {
        return vec![format!("&mut {name}")];
    }
    vec![name]
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

fn is_runtime_support_trait_name(name: &str) -> bool {
    matches!(
        name,
        "error" | "GorsOwnedSliceStorage" | "GorsReflectOps" | "ProjectedCell" | "ProjectedGuard"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_external_impl_root_does_not_retain_same_basename_other_module() {
        let first: syn::Item = syn::parse_quote! {
            impl crate::first_io::Reader for Source {}
        };
        let second: syn::Item = syn::parse_quote! {
            impl crate::second_io::Reader for Source {}
        };
        let names = std::collections::HashSet::from([
            "Source".to_string(),
            qualified_trait_impl_reachability_name("first_io", "Reader", "Source"),
        ]);
        let item_names = std::collections::HashSet::from(["Source".to_string()]);
        let top_level_names = item_names.clone();

        assert!(
            reachable_item_for_names(&first, &names, &item_names, &top_level_names, &names)
                .is_some()
        );
        assert!(
            reachable_item_for_names(&second, &names, &item_names, &top_level_names, &names)
                .is_none()
        );
    }

    #[test]
    fn qualified_impl_roots_preserve_exact_pointer_target_shape() {
        let value: syn::Item = syn::parse_quote! {
            impl crate::io::Reader for Source {}
        };
        let pointer: syn::Item = syn::parse_quote! {
            impl crate::io::Reader for crate::builtin::GorsPtr<Source> {}
        };
        let names = std::collections::HashSet::from([
            "Source".to_string(),
            qualified_trait_impl_reachability_name("io", "Reader", "GorsPtr<Source>"),
        ]);
        let item_names = std::collections::HashSet::from(["Source".to_string()]);
        let top_level_names = item_names.clone();

        assert!(
            reachable_item_for_names(&value, &names, &item_names, &top_level_names, &names)
                .is_none()
        );
        assert!(
            reachable_item_for_names(&pointer, &names, &item_names, &top_level_names, &names)
                .is_some()
        );
    }

    #[test]
    fn standard_library_trait_impls_keep_unqualified_runtime_roots() {
        let display: syn::Item = syn::parse_quote! {
            impl std::fmt::Display for GorsNilPointer {
                fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    Ok(())
                }
            }
        };
        let names = std::collections::HashSet::from([
            "GorsNilPointer".to_string(),
            trait_impl_reachability_name("Display", "GorsNilPointer"),
        ]);
        let item_names = std::collections::HashSet::from(["GorsNilPointer".to_string()]);
        let top_level_names = item_names.clone();

        assert!(
            reachable_item_for_names(&display, &names, &item_names, &top_level_names, &names)
                .is_some()
        );
    }
}
