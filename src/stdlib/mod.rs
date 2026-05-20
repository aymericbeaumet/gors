pub mod embedded;
mod errors;
mod fmt;
mod io;
mod os;
mod strconv;
mod sync;

pub fn is_known(import_path: &str) -> bool {
    resolve_stdlib_handwritten(import_path).is_some() || embedded::package_exists(import_path)
}

fn resolve_stdlib_handwritten(import_path: &str) -> Option<(&'static str, Vec<syn::Item>)> {
    match import_path {
        "errors" => Some(("errors", errors::module_items())),
        "fmt" => Some(("fmt", fmt::module_items())),
        "io" => Some(("io", io::module_items())),
        "os" => Some(("os", os::module_items())),
        "strconv" => Some(("strconv", strconv::module_items())),
        "sync" => Some(("sync", sync::module_items())),
        _ => None,
    }
}

pub fn resolve_stdlib(import_path: &str) -> Option<syn::ItemMod> {
    if let Some((mod_name, items)) = resolve_stdlib_handwritten(import_path) {
        return Some(syn::ItemMod {
            attrs: vec![],
            vis: syn::Visibility::Inherited,
            unsafety: None,
            mod_token: <syn::Token![mod]>::default(),
            ident: syn::Ident::new(mod_name, proc_macro2::Span::mixed_site()),
            content: Some((syn::token::Brace::default(), items)),
            semi: None,
        });
    }

    let files = embedded::package_files(import_path)?;
    let mod_name = import_path.rsplit('/').next().unwrap_or(import_path);

    let mut all_items: Vec<syn::Item> = Vec::new();

    for (filename, content) in &files {
        let ast = match crate::parser::parse_file(filename, content) {
            Ok(ast) => ast,
            Err(_) => continue,
        };
        let compiled = match crate::compiler::compile(ast) {
            Ok(compiled) => compiled,
            Err(_) => continue,
        };
        all_items.extend(compiled.items);
    }

    if all_items.is_empty() {
        return None;
    }

    Some(syn::ItemMod {
        attrs: vec![],
        vis: syn::Visibility::Inherited,
        unsafety: None,
        mod_token: <syn::Token![mod]>::default(),
        ident: syn::Ident::new(mod_name, proc_macro2::Span::mixed_site()),
        content: Some((syn::token::Brace::default(), all_items)),
        semi: None,
    })
}
