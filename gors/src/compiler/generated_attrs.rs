pub(super) fn allow_dead_code(attrs: &mut Vec<syn::Attribute>) {
    let has_attr = attrs.iter().any(attribute_allows_dead_code);
    if !has_attr {
        attrs.push(syn::parse_quote!(#[allow(dead_code)]));
    }
}

pub(super) fn preserve_for_dce(attrs: &mut Vec<syn::Attribute>) {
    allow_dead_code(attrs);
    if !attrs.iter().any(attribute_preserves_for_dce) {
        let marker = crate::generated_names::PRESERVE_IMPORTED_INTERFACE_IMPL_DOC;
        attrs.push(syn::parse_quote!(#[doc = #marker]));
    }
}

pub(super) fn mark_external_local_interface_impl(attrs: &mut Vec<syn::Attribute>) {
    allow_dead_code(attrs);
    if !attrs
        .iter()
        .any(attribute_marks_external_local_interface_impl)
    {
        let marker = crate::generated_names::EXTERNAL_LOCAL_INTERFACE_IMPL_DOC;
        attrs.push(syn::parse_quote!(#[doc = #marker]));
    }
}

pub(super) fn allow_dead_code_on_item(item: &mut syn::Item) {
    if let Some(attrs) = item_attrs_mut(item) {
        allow_dead_code(attrs);
    }
}

pub(super) fn item_preserves_for_dce(item: &syn::Item) -> bool {
    item_attrs(item).is_some_and(|attrs| attrs_preserve_for_dce(attrs))
}

fn item_attrs(item: &syn::Item) -> Option<&Vec<syn::Attribute>> {
    match item {
        syn::Item::Const(item) => Some(&item.attrs),
        syn::Item::Enum(item) => Some(&item.attrs),
        syn::Item::Fn(item) => Some(&item.attrs),
        syn::Item::Impl(item) => Some(&item.attrs),
        syn::Item::Macro(item) => Some(&item.attrs),
        syn::Item::Mod(item) => Some(&item.attrs),
        syn::Item::Static(item) => Some(&item.attrs),
        syn::Item::Struct(item) => Some(&item.attrs),
        syn::Item::Trait(item) => Some(&item.attrs),
        syn::Item::Type(item) => Some(&item.attrs),
        syn::Item::Union(item) => Some(&item.attrs),
        syn::Item::Use(item) => Some(&item.attrs),
        _ => None,
    }
}

fn item_attrs_mut(item: &mut syn::Item) -> Option<&mut Vec<syn::Attribute>> {
    match item {
        syn::Item::Const(item) => Some(&mut item.attrs),
        syn::Item::Enum(item) => Some(&mut item.attrs),
        syn::Item::Fn(item) => Some(&mut item.attrs),
        syn::Item::Impl(item) => Some(&mut item.attrs),
        syn::Item::Macro(item) => Some(&mut item.attrs),
        syn::Item::Mod(item) => Some(&mut item.attrs),
        syn::Item::Static(item) => Some(&mut item.attrs),
        syn::Item::Struct(item) => Some(&mut item.attrs),
        syn::Item::Trait(item) => Some(&mut item.attrs),
        syn::Item::Type(item) => Some(&mut item.attrs),
        syn::Item::Union(item) => Some(&mut item.attrs),
        syn::Item::Use(item) => Some(&mut item.attrs),
        _ => None,
    }
}

pub(super) fn attribute_allows_dead_code(attr: &syn::Attribute) -> bool {
    if !attr.path().is_ident("allow") {
        return false;
    }

    let mut allows_dead_code = false;
    let _ = attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("dead_code") {
            allows_dead_code = true;
        }
        Ok(())
    });
    allows_dead_code
}

pub(super) fn attribute_preserves_for_dce(attr: &syn::Attribute) -> bool {
    crate::generated_names::doc_attr_value(attr)
        .is_some_and(|doc| doc == crate::generated_names::PRESERVE_IMPORTED_INTERFACE_IMPL_DOC)
}

pub(super) fn attrs_preserve_for_dce(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(attribute_preserves_for_dce)
}

pub(super) fn attribute_marks_external_local_interface_impl(attr: &syn::Attribute) -> bool {
    crate::generated_names::doc_attr_value(attr)
        .is_some_and(|doc| doc == crate::generated_names::EXTERNAL_LOCAL_INTERFACE_IMPL_DOC)
}

pub(super) fn attrs_mark_external_local_interface_impl(attrs: &[syn::Attribute]) -> bool {
    attrs
        .iter()
        .any(attribute_marks_external_local_interface_impl)
}
