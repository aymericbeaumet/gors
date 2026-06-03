use std::collections::HashSet;

mod reflectlite;
mod runtime;

pub(super) fn module(import_path: &str, roots: Option<&HashSet<String>>) -> Option<syn::ItemMod> {
    match import_path {
        "internal/reflectlite" => reflectlite::module(import_path, roots),
        "runtime" => runtime::module(import_path, roots),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::ToTokens;

    fn tokens_for(import_path: &str, roots: &[&str]) -> Option<String> {
        let roots = roots.iter().map(|root| (*root).to_string()).collect();
        module(import_path, Some(&roots)).map(|module| module.to_token_stream().to_string())
    }

    #[test]
    fn runtime_module_emits_only_requested_roots() {
        let tokens = tokens_for("runtime", &["GOMAXPROCS", "GOOS"]).expect("runtime module");

        assert!(tokens.contains("pub fn GOMAXPROCS"), "{tokens}");
        assert!(tokens.contains("pub fn GOOS"), "{tokens}");
        assert!(!tokens.contains("pub fn GOARCH"), "{tokens}");
    }

    #[test]
    fn reflectlite_value_roots_emit_value_contract_without_swapper() {
        let tokens =
            tokens_for("internal/reflectlite", &["ValueOf", "Value::Len"]).expect("reflectlite");

        assert!(tokens.contains("pub struct Value"), "{tokens}");
        assert!(tokens.contains("pub fn Len"), "{tokens}");
        assert!(tokens.contains("pub fn Kind"), "{tokens}");
        assert!(tokens.contains("pub fn ValueOf"), "{tokens}");
        assert!(tokens.contains("pub type Kind"), "{tokens}");
        assert!(!tokens.contains("pub fn Swapper"), "{tokens}");
    }

    #[test]
    fn reflectlite_kind_root_does_not_emit_value_contract() {
        let tokens = tokens_for("internal/reflectlite", &["Slice"]).expect("reflectlite");

        assert!(tokens.contains("pub type Kind"), "{tokens}");
        assert!(tokens.contains("pub const Slice"), "{tokens}");
        assert!(!tokens.contains("pub struct Value"), "{tokens}");
        assert!(!tokens.contains("pub fn ValueOf"), "{tokens}");
        assert!(!tokens.contains("pub fn Swapper"), "{tokens}");
    }

    #[test]
    fn unknown_or_unrooted_runtime_primitives_do_not_emit_modules() {
        let empty_roots = HashSet::new();

        assert!(module("runtime", None).is_none());
        assert!(module("runtime", Some(&empty_roots)).is_none());
        assert!(module("fmt", Some(&HashSet::from(["Println".to_string()]))).is_none());
    }
}
