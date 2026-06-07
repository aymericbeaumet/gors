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
    use crate::compiler::typeinfer::{GoType, TypeEnv, TypeKind};
    use quote::ToTokens;
    use quote::quote;

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

    fn supplemented_tokens_for(
        import_path: &str,
        roots: &[&str],
        mut items: Vec<syn::Item>,
    ) -> String {
        let roots = roots.iter().map(|root| (*root).to_string()).collect();
        supplement_items(import_path, Some(&roots), &mut items);
        quote! { #(#items)* }.to_string()
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
    fn runtime_module_emits_stack_roots() {
        let tokens = required_tokens_for(
            "runtime",
            &["Callers", "CallersFrames", "Frames", "Frames::Next"],
        );

        assert!(tokens.contains("pub fn Callers"), "{tokens}");
        assert!(tokens.contains("pub fn CallersFrames"), "{tokens}");
        assert!(tokens.contains("pub struct Frame"), "{tokens}");
        assert!(tokens.contains("pub struct Frames"), "{tokens}");
        assert!(tokens.contains("pub fn Next"), "{tokens}");
        assert!(tokens.contains("Frame :: default"), "{tokens}");
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
    fn syscall_supplements_write_boundary_and_socklen_alias() {
        let tokens = supplemented_tokens_for(
            "syscall",
            &["Sockaddr", "Write"],
            vec![syn::parse_quote! {
                pub trait Sockaddr {
                    fn sockaddr(
                        &mut self,
                    ) -> (usize, _Socklen, Box<dyn crate::builtin::error>);
                }
            }],
        );

        assert!(tokens.contains("pub type _Socklen = u32"), "{tokens}");
        assert!(tokens.contains("fn write"), "{tokens}");
        assert!(tokens.contains("p . len () as isize"), "{tokens}");
        assert!(!tokens.contains("fn read"), "{tokens}");
    }

    #[test]
    fn syscall_supplements_getuid_host_boundary() {
        let tokens = supplemented_tokens_for("syscall", &["Getuid"], Vec::new());

        assert!(tokens.contains("pub fn Getuid"), "{tokens}");
        assert!(tokens.contains("unsafe extern"), "{tokens}");
        assert!(tokens.contains("fn getuid"), "{tokens}");
        assert!(!tokens.contains("fn write"), "{tokens}");
    }

    #[test]
    fn syscall_supplements_write_and_socklen_type_facts() {
        let mut env = TypeEnv::new();
        supplement_type_env("syscall", &mut env);

        assert_eq!(
            env.get_type_kind("_Socklen").cloned(),
            Some(TypeKind::Alias(GoType::Uint32))
        );
        assert_eq!(
            env.get_func_returns("Write"),
            vec![GoType::Int, GoType::Error]
        );
        assert_eq!(
            env.get_func_params("Write"),
            vec![GoType::Int, GoType::Slice(Box::new(GoType::Uint8))]
        );
        assert_eq!(
            env.get_func_returns("write"),
            vec![GoType::Int, GoType::Error]
        );
        assert_eq!(
            env.get_func_params("write"),
            vec![GoType::Int, GoType::Slice(Box::new(GoType::Uint8))]
        );
        assert_eq!(env.get_func_returns("Getuid"), vec![GoType::Int]);
        assert_eq!(env.get_func_params("Getuid"), Vec::<GoType>::new());
    }

    #[test]
    fn unknown_or_unrooted_runtime_primitives_do_not_emit_modules() {
        let empty_roots = HashSet::new();

        assert!(module("runtime", None).is_none());
        assert!(module("runtime", Some(&empty_roots)).is_none());
        assert!(module("fmt", Some(&HashSet::from(["Println".to_string()]))).is_none());
    }
}
