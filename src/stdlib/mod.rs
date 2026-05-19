mod errors;
mod fmt;
mod io;
mod os;
mod strconv;
mod sync;

pub fn is_known(import_path: &str) -> bool {
    resolve_stdlib(import_path).is_some()
}

pub fn resolve_stdlib(import_path: &str) -> Option<syn::ItemMod> {
    let (mod_name, items) = match import_path {
        "errors" => ("errors", errors::module_items()),
        "fmt" => ("fmt", fmt::module_items()),
        "io" => ("io", io::module_items()),
        "os" => ("os", os::module_items()),
        "strconv" => ("strconv", strconv::module_items()),
        "sync" => ("sync", sync::module_items()),
        _ => return None,
    };

    Some(syn::ItemMod {
        attrs: vec![],
        vis: syn::Visibility::Inherited,
        unsafety: None,
        mod_token: <syn::Token![mod]>::default(),
        ident: syn::Ident::new(mod_name, proc_macro2::Span::mixed_site()),
        content: Some((syn::token::Brace::default(), items)),
        semi: None,
    })
}
