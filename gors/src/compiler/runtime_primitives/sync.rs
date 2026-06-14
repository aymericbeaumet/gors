use super::{CompiledModule, module_has_struct, prune_replaced_items};
use std::collections::HashSet;

pub(super) const MODULE: &str = "sync";
const MAP_TYPE: &str = "Map";
const POOL_TYPE: &str = "Pool";

pub(super) fn replace_module(module: &mut CompiledModule) -> bool {
    replace_map_module(module) | replace_pool_module(module)
}

fn replace_pool_module(module: &mut CompiledModule) -> bool {
    if !module_has_struct(module, POOL_TYPE) {
        return false;
    }

    let pool_names = HashSet::from([POOL_TYPE.to_string()]);
    prune_replaced_items(module, &pool_names, &pool_names);

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
                pub fn Get(mut p: crate::builtin::GorsPtr<Self>) -> Box<dyn std::any::Any> {
                    let new_func = p
                        .lock()
                        .unwrap()
                        .New
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .clone();
                    match new_func {
                        Some(new_func) => new_func(),
                        None => Box::new(()) as Box<dyn std::any::Any>,
                    }
                }

                pub fn Put(mut p: crate::builtin::GorsPtr<Self>, _x: Box<dyn std::any::Any>) {
                    let _ = p;
                }
            }
        },
    ]);
    true
}

fn replace_map_module(module: &mut CompiledModule) -> bool {
    if !module_has_struct(module, MAP_TYPE) {
        return false;
    }

    let map_names = HashSet::from([MAP_TYPE.to_string()]);
    prune_replaced_items(module, &map_names, &map_names);

    module.file.items.extend([
        syn::parse_quote! {
            type __GorsSyncMapEntry = (
                Box<dyn std::any::Any + Send + Sync>,
                Box<dyn std::any::Any + Send + Sync>,
            );
        },
        syn::parse_quote! {
            pub struct Map {
                _gors_blank_0: noCopy,
                entries: std::sync::Arc<std::sync::Mutex<Vec<__GorsSyncMapEntry>>>,
            }
        },
        syn::parse_quote! {
            impl Default for Map {
                fn default() -> Self {
                    Self {
                        _gors_blank_0: Default::default(),
                        entries: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
                    }
                }
            }
        },
        syn::parse_quote! {
            #[allow(unsafe_code)]
            #[allow(dead_code)]
            unsafe impl Send for Map {}
        },
        syn::parse_quote! {
            #[allow(unsafe_code)]
            #[allow(dead_code)]
            unsafe impl Sync for Map {}
        },
        syn::parse_quote! {
            impl Map {
                fn __gors_entries(
                    m: &crate::builtin::GorsPtr<Self>,
                ) -> std::sync::Arc<std::sync::Mutex<Vec<__GorsSyncMapEntry>>> {
                    m.lock().unwrap().entries.clone()
                }

                fn __gors_assert_comparable_key(key: &dyn std::any::Any) {
                    if !crate::builtin::interface_is_nil(key)
                        && !crate::builtin::reflect_type_comparable(key)
                    {
                        crate::builtin::panic_value("hash of unhashable type");
                    }
                }

                fn __gors_find_key(entries: &[__GorsSyncMapEntry], key: &dyn std::any::Any) -> Option<usize> {
                    entries
                        .iter()
                        .position(|(entry_key, _)| crate::builtin::any_eq(entry_key.as_ref(), key))
                }

                fn __gors_nil_any() -> Box<dyn std::any::Any> {
                    Box::new(()) as Box<dyn std::any::Any>
                }

                pub fn Load(
                    mut m: crate::builtin::GorsPtr<Self>,
                    mut key: Box<dyn std::any::Any>,
                ) -> (Box<dyn std::any::Any>, bool) {
                    Self::__gors_assert_comparable_key(key.as_ref());
                    let entries = Self::__gors_entries(&m);
                    let entries = entries.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    if let Some(index) = Self::__gors_find_key(&entries, key.as_ref()) {
                        return (crate::builtin::clone_any(entries[index].1.as_ref()), true);
                    }
                    (Self::__gors_nil_any(), false)
                }

                pub fn Store(
                    mut m: crate::builtin::GorsPtr<Self>,
                    mut key: Box<dyn std::any::Any>,
                    mut value: Box<dyn std::any::Any>,
                ) {
                    let _ = Self::Swap(m, key, value);
                }

                pub fn Clear(mut m: crate::builtin::GorsPtr<Self>) {
                    let entries = Self::__gors_entries(&m);
                    entries
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .clear();
                }

                pub fn LoadOrStore(
                    mut m: crate::builtin::GorsPtr<Self>,
                    mut key: Box<dyn std::any::Any>,
                    mut value: Box<dyn std::any::Any>,
                ) -> (Box<dyn std::any::Any>, bool) {
                    Self::__gors_assert_comparable_key(key.as_ref());
                    let entries = Self::__gors_entries(&m);
                    let mut entries = entries.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    if let Some(index) = Self::__gors_find_key(&entries, key.as_ref()) {
                        return (crate::builtin::clone_any(entries[index].1.as_ref()), true);
                    }
                    let actual = crate::builtin::clone_any(value.as_ref());
                    entries.push((
                        crate::builtin::clone_any_send_sync(key.as_ref()),
                        crate::builtin::clone_any_send_sync(value.as_ref()),
                    ));
                    (actual, false)
                }

                pub fn LoadAndDelete(
                    mut m: crate::builtin::GorsPtr<Self>,
                    mut key: Box<dyn std::any::Any>,
                ) -> (Box<dyn std::any::Any>, bool) {
                    Self::__gors_assert_comparable_key(key.as_ref());
                    let entries = Self::__gors_entries(&m);
                    let mut entries = entries.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    if let Some(index) = Self::__gors_find_key(&entries, key.as_ref()) {
                        let (_, value) = entries.remove(index);
                        return (crate::builtin::clone_any(value.as_ref()), true);
                    }
                    (Self::__gors_nil_any(), false)
                }

                pub fn Delete(mut m: crate::builtin::GorsPtr<Self>, mut key: Box<dyn std::any::Any>) {
                    let _ = Self::LoadAndDelete(m, key);
                }

                pub fn Swap(
                    mut m: crate::builtin::GorsPtr<Self>,
                    mut key: Box<dyn std::any::Any>,
                    mut value: Box<dyn std::any::Any>,
                ) -> (Box<dyn std::any::Any>, bool) {
                    Self::__gors_assert_comparable_key(key.as_ref());
                    let entries = Self::__gors_entries(&m);
                    let mut entries = entries.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    let stored_value = crate::builtin::clone_any_send_sync(value.as_ref());
                    if let Some(index) = Self::__gors_find_key(&entries, key.as_ref()) {
                        let previous = std::mem::replace(&mut entries[index].1, stored_value);
                        return (crate::builtin::clone_any(previous.as_ref()), true);
                    }
                    entries.push((
                        crate::builtin::clone_any_send_sync(key.as_ref()),
                        stored_value,
                    ));
                    (Self::__gors_nil_any(), false)
                }

                pub fn CompareAndSwap(
                    mut m: crate::builtin::GorsPtr<Self>,
                    mut key: Box<dyn std::any::Any>,
                    mut old: Box<dyn std::any::Any>,
                    mut new: Box<dyn std::any::Any>,
                ) -> bool {
                    Self::__gors_assert_comparable_key(key.as_ref());
                    if !crate::builtin::interface_is_nil(old.as_ref())
                        && !crate::builtin::reflect_type_comparable(old.as_ref())
                    {
                        crate::builtin::panic_value("comparing uncomparable type");
                    }
                    let entries = Self::__gors_entries(&m);
                    let mut entries = entries.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    if let Some(index) = Self::__gors_find_key(&entries, key.as_ref()) {
                        if crate::builtin::any_eq(entries[index].1.as_ref(), old.as_ref()) {
                            entries[index].1 = crate::builtin::clone_any_send_sync(new.as_ref());
                            return true;
                        }
                    }
                    false
                }

                pub fn CompareAndDelete(
                    mut m: crate::builtin::GorsPtr<Self>,
                    mut key: Box<dyn std::any::Any>,
                    mut old: Box<dyn std::any::Any>,
                ) -> bool {
                    Self::__gors_assert_comparable_key(key.as_ref());
                    if !crate::builtin::interface_is_nil(old.as_ref())
                        && !crate::builtin::reflect_type_comparable(old.as_ref())
                    {
                        crate::builtin::panic_value("comparing uncomparable type");
                    }
                    let entries = Self::__gors_entries(&m);
                    let mut entries = entries.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    if let Some(index) = Self::__gors_find_key(&entries, key.as_ref()) {
                        if crate::builtin::any_eq(entries[index].1.as_ref(), old.as_ref()) {
                            entries.remove(index);
                            return true;
                        }
                    }
                    false
                }

                pub fn Range(
                    mut m: crate::builtin::GorsPtr<Self>,
                    mut f: std::sync::Arc<
                        std::sync::Mutex<
                            Option<
                                std::sync::Arc<
                                    dyn Fn(Box<dyn std::any::Any>, Box<dyn std::any::Any>) -> bool
                                        + Send
                                        + Sync,
                                >,
                            >,
                        >,
                    >,
                ) {
                    let snapshot: Vec<(Box<dyn std::any::Any>, Box<dyn std::any::Any>)> = {
                        let entries = Self::__gors_entries(&m);
                        let entries = entries.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                        entries
                            .iter()
                            .map(|(key, value)| {
                                (
                                    crate::builtin::clone_any(key.as_ref()),
                                    crate::builtin::clone_any(value.as_ref()),
                                )
                            })
                            .collect()
                    };
                    let __gors_func = {
                        let __gors_func = crate::builtin::lock_func(&f);
                        match __gors_func.as_ref() {
                            Some(__gors_func) => __gors_func.clone(),
                            None => crate::builtin::panic_value("nil function"),
                        }
                    };
                    for (key, value) in snapshot {
                        if !(&*__gors_func)(key, value) {
                            break;
                        }
                    }
                }
            }
        },
    ]);
    true
}
