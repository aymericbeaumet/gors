const DCE_PRESERVE_DOC: &str = "gors:preserve-imported-interface-impl";

pub(super) fn allow_dead_code(attrs: &mut Vec<syn::Attribute>) {
    let has_attr = attrs.iter().any(attribute_allows_dead_code);
    if !has_attr {
        attrs.push(syn::parse_quote!(#[allow(dead_code)]));
    }
}

pub(super) fn preserve_for_dce(attrs: &mut Vec<syn::Attribute>) {
    allow_dead_code(attrs);
    if !attrs.iter().any(attribute_preserves_for_dce) {
        attrs.push(syn::parse_quote!(#[doc = #DCE_PRESERVE_DOC]));
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

pub(super) fn attribute_preserves_for_dce(attr: &syn::Attribute) -> bool {
    let syn::Meta::NameValue(meta) = &attr.meta else {
        return false;
    };
    if !meta.path.is_ident("doc") {
        return false;
    }
    let syn::Expr::Lit(expr_lit) = &meta.value else {
        return false;
    };
    let syn::Lit::Str(doc) = &expr_lit.lit else {
        return false;
    };
    doc.value() == DCE_PRESERVE_DOC
}

pub(super) fn attrs_preserve_for_dce(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(attribute_preserves_for_dce)
}
