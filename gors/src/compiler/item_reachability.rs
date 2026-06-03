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
            super::allow_dead_code_attr(&mut preserved.attrs);
            return Some(syn::Item::Trait(preserved));
        }
        let mut filtered = item_trait.clone();
        filtered.items.retain(|trait_item| match trait_item {
            syn::TraitItem::Fn(func) => {
                let name = func.sig.ident.to_string();
                super::is_runtime_interface_hook(&name)
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
                super::allow_dead_code_attr(&mut preserved.attrs);
                return Some(syn::Item::Impl(preserved));
            }
            if roots.contains(&trait_name) {
                let mut preserved = item_impl.clone();
                super::allow_dead_code_attr(&mut preserved.attrs);
                return Some(syn::Item::Impl(preserved));
            }
            let mut filtered = item_impl.clone();
            filtered.items.retain(|impl_item| match impl_item {
                syn::ImplItem::Fn(func) => {
                    let name = func.sig.ident.to_string();
                    super::is_runtime_interface_hook(&name)
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
            .is_some_and(is_noop_interface_type_name)
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

pub(super) fn path_starts_with(path: &syn::Path, expected: &[&str]) -> bool {
    if path.segments.len() < expected.len() {
        return false;
    }
    path.segments
        .iter()
        .zip(expected)
        .all(|(segment, expected)| segment.ident == *expected)
}

pub(super) fn is_noop_interface_type_name(name: &str) -> bool {
    name.starts_with("__GorsNoop")
}

pub(super) fn impl_method_reachability_name(self_name: &str, method_name: &str) -> String {
    format!("{self_name}::{method_name}")
}

pub(super) fn named_self_type(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(path) => named_self_type_from_path(&path.path),
        syn::Type::Reference(reference) => named_self_type(&reference.elem),
        _ => None,
    }
}

fn named_self_type_from_path(path: &syn::Path) -> Option<String> {
    let last = path.segments.last()?;
    if matches!(last.ident.to_string().as_str(), "Arc" | "Mutex" | "GorsPtr")
        && let syn::PathArguments::AngleBracketed(args) = &last.arguments
        && let Some(name) = args.args.iter().find_map(|arg| match arg {
            syn::GenericArgument::Type(ty) => named_self_type(ty),
            _ => None,
        })
    {
        return Some(name);
    }
    Some(last.ident.to_string())
}

fn direct_self_type(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(path) => path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string()),
        syn::Type::Reference(reference) => direct_self_type(&reference.elem),
        _ => None,
    }
}

pub(super) fn self_type_reachability_names(ty: &syn::Type) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(name) = named_self_type(ty) {
        names.push(name);
    }
    if let Some(name) = direct_self_type(ty)
        && !names.contains(&name)
    {
        names.push(name);
    }
    names
}

pub(super) fn item_name(item: &syn::Item) -> Option<String> {
    match item {
        syn::Item::Const(item) => Some(item.ident.to_string()),
        syn::Item::Enum(item) => Some(item.ident.to_string()),
        syn::Item::Fn(item) => Some(item.sig.ident.to_string()),
        syn::Item::Static(item) => Some(item.ident.to_string()),
        syn::Item::Struct(item) => Some(item.ident.to_string()),
        syn::Item::Trait(item) => Some(item.ident.to_string()),
        syn::Item::Type(item) => Some(item.ident.to_string()),
        syn::Item::Union(item) => Some(item.ident.to_string()),
        syn::Item::Macro(item) => item_macro_name(item),
        _ => None,
    }
}

pub(super) fn item_macro_name(item: &syn::ItemMacro) -> Option<String> {
    item.ident
        .as_ref()
        .map(std::string::ToString::to_string)
        .or_else(|| {
            item.mac
                .path
                .segments
                .last()
                .map(|seg| seg.ident.to_string())
        })
}

pub(super) fn macro_token_item_names(
    tokens: &proc_macro2::TokenStream,
    item_names: &std::collections::HashSet<String>,
) -> std::collections::HashSet<String> {
    fn collect(
        tokens: proc_macro2::TokenStream,
        item_names: &std::collections::HashSet<String>,
        names: &mut std::collections::HashSet<String>,
    ) {
        for token in tokens {
            match token {
                proc_macro2::TokenTree::Ident(ident) => {
                    let name = ident.to_string();
                    if item_names.contains(&name) {
                        names.insert(name);
                    }
                }
                proc_macro2::TokenTree::Group(group) => {
                    collect(group.stream(), item_names, names);
                }
                proc_macro2::TokenTree::Literal(_) | proc_macro2::TokenTree::Punct(_) => {}
            }
        }
    }

    let mut names = std::collections::HashSet::new();
    collect(tokens.clone(), item_names, &mut names);
    names
}

pub(super) fn type_mentions_name(
    ty: &syn::Type,
    names: &std::collections::HashSet<String>,
) -> bool {
    match ty {
        syn::Type::Array(array) => type_mentions_name(&array.elem, names),
        syn::Type::Group(group) => type_mentions_name(&group.elem, names),
        syn::Type::Paren(paren) => type_mentions_name(&paren.elem, names),
        syn::Type::Path(path) => path_mentions_name(&path.path, names),
        syn::Type::Reference(reference) => type_mentions_name(&reference.elem, names),
        syn::Type::Ptr(ptr) => type_mentions_name(&ptr.elem, names),
        syn::Type::Slice(slice) => type_mentions_name(&slice.elem, names),
        syn::Type::TraitObject(trait_object) => {
            trait_object.bounds.iter().any(|bound| match bound {
                syn::TypeParamBound::Trait(trait_bound) => {
                    path_mentions_name(&trait_bound.path, names)
                }
                _ => false,
            })
        }
        syn::Type::Tuple(tuple) => tuple.elems.iter().any(|ty| type_mentions_name(ty, names)),
        _ => false,
    }
}

pub(super) fn path_mentions_name(
    path: &syn::Path,
    names: &std::collections::HashSet<String>,
) -> bool {
    path.segments.iter().any(|seg| {
        names.contains(&seg.ident.to_string())
            || match &seg.arguments {
                syn::PathArguments::AngleBracketed(args) => {
                    args.args.iter().any(|arg| match arg {
                        syn::GenericArgument::Type(ty) => type_mentions_name(ty, names),
                        syn::GenericArgument::AssocType(assoc) => type_mentions_name(&assoc.ty, names),
                        syn::GenericArgument::Constraint(constraint) => {
                            constraint.bounds.iter().any(|bound| match bound {
                                syn::TypeParamBound::Trait(trait_bound) => {
                                    path_mentions_name(&trait_bound.path, names)
                                }
                                _ => false,
                            })
                        }
                        _ => false,
                    })
                }
                syn::PathArguments::Parenthesized(args) => {
                    args.inputs.iter().any(|ty| type_mentions_name(ty, names))
                        || matches!(&args.output, syn::ReturnType::Type(_, ty) if type_mentions_name(ty, names))
                }
                syn::PathArguments::None => false,
            }
    })
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
