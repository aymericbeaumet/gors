use std::collections::HashSet;

pub(super) fn module(import_path: &str, roots: Option<&HashSet<String>>) -> Option<syn::ItemMod> {
    let roots = roots?;
    if roots.is_empty() {
        return None;
    }

    let mut items = Vec::new();
    if roots.contains("GOMAXPROCS") {
        items.push(syn::parse_quote! {
            pub fn GOMAXPROCS(mut n: isize) -> isize {
                let current = std::thread::available_parallelism()
                    .map(|parallelism| parallelism.get() as isize)
                    .unwrap_or(1)
                    .max(1);
                if n < 1 {
                    return current;
                }
                current
            }
        });
    }
    if roots.contains("GOARCH") {
        items.push(syn::parse_quote! {
            pub fn GOARCH() -> String {
                std::env::consts::ARCH.to_string()
            }
        });
    }
    if roots.contains("GOOS") {
        items.push(syn::parse_quote! {
            pub fn GOOS() -> String {
                std::env::consts::OS.to_string()
            }
        });
    }

    (!items.is_empty()).then(|| super::super::item_mod_for(import_path, items))
}
