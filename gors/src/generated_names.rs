pub const AS_ANY_METHOD: &str = "__gors_as_any";
pub const CLONE_BOX_METHOD: &str = "__gors_clone_box";
pub const ERROR_EXT_TRAIT: &str = "__GorsErrorExt";
pub const FMT_FLUSH_HOOK: &str = "__gors_flush_fmt";
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

pub fn noop_interface_ident() -> syn::Ident {
    ident(NOOP_INTERFACE)
}
