use super::CompiledModule;
use crate::compiler::syn_inspect::{item_name, type_mentions_name};
use std::collections::{BTreeMap, HashSet};

mod os;
mod reflect;
mod sync;
mod sync_atomic;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrimitiveCategory {
    CompilerIntrinsic,
    GoRuntimeAbi,
    HostResource,
    LanguagePrimitive,
}

const PRIMITIVE_CATEGORIES: &[PrimitiveCategory] = &[
    PrimitiveCategory::CompilerIntrinsic,
    PrimitiveCategory::GoRuntimeAbi,
    PrimitiveCategory::HostResource,
    PrimitiveCategory::LanguagePrimitive,
];

struct PostPrunePrimitive {
    module: &'static str,
    category: PrimitiveCategory,
    owned_symbols: &'static [&'static str],
    inject: fn(&mut CompiledModule) -> bool,
}

impl PostPrunePrimitive {
    fn inject(&self, module: &mut CompiledModule) -> bool {
        debug_assert_eq!(module.mod_name, self.module);
        debug_assert!(!self.owned_symbols.is_empty());
        debug_assert!(PRIMITIVE_CATEGORIES.contains(&self.category));
        (self.inject)(module)
    }
}

const POST_PRUNE_PRIMITIVES: &[PostPrunePrimitive] = &[
    PostPrunePrimitive {
        module: reflect::MODULE,
        category: PrimitiveCategory::CompilerIntrinsic,
        owned_symbols: &["Value", "MapIter", "ValueOf", "DeepEqual"],
        inject: reflect::replace_value_module,
    },
    PostPrunePrimitive {
        module: os::MODULE,
        category: PrimitiveCategory::HostResource,
        owned_symbols: &["File", "Stdout", "File::Write"],
        inject: os::inject_stdout,
    },
    PostPrunePrimitive {
        module: sync::MODULE,
        category: PrimitiveCategory::GoRuntimeAbi,
        owned_symbols: &["Map", "Pool", "Map::*", "Pool::*"],
        inject: sync::replace_module,
    },
    PostPrunePrimitive {
        module: sync_atomic::MODULE,
        category: PrimitiveCategory::GoRuntimeAbi,
        owned_symbols: &[
            "AddInt32",
            "CompareAndSwapInt32",
            "LoadUint32",
            "StoreUint32",
            "Int32",
            "Pointer",
            "Value",
        ],
        inject: sync_atomic::replace_module,
    },
];

pub(super) fn inject_post_prune_helpers(modules: &mut BTreeMap<String, CompiledModule>) {
    for module in modules.values_mut().filter(|module| module.is_stdlib) {
        let changed = POST_PRUNE_PRIMITIVES
            .iter()
            .find(|primitive| primitive.module == module.mod_name)
            .is_some_and(|primitive| primitive.inject(module));
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

fn module_has_item(module: &CompiledModule, name: &str) -> bool {
    module
        .file
        .items
        .iter()
        .any(|item| item_name(item).as_deref() == Some(name))
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
    fn post_prune_primitives_declare_boundaries() {
        let modules = POST_PRUNE_PRIMITIVES
            .iter()
            .map(|primitive| primitive.module)
            .collect::<HashSet<_>>();

        assert_eq!(
            modules,
            HashSet::from([
                reflect::MODULE,
                os::MODULE,
                sync::MODULE,
                sync_atomic::MODULE
            ])
        );
        for primitive in POST_PRUNE_PRIMITIVES {
            assert!(PRIMITIVE_CATEGORIES.contains(&primitive.category));
            assert!(
                !primitive.owned_symbols.is_empty(),
                "{} helper must declare owned symbols",
                primitive.module
            );
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
        assert!(
            source.contains("LazyLock<crate::builtin::GorsPtr<File>>"),
            "{source}"
        );
        let compact_source: String = source.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(
            compact_source.contains("pubfnWrite(")
                && compact_source.contains("mutfile:crate::builtin::GorsPtr<Self>")
                && compact_source.contains("b:Vec<u8>"),
            "{source}"
        );
        assert!(source.contains("#[allow(dead_code)]"), "{source}");
        assert!(
            source.contains("impl crate::io::Writer for File"),
            "{source}"
        );
        assert!(
            source.contains("impl crate::io::Writer for crate::builtin::GorsPtr<File>"),
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

        assert!(sync::replace_module(&mut module));
        let source = prettyplease::unparse(&module.file);

        assert!(source.contains("pub struct Pool"), "{source}");
        assert!(source.contains("pub fn Get"), "{source}");
        assert!(!source.contains("pub fn old"), "{source}");
        assert!(source.contains("#[allow(dead_code)]"), "{source}");
        assert!(source.contains("pub struct Mutex"), "{source}");
        assert!(source.contains("pub fn Lock"), "{source}");
    }

    #[test]
    fn sync_map_replacement_is_scoped_to_map_modules() {
        let mut module = stdlib_module(
            "sync",
            syn::parse_quote! {
                pub struct Map;
                pub struct Mutex;
                pub struct noCopy;
                impl Map {
                    pub fn old(&self) {}
                }
                impl Mutex {
                    pub fn Lock(&self) {}
                }
            },
        );

        assert!(sync::replace_module(&mut module));
        let source = prettyplease::unparse(&module.file);

        assert!(source.contains("pub struct Map"), "{source}");
        assert!(source.contains("pub fn Load"), "{source}");
        assert!(source.contains("pub fn Store"), "{source}");
        assert!(source.contains("pub fn Range"), "{source}");
        assert!(!source.contains("pub fn old"), "{source}");
        assert!(source.contains("#[allow(dead_code)]"), "{source}");
        assert!(source.contains("pub struct Mutex"), "{source}");
        assert!(source.contains("pub fn Lock"), "{source}");
        assert!(source.contains("pub struct noCopy"), "{source}");
    }

    #[test]
    fn sync_atomic_replacement_preserves_requested_runtime_contract() {
        let mut module = stdlib_module(
            "sync__atomic",
            syn::parse_quote! {
                pub fn AddInt32(addr: crate::builtin::GorsPtr<i32>, delta: i32) -> i32 { 0 }
                pub fn LoadUint32(addr: crate::builtin::GorsPtr<u32>) -> u32 { 0 }
                pub fn StoreUint32(addr: crate::builtin::GorsPtr<u32>, val: u32) {}
                pub struct Int32;
                pub struct noCopy;
                pub struct Pointer<T> {
                    _blank: noCopy,
                    v: usize,
                    _marker: std::marker::PhantomData<T>,
                }
                pub struct Value;
                impl<T> Pointer<T> {
                    pub fn old(&self) {}
                }
                impl Value {
                    pub fn old(&self) {}
                }
                pub fn Keep() -> i32 { 1 }
            },
        );

        assert!(sync_atomic::replace_module(&mut module));
        let source = prettyplease::unparse(&module.file);

        assert!(source.contains("pub fn AddInt32"), "{source}");
        assert!(source.contains("pub fn LoadUint32"), "{source}");
        assert!(source.contains("pub fn StoreUint32"), "{source}");
        assert!(source.contains("pub struct Int32"), "{source}");
        assert!(source.contains("pub struct Pointer"), "{source}");
        assert!(source.contains("GorsPtr<T>"), "{source}");
        assert!(source.contains("pub fn CompareAndSwap"), "{source}");
        assert!(source.contains("pub fn Swap"), "{source}");
        assert!(source.contains("pub struct Value"), "{source}");
        assert!(source.contains("pub fn Load"), "{source}");
        assert!(source.contains("pub fn Store"), "{source}");
        assert!(source.contains("pub fn Keep"), "{source}");
        assert!(!source.contains("pub fn old"), "{source}");
    }

    #[test]
    fn sync_atomic_replacement_triggers_on_function_only_roots() {
        let mut module = stdlib_module(
            "sync__atomic",
            syn::parse_quote! {
                pub fn LoadUint32(addr: crate::builtin::GorsPtr<u32>) -> u32 { 0 }
                pub fn StoreUint32(addr: crate::builtin::GorsPtr<u32>, val: u32) {}
                pub fn Keep() -> i32 { 1 }
            },
        );

        assert!(sync_atomic::replace_module(&mut module));
        let source = prettyplease::unparse(&module.file);

        assert!(source.contains("pub fn LoadUint32"), "{source}");
        assert!(source.contains("*value"), "{source}");
        assert!(source.contains("pub fn StoreUint32"), "{source}");
        assert!(source.contains("*value = val"), "{source}");
        assert!(source.contains("pub fn Keep"), "{source}");
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
        assert!(
            source.contains("#[derive(Clone, Default, PartialEq)]"),
            "{source}"
        );

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
                pub trait Type {
                    fn String(&mut self) -> String;
                }
                #[derive(Default)]
                pub struct __GorsNoopType;
                impl Type for __GorsNoopType {
                    fn String(&mut self) -> String {
                        String::new()
                    }
                }
                impl Value {
                    pub fn old(&self) {}
                }
                pub struct MapIter;
                impl MapIter {
                    pub fn Key(&mut self) -> Value {
                        copyVal()
                    }
                    pub fn Next(&mut self) -> bool {
                        true
                    }
                }
                pub fn copyVal() -> Value {
                    Value
                }
                pub fn ValueOf(value: Box<dyn std::any::Any>) -> Value {
                    let _ = value;
                    Value
                }
                fn deepValueEqual(v1: Value, v2: Value) -> bool {
                    let _ = (v1, v2);
                    true
                }
                pub fn DeepEqual(x: Box<dyn std::any::Any>, y: Box<dyn std::any::Any>) -> bool {
                    deepValueEqual(ValueOf(x), ValueOf(y))
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
        assert!(source.contains("pub trait Type"), "{source}");
        assert!(source.contains("pub struct __GorsNoopType"), "{source}");
        assert!(source.contains("pub struct MapIter"), "{source}");
        assert!(source.contains("pub fn Next"), "{source}");
        assert!(source.contains("pub fn Key"), "{source}");
        assert!(source.contains("pub fn MapRange"), "{source}");
        assert!(source.contains("pub fn KeepKind"), "{source}");
        assert!(source.contains("#[allow(dead_code)]"), "{source}");
        assert!(source.contains("pub struct Value"), "{source}");
        assert!(source.contains("pub fn IsValid"), "{source}");
        assert!(source.contains("pub fn Type"), "{source}");
        assert!(!source.contains("pub fn copyVal"), "{source}");
        assert!(source.contains("pub fn ValueOf"), "{source}");
        assert!(!source.contains("fn deepValueEqual"), "{source}");
        assert!(source.contains("pub fn DeepEqual"), "{source}");
        assert!(!source.contains("pub fn old"), "{source}");
    }
}
