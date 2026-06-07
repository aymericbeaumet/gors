use super::{CompiledModule, module_has_item, module_has_struct, prune_replaced_items};
use std::collections::HashSet;

pub(super) const MODULE: &str = "sync__atomic";
const INT32_TYPE: &str = "Int32";
const POINTER_TYPE: &str = "Pointer";
const VALUE_TYPE: &str = "Value";

pub(super) fn replace_module(module: &mut CompiledModule) -> bool {
    if !module_has_struct(module, INT32_TYPE)
        && !module_has_struct(module, POINTER_TYPE)
        && !module_has_struct(module, VALUE_TYPE)
        && !module_has_item(module, "AddInt32")
        && !module_has_item(module, "CompareAndSwapInt32")
        && !module_has_item(module, "LoadUint32")
        && !module_has_item(module, "StoreUint32")
    {
        return false;
    }

    let item_names = HashSet::from([
        "AddInt32".to_string(),
        "CompareAndSwapInt32".to_string(),
        "LoadUint32".to_string(),
        "StoreUint32".to_string(),
        INT32_TYPE.to_string(),
        POINTER_TYPE.to_string(),
        VALUE_TYPE.to_string(),
    ]);
    let impl_self_type_names = HashSet::from([
        INT32_TYPE.to_string(),
        POINTER_TYPE.to_string(),
        VALUE_TYPE.to_string(),
    ]);
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
            pub fn LoadUint32(mut addr: crate::builtin::GorsPtr<u32>) -> u32 {
                let value = addr.lock().unwrap();
                *value
            }
        },
        syn::parse_quote! {
            pub fn StoreUint32(mut addr: crate::builtin::GorsPtr<u32>, val: u32) {
                let mut value = addr.lock().unwrap();
                *value = val;
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
            pub struct Pointer<T> {
                _gors_blank_0: [crate::builtin::GorsPtr<T>; 0],
                _gors_blank_1: noCopy,
                v: std::sync::Arc<std::sync::Mutex<crate::builtin::GorsPtr<T>>>,
            }
        },
        syn::parse_quote! {
            impl<T> Clone for Pointer<T> {
                fn clone(&self) -> Self {
                    Self {
                        _gors_blank_0: self._gors_blank_0.clone(),
                        _gors_blank_1: self._gors_blank_1.clone(),
                        v: self.v.clone(),
                    }
                }
            }
        },
        syn::parse_quote! {
            impl<T> Default for Pointer<T> {
                fn default() -> Self {
                    Self {
                        _gors_blank_0: std::array::from_fn(|_| Default::default()),
                        _gors_blank_1: Default::default(),
                        v: std::sync::Arc::new(
                            std::sync::Mutex::new(crate::builtin::GorsPtr::nil()),
                        ),
                    }
                }
            }
        },
        syn::parse_quote! {
            impl<T> PartialEq for Pointer<T> {
                fn eq(&self, other: &Self) -> bool {
                    let left = self.v.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    let right = other.v.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    crate::builtin::GorsPtr::ptr_eq(&left, &right)
                }
            }
        },
        syn::parse_quote! {
            impl<T> Eq for Pointer<T> {}
        },
        syn::parse_quote! {
            impl<T> Pointer<T> {
                fn __gors_slot(
                    x: &crate::builtin::GorsPtr<Self>,
                ) -> std::sync::Arc<std::sync::Mutex<crate::builtin::GorsPtr<T>>> {
                    x.lock().unwrap().v.clone()
                }

                pub fn CompareAndSwap(
                    mut x: crate::builtin::GorsPtr<Self>,
                    mut old: crate::builtin::GorsPtr<T>,
                    mut new: crate::builtin::GorsPtr<T>,
                ) -> bool {
                    let slot = Self::__gors_slot(&x);
                    let mut slot = slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    if crate::builtin::GorsPtr::ptr_eq(&slot, &old) {
                        *slot = new.clone();
                        true
                    } else {
                        false
                    }
                }

                pub fn Load(mut x: crate::builtin::GorsPtr<Self>) -> crate::builtin::GorsPtr<T> {
                    let slot = Self::__gors_slot(&x);
                    slot.lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .clone()
                }

                pub fn Store(
                    mut x: crate::builtin::GorsPtr<Self>,
                    mut val: crate::builtin::GorsPtr<T>,
                ) {
                    let slot = Self::__gors_slot(&x);
                    *slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = val.clone();
                }

                pub fn Swap(
                    mut x: crate::builtin::GorsPtr<Self>,
                    mut new: crate::builtin::GorsPtr<T>,
                ) -> crate::builtin::GorsPtr<T> {
                    let slot = Self::__gors_slot(&x);
                    std::mem::replace(
                        &mut *slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()),
                        new.clone(),
                    )
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
