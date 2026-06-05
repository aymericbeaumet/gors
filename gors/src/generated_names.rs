pub const AS_ANY_METHOD: &str = "__gors_as_any";
pub const CLONE_BOX_METHOD: &str = "__gors_clone_box";
pub const ERROR_EXT_TRAIT: &str = "__GorsErrorExt";
pub const FMT_FLUSH_HOOK: &str = "__gors_flush_fmt";
pub const FMT_FLUSH_METHOD_DOC_PREFIX: &str = "gors:fmt-flush-method=";
pub const FMT_FLUSH_SOURCE_DOC_PREFIX: &str = "gors:fmt-flush-source=";
pub const NOOP_INTERFACE: &str = "__GorsNoopInterface";

fn ident(name: &str) -> syn::Ident {
    syn::Ident::new(name, proc_macro2::Span::mixed_site())
}

pub fn as_any_method_ident() -> syn::Ident {
    ident(AS_ANY_METHOD)
}

pub fn clone_box_method_ident() -> syn::Ident {
    ident(CLONE_BOX_METHOD)
}

pub fn error_ext_trait_ident() -> syn::Ident {
    ident(ERROR_EXT_TRAIT)
}

pub fn fmt_flush_hook_ident() -> syn::Ident {
    ident(FMT_FLUSH_HOOK)
}

pub fn fmt_flush_method_doc(method: &str) -> String {
    format!("{FMT_FLUSH_METHOD_DOC_PREFIX}{method}")
}

pub fn fmt_flush_method_from_doc(doc: &str) -> Option<&str> {
    doc.strip_prefix(FMT_FLUSH_METHOD_DOC_PREFIX)
        .filter(|method| !method.is_empty())
}

pub fn fmt_flush_method_from_attr(attr: &syn::Attribute) -> Option<String> {
    doc_attr_value(attr).and_then(|doc| fmt_flush_method_from_doc(&doc).map(str::to_owned))
}

pub fn fmt_flush_source_doc(source_field: &str) -> String {
    format!("{FMT_FLUSH_SOURCE_DOC_PREFIX}{source_field}")
}

pub fn fmt_flush_source_from_doc(doc: &str) -> Option<&str> {
    doc.strip_prefix(FMT_FLUSH_SOURCE_DOC_PREFIX)
        .filter(|source| !source.is_empty())
}

pub fn fmt_flush_source_from_attr(attr: &syn::Attribute) -> Option<String> {
    doc_attr_value(attr).and_then(|doc| fmt_flush_source_from_doc(&doc).map(str::to_owned))
}

pub fn doc_attr_value(attr: &syn::Attribute) -> Option<String> {
    let syn::Meta::NameValue(meta) = &attr.meta else {
        return None;
    };
    if !meta.path.is_ident("doc") {
        return None;
    }
    let syn::Expr::Lit(expr_lit) = &meta.value else {
        return None;
    };
    let syn::Lit::Str(doc) = &expr_lit.lit else {
        return None;
    };
    Some(doc.value())
}

pub fn noop_interface_ident() -> syn::Ident {
    ident(NOOP_INTERFACE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_flush_method_doc_round_trips_method_name() {
        let doc = fmt_flush_method_doc("emit");

        assert_eq!(fmt_flush_method_from_doc(&doc), Some("emit"));
    }

    #[test]
    fn fmt_flush_source_doc_round_trips_source_field() {
        let doc = fmt_flush_source_doc("scratch");

        assert_eq!(fmt_flush_source_from_doc(&doc), Some("scratch"));
    }

    #[test]
    fn fmt_flush_marker_attrs_read_doc_attributes() {
        let method: syn::Attribute = syn::parse_quote! {
            #[doc = "gors:fmt-flush-method=emit"]
        };
        let source: syn::Attribute = syn::parse_quote! {
            #[doc = "gors:fmt-flush-source=scratch"]
        };

        assert_eq!(fmt_flush_method_from_attr(&method).as_deref(), Some("emit"));
        assert_eq!(
            fmt_flush_source_from_attr(&source).as_deref(),
            Some("scratch")
        );
    }

    #[test]
    fn fmt_flush_docs_reject_empty_or_unrelated_docs() {
        assert_eq!(fmt_flush_method_from_doc(FMT_FLUSH_METHOD_DOC_PREFIX), None);
        assert_eq!(fmt_flush_method_from_doc("gors:other"), None);
        assert_eq!(fmt_flush_source_from_doc(FMT_FLUSH_SOURCE_DOC_PREFIX), None);
        assert_eq!(fmt_flush_source_from_doc("gors:other"), None);
    }

    #[test]
    fn doc_attr_value_rejects_non_doc_attributes() {
        let attr: syn::Attribute = syn::parse_quote! {
            #[allow(dead_code)]
        };

        assert!(doc_attr_value(&attr).is_none());
    }
}
