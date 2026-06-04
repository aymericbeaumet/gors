use super::{CompiledModule, module_has_struct};
use crate::compiler::syn_inspect::{item_name, type_mentions_name};
use std::collections::HashSet;

pub(super) fn replace_pool_module(module: &mut CompiledModule) -> bool {
    if !module_has_struct(module, "Pool") {
        return false;
    }

    let pool_names = HashSet::from(["Pool".to_string()]);
    module.file.items.retain(|item| match item {
        syn::Item::Impl(item_impl) => !type_mentions_name(&item_impl.self_ty, &pool_names),
        _ => item_name(item).as_deref() != Some("Pool"),
    });
    for item in &mut module.file.items {
        crate::compiler::generated_attrs::allow_dead_code_on_item(item);
    }

    module.file.items.extend([
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
    ]);
    true
}
