use crate::ast;
use proc_macro2::Span;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};

thread_local! {
    static IMPORT_NAMES: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    static IMPORT_PATHS_BY_LOCAL_NAME: RefCell<BTreeMap<String, String>> = const { RefCell::new(BTreeMap::new()) };
    static IMPORT_RENAMES: RefCell<BTreeMap<String, String>> = const { RefCell::new(BTreeMap::new()) };
    static DOT_IMPORT_RENAMES: RefCell<BTreeMap<String, String>> = const { RefCell::new(BTreeMap::new()) };
    static IMPORT_PACKAGE_NAMES: RefCell<BTreeMap<String, String>> = const { RefCell::new(BTreeMap::new()) };
}

pub(super) fn clear() {
    IMPORT_NAMES.with(|names| names.borrow_mut().clear());
    IMPORT_PATHS_BY_LOCAL_NAME.with(|paths| paths.borrow_mut().clear());
    IMPORT_RENAMES.with(|renames| renames.borrow_mut().clear());
    DOT_IMPORT_RENAMES.with(|renames| renames.borrow_mut().clear());
    IMPORT_PACKAGE_NAMES.with(|names| names.borrow_mut().clear());
}

pub(super) fn set_import_renames(import_renames: BTreeMap<String, String>) {
    IMPORT_RENAMES.with(|renames| {
        *renames.borrow_mut() = import_renames;
    });
}

pub(super) fn set_dot_import_renames(dot_import_renames: BTreeMap<String, String>) {
    DOT_IMPORT_RENAMES.with(|renames| {
        *renames.borrow_mut() = dot_import_renames;
    });
}

pub(super) fn set_import_package_names(import_package_names: BTreeMap<String, String>) {
    IMPORT_PACKAGE_NAMES.with(|names| {
        *names.borrow_mut() = import_package_names;
    });
}

fn import_local_name(import: &ast::ImportSpec<'_>) -> Option<String> {
    let path = import.path.value.trim_matches('"');
    if let Some(name) = &import.name {
        return (!matches!(name.name, "." | "_")).then(|| name.name.to_string());
    }
    IMPORT_PACKAGE_NAMES
        .with(|names| names.borrow().get(path).cloned())
        .or_else(|| crate::resolve::scan_type_env(path).map(|(package_name, _)| package_name))
        .or_else(|| path.rsplit('/').next().map(str::to_string))
}

pub(super) fn set_current_file_imports(file: &ast::File<'_>) {
    let imports = file
        .imports()
        .into_iter()
        .filter_map(|import| {
            let local_name = import_local_name(import)?;
            let import_path = import.path.value.trim_matches('"').to_string();
            Some((local_name, import_path))
        })
        .collect::<Vec<_>>();

    IMPORT_NAMES.with(|names| {
        let mut names = names.borrow_mut();
        names.clear();
        names.extend(imports.iter().map(|(local_name, _)| local_name.clone()));
    });
    IMPORT_PATHS_BY_LOCAL_NAME.with(|paths| {
        let mut paths = paths.borrow_mut();
        paths.clear();
        paths.extend(imports);
    });
}

pub(super) fn is_import_local_name(local_name: &str) -> bool {
    IMPORT_NAMES.with(|names| names.borrow().contains(local_name))
}

pub(super) fn import_local_name_matches_path(local_name: &str, import_path: &str) -> bool {
    IMPORT_PATHS_BY_LOCAL_NAME.with(|paths| {
        paths
            .borrow()
            .get(local_name)
            .is_some_and(|path| path == import_path)
    })
}

pub(super) fn file_import_package_names(file: &ast::File<'_>) -> BTreeMap<String, String> {
    file.imports()
        .into_iter()
        .filter(|import| import.name.is_none())
        .filter_map(|import| {
            let path = import.path.value.trim_matches('"');
            crate::resolve::scan_type_env(path)
                .map(|(package_name, _)| (path.to_string(), package_name))
        })
        .collect()
}

pub(super) fn import_rust_name(name: &str) -> String {
    let renamed = IMPORT_RENAMES.with(|renames| {
        renames
            .borrow()
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_string())
    });
    super::rust_safe_ident_name(&renamed)
}

pub(super) fn local_names_for_rust_module(module: &str) -> Vec<String> {
    IMPORT_RENAMES.with(|renames| {
        renames
            .borrow()
            .iter()
            .filter(|(_, rust_name)| *rust_name == module)
            .map(|(local_name, _)| local_name.clone())
            .collect()
    })
}

pub(super) fn dot_import_path_expr(name: &str) -> Option<syn::Expr> {
    DOT_IMPORT_RENAMES.with(|renames| {
        renames.borrow().get(name).map(|module| {
            let module = syn::Ident::new(&super::rust_safe_ident_name(module), Span::mixed_site());
            let name = syn::Ident::new(&super::rust_safe_ident_name(name), Span::mixed_site());
            syn::parse_quote! { #module::#name }
        })
    })
}

pub(super) fn selector_base_is_import(selector: &ast::SelectorExpr) -> bool {
    matches!(
        selector.x.as_ref(),
        ast::Expr::Ident(id) if is_import_local_name(id.name)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_file;
    use quote::quote;

    #[test]
    fn current_file_imports_record_default_alias_and_ignore_dot_blank() -> Result<(), String> {
        clear();
        set_import_package_names(BTreeMap::from([(
            "example/default".to_string(),
            "actual".to_string(),
        )]));
        let file = parse_file(
            "test.go",
            r#"
package main

import (
	alias "example/alias"
	. "example/dot"
	_ "example/blank"
	"example/default"
)
"#,
        )
        .map_err(|err| format!("fixture should parse: {err:?}"))?;

        set_current_file_imports(&file);

        assert!(is_import_local_name("alias"));
        assert!(is_import_local_name("actual"));
        assert!(import_local_name_matches_path("alias", "example/alias"));
        assert!(import_local_name_matches_path("actual", "example/default"));
        assert!(!is_import_local_name("."));
        assert!(!is_import_local_name("_"));
        assert!(!import_local_name_matches_path("dot", "example/dot"));
        clear();
        Ok(())
    }

    #[test]
    fn import_rust_name_applies_rewrites_and_rust_safety() {
        clear();
        set_import_renames(BTreeMap::from([(
            "ord".to_string(),
            "example__ordered".to_string(),
        )]));

        assert_eq!(import_rust_name("ord"), "example__ordered");
        assert_eq!(import_rust_name("type"), "type_");
        assert_eq!(
            local_names_for_rust_module("example__ordered"),
            vec!["ord".to_string()]
        );
        clear();
    }

    #[test]
    fn dot_import_path_expr_uses_recorded_module_rename() -> Result<(), String> {
        clear();
        set_dot_import_renames(BTreeMap::from([(
            "Answer".to_string(),
            "example__dot".to_string(),
        )]));

        let expr = dot_import_path_expr("Answer")
            .ok_or_else(|| "dot import should resolve".to_string())?;

        assert_eq!(quote! { #expr }.to_string(), "example__dot :: Answer");
        clear();
        Ok(())
    }
}
