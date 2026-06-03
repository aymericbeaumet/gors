use super::{CompiledModule, item_name, type_mentions_name};
use std::collections::{BTreeMap, HashSet};

pub(super) fn inject_post_prune_helpers(modules: &mut BTreeMap<String, CompiledModule>) {
    for module in modules.values_mut().filter(|module| module.is_stdlib) {
        let changed = match module.mod_name.as_str() {
            "reflect" => replace_reflect_value_module(module),
            "os" => inject_os_stdout(module),
            "sync" => replace_sync_pool_module(module),
            _ => false,
        };
        if changed {
            module.content_hash.clear();
        }
    }
}

fn replace_reflect_value_module(module: &mut CompiledModule) -> bool {
    if !module_has_struct(module, "Value") {
        return false;
    }
    module.file.items = vec![syn::parse_quote! {
        #[derive(Clone, Default)]
        pub struct Value;
    }];
    true
}

fn inject_os_stdout(module: &mut CompiledModule) -> bool {
    if !module_has_static(module, "Stdout") {
        return false;
    }

    let file_names = HashSet::from(["File".to_string()]);
    module.file.items.retain(|item| match item {
        syn::Item::Impl(item_impl) => !type_mentions_name(&item_impl.self_ty, &file_names),
        _ => item_name(item)
            .as_deref()
            .is_none_or(|name| !matches!(name, "File" | "Stdout")),
    });
    module.file.items.extend([
        syn::parse_quote! {
            #[derive(Clone, Copy, Default)]
            pub struct File;
        },
        syn::parse_quote! {
            #[allow(non_upper_case_globals)]
            pub static Stdout: std::sync::LazyLock<File> =
                std::sync::LazyLock::new(|| File);
        },
        syn::parse_quote! {
            impl crate::io::Writer for File {
                fn __gors_as_any(&self) -> Option<&dyn std::any::Any> {
                    Some(self)
                }

                fn __gors_clone_box(&self) -> Box<dyn crate::io::Writer> {
                    Box::new(*self) as Box<dyn crate::io::Writer>
                }

                fn Write(&mut self, b: Vec<u8>) -> (isize, Box<dyn crate::builtin::error>) {
                    let mut stdout = std::io::stdout();
                    match std::io::Write::write_all(&mut stdout, &b) {
                        Ok(()) => (
                            b.len() as isize,
                            Box::new(crate::builtin::__GorsNooperror::default())
                                as Box<dyn crate::builtin::error>,
                        ),
                        Err(err) => (
                            0,
                            Box::new(crate::builtin::__GorsStringError(err.to_string()))
                                as Box<dyn crate::builtin::error>,
                        ),
                    }
                }
            }
        },
    ]);
    true
}

fn replace_sync_pool_module(module: &mut CompiledModule) -> bool {
    if !module_has_struct(module, "Pool") {
        return false;
    }

    module.file.items = vec![
        syn::parse_quote! {
            pub struct Pool {
                pub New: std::sync::Arc<
                    std::sync::Mutex<
                        Option<
                            std::sync::Arc<
                                dyn Fn() -> Box<dyn std::any::Any> + Send + Sync
                            >
                        >
                    >
                >,
                pub noCopy: (),
                pub local: usize,
                pub localSize: usize,
                pub victim: usize,
                pub victimSize: usize,
            }
        },
        syn::parse_quote! {
            impl Default for Pool {
                fn default() -> Self {
                    Self {
                        New: std::sync::Arc::new(std::sync::Mutex::new(None)),
                        noCopy: Default::default(),
                        local: Default::default(),
                        localSize: Default::default(),
                        victim: Default::default(),
                        victimSize: Default::default(),
                    }
                }
            }
        },
        syn::parse_quote! {
            impl Pool {
                pub fn Get(&self) -> Box<dyn std::any::Any> {
                    let new_func = self
                        .New
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .clone();
                    match new_func {
                        Some(new_func) => new_func(),
                        None => Box::new(()) as Box<dyn std::any::Any>,
                    }
                }

                pub fn Put(&self, _x: Box<dyn std::any::Any>) {}
            }
        },
    ];
    true
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

        assert!(inject_os_stdout(&mut module));
        let source = prettyplease::unparse(&module.file);

        assert!(source.contains("pub const PathSeparator"), "{source}");
        assert!(source.contains("pub struct File"), "{source}");
        assert!(source.contains("pub static Stdout"), "{source}");
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
            },
        );

        assert!(replace_sync_pool_module(&mut module));
        let source = prettyplease::unparse(&module.file);

        assert!(source.contains("pub struct Pool"), "{source}");
        assert!(source.contains("pub fn Get"), "{source}");
        assert!(!source.contains("pub struct Mutex"), "{source}");
    }
}
