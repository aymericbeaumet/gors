use std::collections::HashSet;

mod reflectlite;
mod runtime;
mod syscall;

pub(super) fn module(import_path: &str, roots: Option<&HashSet<String>>) -> Option<syn::ItemMod> {
    match import_path {
        reflectlite::IMPORT_PATH => reflectlite::module(import_path, roots),
        runtime::IMPORT_PATH => runtime::module(import_path, roots),
        _ => None,
    }
}

pub(super) fn supplement_items(
    import_path: &str,
    roots: Option<&HashSet<String>>,
    items: &mut Vec<syn::Item>,
) {
    if import_path == syscall::IMPORT_PATH {
        syscall::supplement_items(roots, items);
    }
}

pub(super) fn supplement_type_env(
    import_path: &str,
    env: &mut crate::compiler::typeinfer::TypeEnv,
) {
    if import_path == syscall::IMPORT_PATH {
        syscall::supplement_type_env(env);
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

    fn required_tokens_for(import_path: &str, roots: &[&str]) -> String {
        let tokens = tokens_for(import_path, roots);
        assert!(
            tokens.is_some(),
            "expected runtime primitive module for {import_path}"
        );
        tokens.unwrap_or_default()
    }

    #[test]
    fn runtime_module_emits_only_requested_roots() {
        let tokens = required_tokens_for("runtime", &["GOMAXPROCS", "GOOS", "GOROOT", "stringer"]);

        assert!(tokens.contains("pub fn GOMAXPROCS"), "{tokens}");
        assert!(tokens.contains("pub fn GOOS"), "{tokens}");
        assert!(tokens.contains("pub fn GOROOT"), "{tokens}");
        assert!(tokens.contains("pub trait stringer"), "{tokens}");
        assert!(!tokens.contains("pub fn GOARCH"), "{tokens}");
    }

    #[test]
    fn reflectlite_value_roots_emit_value_contract_without_swapper() {
        let tokens = required_tokens_for("internal/reflectlite", &["ValueOf", "Value::Len"]);

        assert!(tokens.contains("pub struct Value"), "{tokens}");
        assert!(tokens.contains("pub fn Len"), "{tokens}");
        assert!(tokens.contains("pub fn Kind"), "{tokens}");
        assert!(tokens.contains("pub fn ValueOf"), "{tokens}");
        assert!(tokens.contains("pub type Kind"), "{tokens}");
        assert!(!tokens.contains("pub fn Swapper"), "{tokens}");
    }

    #[test]
    fn reflectlite_kind_root_does_not_emit_value_contract() {
        let tokens = required_tokens_for("internal/reflectlite", &["Slice"]);

        assert!(tokens.contains("pub type Kind"), "{tokens}");
        assert!(tokens.contains("pub const Slice"), "{tokens}");
        assert!(!tokens.contains("pub struct Value"), "{tokens}");
        assert!(!tokens.contains("pub fn ValueOf"), "{tokens}");
        assert!(!tokens.contains("pub fn Swapper"), "{tokens}");
    }

    #[test]
    fn reflectlite_type_roots_emit_typeof_comparable_contract() {
        let tokens = required_tokens_for(
            "internal/reflectlite",
            &["TypeOf", "Type::Comparable", "rtype"],
        );

        assert!(tokens.contains("pub struct Type"), "{tokens}");
        assert!(tokens.contains("pub type rtype"), "{tokens}");
        assert!(tokens.contains("pub fn TypeOf"), "{tokens}");
        assert!(tokens.contains("pub fn Comparable"), "{tokens}");
        assert!(tokens.contains("pub fn String"), "{tokens}");
        assert!(tokens.contains("reflect_type_comparable"), "{tokens}");
        assert!(!tokens.contains("pub struct Value"), "{tokens}");
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
