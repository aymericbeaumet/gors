pub(super) fn path_starts_with(path: &syn::Path, expected: &[&str]) -> bool {
    if path.segments.len() < expected.len() {
        return false;
    }
    path.segments
        .iter()
        .zip(expected)
        .all(|(segment, expected)| segment.ident == *expected)
}

pub(super) fn pat_ident_name(pat: &syn::Pat) -> Option<String> {
    match pat {
        syn::Pat::Ident(pat_ident) => Some(pat_ident.ident.to_string()),
        syn::Pat::Type(pat_type) => pat_ident_name(&pat_type.pat),
        _ => None,
    }
}

pub(super) fn pat_ident_names(pat: &syn::Pat) -> Vec<String> {
    match pat {
        syn::Pat::Ident(ident) => vec![ident.ident.to_string()],
        syn::Pat::Reference(reference) => pat_ident_names(&reference.pat),
        syn::Pat::Tuple(tuple) => tuple.elems.iter().flat_map(pat_ident_names).collect(),
        syn::Pat::TupleStruct(tuple) => tuple.elems.iter().flat_map(pat_ident_names).collect(),
        syn::Pat::Type(pat_type) => pat_ident_names(&pat_type.pat),
        _ => Vec::new(),
    }
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
                        syn::GenericArgument::AssocType(assoc) => {
                            type_mentions_name(&assoc.ty, names)
                        }
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

pub(super) fn trait_method_fns(
    items: &[syn::Item],
) -> std::collections::BTreeMap<String, Vec<syn::TraitItemFn>> {
    let mut traits = std::collections::BTreeMap::new();
    for item in items {
        let syn::Item::Trait(item_trait) = item else {
            continue;
        };
        let methods = item_trait
            .items
            .iter()
            .filter_map(|trait_item| match trait_item {
                syn::TraitItem::Fn(trait_fn) => Some(trait_fn.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        if !methods.is_empty() {
            traits.insert(item_trait.ident.to_string(), methods);
        }
    }
    traits
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    #[test]
    fn pat_ident_name_reads_ident_and_typed_patterns() {
        let ident: syn::Pat = parse_quote! { value };
        assert_eq!(pat_ident_name(&ident).as_deref(), Some("value"));

        let typed: syn::Pat = syn::Pat::Type(parse_quote! { value: String });
        assert_eq!(pat_ident_name(&typed).as_deref(), Some("value"));

        let tuple: syn::Pat = parse_quote! { (left, right) };
        assert_eq!(pat_ident_name(&tuple), None);
    }

    #[test]
    fn pat_ident_names_reads_nested_bindings() {
        let pat: syn::Pat = parse_quote! { (first, &second, Some(third)) };
        assert_eq!(pat_ident_names(&pat), ["first", "second", "third"]);
    }
}
