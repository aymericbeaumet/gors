pub(super) fn allow_dead_code(attrs: &mut Vec<syn::Attribute>) {
    let has_attr = attrs.iter().any(attribute_allows_dead_code);
    if !has_attr {
        attrs.push(syn::parse_quote!(#[allow(dead_code)]));
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
