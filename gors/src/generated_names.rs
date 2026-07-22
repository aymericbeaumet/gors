pub const AS_ANY_METHOD: &str = "__gors_as_any";
pub const CLONE_BOX_METHOD: &str = "__gors_clone_box";
pub const INTERFACE_KEY_METHOD: &str = "__gors_interface_key";
pub const PACKAGE_INIT_FN: &str = "__gors_init";
pub const ERROR_EXT_TRAIT: &str = "__GorsErrorExt";
pub const FMT_FLUSH_HOOK: &str = "__gors_flush_fmt";
pub const FMT_FLUSH_METHOD_DOC_PREFIX: &str = "gors:fmt-flush-method=";
pub const FMT_FLUSH_SOURCE_DOC_PREFIX: &str = "gors:fmt-flush-source=";
pub const NOOP_INTERFACE: &str = "__GorsNoopInterface";
pub const EXTERNAL_LOCAL_INTERFACE_IMPL_DOC: &str = "gors:external-local-interface-impl";
pub const PRESERVE_IMPORTED_INTERFACE_IMPL_DOC: &str = "gors:preserve-imported-interface-impl";
pub const REMOVABLE_INTERFACE_FALLBACK_DOC: &str = "gors:removable-interface-fallback";
pub const INTERFACE_IMPL_REQUIRED_BY_DOC_PREFIX: &str = "gors:interface-impl-required-by=";
pub const INTERFACE_ASSERTION_CANDIDATE_DOC_PREFIX: &str = "gors:interface-assertion-candidate=";

fn ident(name: &str) -> syn::Ident {
    syn::Ident::new(name, proc_macro2::Span::mixed_site())
}

pub fn as_any_method_ident() -> syn::Ident {
    ident(AS_ANY_METHOD)
}

pub fn clone_box_method_ident() -> syn::Ident {
    ident(CLONE_BOX_METHOD)
}

pub fn interface_key_method_ident() -> syn::Ident {
    ident(INTERFACE_KEY_METHOD)
}

pub fn package_init_ident() -> syn::Ident {
    ident(PACKAGE_INIT_FN)
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

pub fn interface_impl_required_by_doc(interface: &str) -> String {
    format!("{INTERFACE_IMPL_REQUIRED_BY_DOC_PREFIX}{interface}")
}

pub fn interface_impl_required_by_from_doc(doc: &str) -> Option<&str> {
    doc.strip_prefix(INTERFACE_IMPL_REQUIRED_BY_DOC_PREFIX)
        .filter(|interface| !interface.is_empty())
}

pub fn interface_impl_required_by_from_attr(attr: &syn::Attribute) -> Option<String> {
    doc_attr_value(attr)
        .and_then(|doc| interface_impl_required_by_from_doc(&doc).map(str::to_owned))
}

pub fn interface_assertion_candidate_doc(concrete: &syn::Type) -> String {
    format!(
        "{INTERFACE_ASSERTION_CANDIDATE_DOC_PREFIX}{}",
        quote::quote! { #concrete }
    )
}

pub fn interface_assertion_candidate_from_doc(doc: &str) -> Option<syn::Type> {
    let concrete = doc
        .strip_prefix(INTERFACE_ASSERTION_CANDIDATE_DOC_PREFIX)
        .filter(|concrete| !concrete.is_empty())?;
    syn::parse_str(concrete).ok()
}

pub fn interface_assertion_candidate_from_attr(attr: &syn::Attribute) -> Option<syn::Type> {
    doc_attr_value(attr).and_then(|doc| interface_assertion_candidate_from_doc(&doc))
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
    fn interface_impl_dependency_doc_round_trips_interface_name() {
        let doc = interface_impl_required_by_doc("ReadCloser");

        assert_eq!(
            interface_impl_required_by_from_doc(&doc),
            Some("ReadCloser")
        );
    }

    #[test]
    fn interface_assertion_candidate_doc_round_trips_concrete_type() {
        let concrete: syn::Type =
            syn::parse_quote! { crate::builtin::GorsPtr<crate::bufio::Reader> };
        let doc = interface_assertion_candidate_doc(&concrete);
        let parsed = interface_assertion_candidate_from_doc(&doc).unwrap();

        assert_eq!(
            quote::quote! { #parsed }.to_string(),
            quote::quote! { #concrete }.to_string()
        );
    }

    #[test]
    fn fmt_flush_marker_attrs_read_doc_attributes() {
        let method: syn::Attribute = syn::parse_quote! {
            #[doc = "gors:fmt-flush-method=emit"]
        };
        let source: syn::Attribute = syn::parse_quote! {
            #[doc = "gors:fmt-flush-source=scratch"]
        };
        let dependency: syn::Attribute = syn::parse_quote! {
            #[doc = "gors:interface-impl-required-by=ReadCloser"]
        };
        let candidate: syn::Attribute = syn::parse_quote! {
            #[doc = "gors:interface-assertion-candidate=crate::model::Reader"]
        };

        assert_eq!(fmt_flush_method_from_attr(&method).as_deref(), Some("emit"));
        assert_eq!(
            fmt_flush_source_from_attr(&source).as_deref(),
            Some("scratch")
        );
        assert_eq!(
            interface_impl_required_by_from_attr(&dependency).as_deref(),
            Some("ReadCloser")
        );
        let candidate = interface_assertion_candidate_from_attr(&candidate).unwrap();
        assert_eq!(
            quote::quote! { #candidate }.to_string(),
            "crate :: model :: Reader"
        );
    }

    #[test]
    fn fmt_flush_docs_reject_empty_or_unrelated_docs() {
        assert_eq!(fmt_flush_method_from_doc(FMT_FLUSH_METHOD_DOC_PREFIX), None);
        assert_eq!(fmt_flush_method_from_doc("gors:other"), None);
        assert_eq!(fmt_flush_source_from_doc(FMT_FLUSH_SOURCE_DOC_PREFIX), None);
        assert_eq!(fmt_flush_source_from_doc("gors:other"), None);
        assert_eq!(
            interface_impl_required_by_from_doc(INTERFACE_IMPL_REQUIRED_BY_DOC_PREFIX),
            None
        );
        assert_eq!(interface_impl_required_by_from_doc("gors:other"), None);
        assert!(
            interface_assertion_candidate_from_doc(INTERFACE_ASSERTION_CANDIDATE_DOC_PREFIX)
                .is_none()
        );
        assert!(interface_assertion_candidate_from_doc("gors:other").is_none());
    }

    #[test]
    fn doc_attr_value_rejects_non_doc_attributes() {
        let attr: syn::Attribute = syn::parse_quote! {
            #[allow(dead_code)]
        };

        assert!(doc_attr_value(&attr).is_none());
    }
}
