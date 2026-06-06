use super::{CompiledModule, module_has_struct, prune_replaced_items};
use std::collections::HashSet;

pub(super) const MODULE: &str = "sync__atomic";
const INT32_TYPE: &str = "Int32";
const VALUE_TYPE: &str = "Value";

pub(super) fn replace_module(module: &mut CompiledModule) -> bool {
    if !module_has_struct(module, INT32_TYPE) && !module_has_struct(module, VALUE_TYPE) {
        return false;
    }

    let item_names = HashSet::from([
        "AddInt32".to_string(),
        "CompareAndSwapInt32".to_string(),
        INT32_TYPE.to_string(),
        VALUE_TYPE.to_string(),
    ]);
    let impl_self_type_names = HashSet::from([INT32_TYPE.to_string(), VALUE_TYPE.to_string()]);
    prune_replaced_items(module, &item_names, &impl_self_type_names);

    module.file.items.extend([
        syn::parse_quote! {
            pub fn AddInt32(mut addr: crate::builtin::GorsPtr<i32>, delta: i32) -> i32 {
                let mut value = addr.lock().unwrap();
                *value += delta;
                *value
            }
        },
        syn::parse_quote! {
            pub fn CompareAndSwapInt32(
                mut addr: crate::builtin::GorsPtr<i32>,
                old: i32,
                new: i32,
            ) -> bool {
                let mut value = addr.lock().unwrap();
                if *value == old {
                    *value = new;
                    true
                } else {
                    false
                }
            }
        },
        syn::parse_quote! {
            #[derive(Clone, Default, PartialEq)]
            pub struct Int32 {
                v: i32,
            }
        },
        syn::parse_quote! {
            impl Int32 {
                pub fn Add(mut x: crate::builtin::GorsPtr<Self>, delta: i32) -> i32 {
                    let mut value = x.lock().unwrap();
                    value.v += delta;
                    value.v
                }
            }
        },
        syn::parse_quote! {
            #[derive(Clone)]
            pub struct Value {
                v: std::sync::Arc<
                    std::sync::Mutex<Box<dyn std::any::Any + Send + Sync>>,
                >,
            }
        },
        syn::parse_quote! {
            impl Default for Value {
                fn default() -> Self {
                    Self {
                        v: std::sync::Arc::new(
                            std::sync::Mutex::new(
                                Box::new(()) as Box<dyn std::any::Any + Send + Sync>
                            )
                        ),
                    }
                }
            }
        },
        syn::parse_quote! {
            impl Value {
                pub fn Load(mut v: crate::builtin::GorsPtr<Self>) -> Box<dyn std::any::Any> {
                    let value = v.lock().unwrap();
                    let value = value
                        .v
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    crate::builtin::clone_any(value.as_ref())
                }

                pub fn Store(
                    mut v: crate::builtin::GorsPtr<Self>,
                    val: Box<dyn std::any::Any>,
                ) {
                    if crate::builtin::interface_is_nil(val.as_ref()) {
                        crate::builtin::panic_value(
                            "sync/atomic: store of nil value into Value".to_string(),
                        );
                    }
                    let value = v.lock().unwrap();
                    let mut slot = value
                        .v
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    *slot = crate::builtin::clone_any_send_sync(val.as_ref());
                }
            }
        },
    ]);
    true
}
