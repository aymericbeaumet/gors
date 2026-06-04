pub(super) fn allow_dead_code(attrs: &mut Vec<syn::Attribute>) {
    let has_attr = attrs.iter().any(attribute_allows_dead_code);
    if !has_attr {
        attrs.push(syn::parse_quote!(#[allow(dead_code)]));
    }
}

pub(super) fn allow_dead_code_on_item(item: &mut syn::Item) {
    if let Some(attrs) = item_attrs_mut(item) {
        allow_dead_code(attrs);
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
