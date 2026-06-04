use super::CompiledModule;
use crate::compiler::syn_inspect::{item_name, type_mentions_name};
use std::collections::{BTreeMap, HashSet};

mod os;
mod reflect;
mod sync;

pub(super) fn inject_post_prune_helpers(modules: &mut BTreeMap<String, CompiledModule>) {
    for module in modules.values_mut().filter(|module| module.is_stdlib) {
        let changed = match module.mod_name.as_str() {
            reflect::MODULE => reflect::replace_value_module(module),
            os::MODULE => os::inject_stdout(module),
            sync::MODULE => sync::replace_pool_module(module),
            _ => false,
        };
        if changed {
            module.content_hash.clear();
        }
    }
}

pub(super) fn inject_missing_preserved_modules(
    modules: &mut BTreeMap<String, CompiledModule>,
    preserved: &HashSet<String>,
) {
    reflect::inject_missing_value_module(modules, preserved);
}

fn module_has_struct(module: &CompiledModule, name: &str) -> bool {
    module
        .file
        .items
        .iter()
        .any(|item| matches!(item, syn::Item::Struct(item_struct) if item_struct.ident == name))
}

fn module_has_static(module: &CompiledModule, name: &str) -> bool {
    module
        .file
        .items
        .iter()
        .any(|item| matches!(item, syn::Item::Static(item_static) if item_static.ident == name))
}

fn prune_replaced_items(
    module: &mut CompiledModule,
    item_names: &HashSet<String>,
    impl_self_type_names: &HashSet<String>,
) {
    module.file.items.retain(|item| match item {
        syn::Item::Impl(item_impl) => !type_mentions_name(&item_impl.self_ty, impl_self_type_names),
        _ => item_name(item)
            .as_deref()
            .is_none_or(|name| !item_names.contains(name)),
    });
    for item in &mut module.file.items {
        crate::compiler::generated_attrs::allow_dead_code_on_item(item);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stdlib_module(mod_name: &str, file: syn::File) -> CompiledModule {
        CompiledModule {
            mod_name: mod_name.to_string(),
            import_path: mod_name.to_string(),
            file,
            filename: format!("{mod_name}.rs"),
            content_hash: "original".to_string(),
            is_main: false,
            is_stdlib: true,
        }
    }

    #[test]
    fn os_stdout_helper_preserves_unrelated_items() {
        let mut module = stdlib_module(
            "os",
            syn::parse_quote! {
                pub const PathSeparator: i32 = 47;
                pub struct File;
                pub static Stdout: File = File;
                impl File {
                    pub fn old(&self) {}
                }
            },
        );

        assert!(os::inject_stdout(&mut module));
        let source = prettyplease::unparse(&module.file);

        assert!(source.contains("pub const PathSeparator"), "{source}");
        assert!(source.contains("pub struct File"), "{source}");
        assert!(source.contains("pub static Stdout"), "{source}");
        assert!(source.contains("#[allow(dead_code)]"), "{source}");
        assert!(
            source.contains("impl crate::io::Writer for File"),
            "{source}"
        );
        assert!(!source.contains("pub fn old"), "{source}");
    }

    #[test]
    fn sync_pool_replacement_is_scoped_to_pool_modules() {
        let mut module = stdlib_module(
            "sync",
            syn::parse_quote! {
                pub struct Pool;
                pub struct Mutex;
                impl Pool {
                    pub fn old(&self) {}
                }
                impl Mutex {
                    pub fn Lock(&self) {}
                }
            },
        );

        assert!(sync::replace_pool_module(&mut module));
        let source = prettyplease::unparse(&module.file);

        assert!(source.contains("pub struct Pool"), "{source}");
        assert!(source.contains("pub fn Get"), "{source}");
        assert!(!source.contains("pub fn old"), "{source}");
        assert!(source.contains("#[allow(dead_code)]"), "{source}");
        assert!(source.contains("pub struct Mutex"), "{source}");
        assert!(source.contains("pub fn Lock"), "{source}");
    }

    #[test]
    fn reflect_value_module_injection_is_owned_by_runtime_primitives() {
        let mut modules = BTreeMap::new();
        let preserved = HashSet::from(["reflect".to_string()]);

        inject_missing_preserved_modules(&mut modules, &preserved);

        let module = modules.get("reflect");
        assert!(module.is_some(), "expected reflect module");
        let Some(module) = module else {
            return;
        };
        assert_eq!(module.mod_name, "reflect");
        assert!(module.is_stdlib);
        let source = prettyplease::unparse(&module.file);
        assert!(source.contains("pub struct Value"), "{source}");

        inject_missing_preserved_modules(&mut modules, &preserved);

        assert_eq!(modules.len(), 1);
    }

    #[test]
    fn reflect_value_replacement_preserves_unrelated_items() {
        let mut module = stdlib_module(
            "reflect",
            syn::parse_quote! {
                pub type Kind = isize;
                pub const Slice: Kind = 23;
                pub struct Value;
                pub struct Type;
                impl Value {
                    pub fn old(&self) {}
                }
                pub fn ValueOf(value: Box<dyn std::any::Any>) -> Value {
                    Value
                }
                pub fn KeepKind() -> Kind {
                    Slice
                }
            },
        );

        assert!(reflect::replace_value_module(&mut module));
        let source = prettyplease::unparse(&module.file);

        assert!(source.contains("pub type Kind"), "{source}");
        assert!(source.contains("pub const Slice"), "{source}");
        assert!(source.contains("pub struct Type"), "{source}");
        assert!(source.contains("pub fn KeepKind"), "{source}");
        assert!(source.contains("#[allow(dead_code)]"), "{source}");
        assert!(source.contains("pub struct Value"), "{source}");
        assert!(!source.contains("pub fn ValueOf"), "{source}");
        assert!(!source.contains("pub fn old"), "{source}");
    }
}
