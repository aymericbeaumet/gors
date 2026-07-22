use super::{CompiledModule, module_has_item, module_has_struct, prune_replaced_items};
use proc_macro2::Span;
use std::collections::HashSet;

pub(super) const MODULE: &str = "sync__atomic";
const INT32_TYPE: &str = "Int32";
const POINTER_TYPE: &str = "Pointer";
const VALUE_TYPE: &str = "Value";

const SCALAR_ATOMIC_FUNCTIONS: &[&str] = &[
    "SwapInt32",
    "SwapInt64",
    "SwapUint32",
    "SwapUint64",
    "SwapUintptr",
    "CompareAndSwapInt32",
    "CompareAndSwapInt64",
    "CompareAndSwapUint32",
    "CompareAndSwapUint64",
    "CompareAndSwapUintptr",
    "AddInt32",
    "AddInt64",
    "AddUint32",
    "AddUint64",
    "AddUintptr",
    "AndInt32",
    "AndInt64",
    "AndUint32",
    "AndUint64",
    "AndUintptr",
    "OrInt32",
    "OrInt64",
    "OrUint32",
    "OrUint64",
    "OrUintptr",
    "LoadInt32",
    "LoadInt64",
    "LoadUint32",
    "LoadUint64",
    "LoadUintptr",
    "StoreInt32",
    "StoreInt64",
    "StoreUint32",
    "StoreUint64",
    "StoreUintptr",
];

pub(super) const OWNED_SYMBOLS: &[&str] = &[
    "SwapInt32",
    "SwapInt64",
    "SwapUint32",
    "SwapUint64",
    "SwapUintptr",
    "CompareAndSwapInt32",
    "CompareAndSwapInt64",
    "CompareAndSwapUint32",
    "CompareAndSwapUint64",
    "CompareAndSwapUintptr",
    "AddInt32",
    "AddInt64",
    "AddUint32",
    "AddUint64",
    "AddUintptr",
    "AndInt32",
    "AndInt64",
    "AndUint32",
    "AndUint64",
    "AndUintptr",
    "OrInt32",
    "OrInt64",
    "OrUint32",
    "OrUint64",
    "OrUintptr",
    "LoadInt32",
    "LoadInt64",
    "LoadUint32",
    "LoadUint64",
    "LoadUintptr",
    "StoreInt32",
    "StoreInt64",
    "StoreUint32",
    "StoreUint64",
    "StoreUintptr",
    "Int32",
    "Int32::*",
    "Pointer",
    "Pointer::*",
    "Value",
    "Value::*",
];

fn scalar_atomic_items(requested: &HashSet<String>) -> Vec<syn::Item> {
    let families: [(&str, syn::Type); 5] = [
        ("Int32", syn::parse_quote! { i32 }),
        ("Int64", syn::parse_quote! { i64 }),
        ("Uint32", syn::parse_quote! { u32 }),
        ("Uint64", syn::parse_quote! { u64 }),
        ("Uintptr", syn::parse_quote! { usize }),
    ];
    let mut items = Vec::with_capacity(SCALAR_ATOMIC_FUNCTIONS.len());
    for (suffix, ty) in families {
        let swap = syn::Ident::new(&format!("Swap{suffix}"), Span::mixed_site());
        let compare_and_swap =
            syn::Ident::new(&format!("CompareAndSwap{suffix}"), Span::mixed_site());
        let add = syn::Ident::new(&format!("Add{suffix}"), Span::mixed_site());
        let and = syn::Ident::new(&format!("And{suffix}"), Span::mixed_site());
        let or = syn::Ident::new(&format!("Or{suffix}"), Span::mixed_site());
        let load = syn::Ident::new(&format!("Load{suffix}"), Span::mixed_site());
        let store = syn::Ident::new(&format!("Store{suffix}"), Span::mixed_site());
        items.extend([
            syn::parse_quote! {
                pub fn #swap(mut addr: crate::builtin::GorsPtr<#ty>, new: #ty) -> #ty {
                    let mut value = addr.lock().unwrap();
                    std::mem::replace(&mut *value, new)
                }
            },
            syn::parse_quote! {
                pub fn #compare_and_swap(
                    mut addr: crate::builtin::GorsPtr<#ty>,
                    old: #ty,
                    new: #ty,
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
                pub fn #add(mut addr: crate::builtin::GorsPtr<#ty>, delta: #ty) -> #ty {
                    let mut value = addr.lock().unwrap();
                    *value = (*value).wrapping_add(delta);
                    *value
                }
            },
            syn::parse_quote! {
                pub fn #and(mut addr: crate::builtin::GorsPtr<#ty>, mask: #ty) -> #ty {
                    let mut value = addr.lock().unwrap();
                    let old = *value;
                    *value = old & mask;
                    old
                }
            },
            syn::parse_quote! {
                pub fn #or(mut addr: crate::builtin::GorsPtr<#ty>, mask: #ty) -> #ty {
                    let mut value = addr.lock().unwrap();
                    let old = *value;
                    *value = old | mask;
                    old
                }
            },
            syn::parse_quote! {
                pub fn #load(mut addr: crate::builtin::GorsPtr<#ty>) -> #ty {
                    let value = addr.lock().unwrap();
                    *value
                }
            },
            syn::parse_quote! {
                pub fn #store(mut addr: crate::builtin::GorsPtr<#ty>, val: #ty) {
                    let mut value = addr.lock().unwrap();
                    *value = val;
                }
            },
        ]);
    }
    items.retain(|item| {
        crate::compiler::syn_inspect::item_name(item).is_some_and(|name| requested.contains(&name))
    });
    items
}

pub(super) fn replace_module(module: &mut CompiledModule) -> bool {
    let requested_scalar_functions = SCALAR_ATOMIC_FUNCTIONS
        .iter()
        .filter(|name| module_has_item(module, name))
        .map(|name| (*name).to_string())
        .collect::<HashSet<_>>();
    if !module_has_struct(module, INT32_TYPE)
        && !module_has_struct(module, POINTER_TYPE)
        && !module_has_struct(module, VALUE_TYPE)
        && requested_scalar_functions.is_empty()
    {
        return false;
    }

    let item_names = OWNED_SYMBOLS
        .iter()
        .map(|name| (*name).to_string())
        .collect::<HashSet<_>>();
    let impl_self_type_names = HashSet::from([
        INT32_TYPE.to_string(),
        POINTER_TYPE.to_string(),
        VALUE_TYPE.to_string(),
    ]);
    prune_replaced_items(module, &item_names, &impl_self_type_names);

    module
        .file
        .items
        .extend(scalar_atomic_items(&requested_scalar_functions));
    module.file.items.extend([
        syn::parse_quote! {
            #[derive(Clone, Default, PartialEq)]
            pub struct Int32 {
                v: i32,
            }
        },
        syn::parse_quote! {
            impl Int32 {
                pub fn Load(mut x: crate::builtin::GorsPtr<Self>) -> i32 {
                    x.lock().unwrap().v
                }

                pub fn Store(mut x: crate::builtin::GorsPtr<Self>, val: i32) {
                    x.lock().unwrap().v = val;
                }

                pub fn Swap(mut x: crate::builtin::GorsPtr<Self>, new: i32) -> i32 {
                    let mut value = x.lock().unwrap();
                    std::mem::replace(&mut value.v, new)
                }

                pub fn CompareAndSwap(
                    mut x: crate::builtin::GorsPtr<Self>,
                    old: i32,
                    new: i32,
                ) -> bool {
                    let mut value = x.lock().unwrap();
                    if value.v == old {
                        value.v = new;
                        true
                    } else {
                        false
                    }
                }

                pub fn Add(mut x: crate::builtin::GorsPtr<Self>, delta: i32) -> i32 {
                    let mut value = x.lock().unwrap();
                    value.v = value.v.wrapping_add(delta);
                    value.v
                }

                pub fn And(mut x: crate::builtin::GorsPtr<Self>, mask: i32) -> i32 {
                    let mut value = x.lock().unwrap();
                    let old = value.v;
                    value.v = old & mask;
                    old
                }

                pub fn Or(mut x: crate::builtin::GorsPtr<Self>, mask: i32) -> i32 {
                    let mut value = x.lock().unwrap();
                    let old = value.v;
                    value.v = old | mask;
                    old
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

#[cfg(test)]
mod tests {
    use super::*;

    fn generated_names(items: &[syn::Item]) -> HashSet<String> {
        items
            .iter()
            .filter_map(crate::compiler::syn_inspect::item_name)
            .collect()
    }

    #[test]
    fn scalar_integer_generation_and_ownership_cover_the_same_abi() {
        let requested = SCALAR_ATOMIC_FUNCTIONS
            .iter()
            .map(|name| (*name).to_string())
            .collect::<HashSet<_>>();
        let items = scalar_atomic_items(&requested);
        let generated = generated_names(&items);
        let owned = OWNED_SYMBOLS
            .iter()
            .map(|name| (*name).to_string())
            .collect::<HashSet<_>>();

        assert_eq!(generated, requested);
        assert!(generated.is_subset(&owned));

        let source = prettyplease::unparse(&syn::File {
            attrs: Vec::new(),
            items,
            shebang: None,
        });
        assert!(source.contains("pub fn CompareAndSwapUint64"), "{source}");
        assert!(source.contains("pub fn LoadUint64"), "{source}");
        assert_eq!(source.matches("wrapping_add").count(), 5, "{source}");
    }

    #[test]
    fn scalar_integer_generation_is_reachability_driven() {
        let requested =
            HashSet::from(["CompareAndSwapUint64".to_string(), "LoadUint64".to_string()]);
        let generated = generated_names(&scalar_atomic_items(&requested));

        assert_eq!(generated, requested);
        assert!(!generated.contains("AddUint64"));
        assert!(!generated.contains("CompareAndSwapInt32"));
    }
}
