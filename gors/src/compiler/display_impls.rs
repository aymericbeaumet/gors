pub(super) fn prune_without_string_method(items: &mut Vec<syn::Item>) {
    let display_required_types = display_required_types(items);
    items.retain(|item| {
        let syn::Item::Impl(item_impl) = item else {
            return true;
        };
        if !is_display_impl(item_impl) {
            return true;
        }
        super::named_self_type(&item_impl.self_ty)
            .is_none_or(|self_name| display_required_types.contains(&self_name))
    });
}

fn display_required_types(items: &[syn::Item]) -> std::collections::HashSet<String> {
    let mut types = types_with_inherent_string_method(items);
    types.extend(types_with_std_error_impl(items));
    types
}

fn types_with_inherent_string_method(items: &[syn::Item]) -> std::collections::HashSet<String> {
    items
        .iter()
        .filter_map(|item| {
            let syn::Item::Impl(item_impl) = item else {
                return None;
            };
            (item_impl.trait_.is_none() && impl_has_method(item_impl, "String"))
                .then(|| super::named_self_type(&item_impl.self_ty))
                .flatten()
        })
        .collect()
}

fn types_with_std_error_impl(items: &[syn::Item]) -> std::collections::HashSet<String> {
    items
        .iter()
        .filter_map(|item| {
            let syn::Item::Impl(item_impl) = item else {
                return None;
            };
            item_impl
                .trait_
                .as_ref()
                .is_some_and(|(_, path, _)| is_std_error_trait(path))
                .then(|| super::named_self_type(&item_impl.self_ty))
                .flatten()
        })
        .collect()
}

fn impl_has_method(item_impl: &syn::ItemImpl, method_name: &str) -> bool {
    item_impl.items.iter().any(
        |impl_item| matches!(impl_item, syn::ImplItem::Fn(func) if func.sig.ident == method_name),
    )
}

fn is_display_impl(item_impl: &syn::ItemImpl) -> bool {
    item_impl
        .trait_
        .as_ref()
        .is_some_and(|(_, path, _)| path_ends_with(path, "Display"))
}

fn is_std_error_trait(path: &syn::Path) -> bool {
    path_ends_with(path, "Error") && super::path_starts_with(path, &["std", "error"])
}

fn path_ends_with(path: &syn::Path, name: &str) -> bool {
    path.segments
        .last()
        .is_some_and(|segment| segment.ident == name)
}
