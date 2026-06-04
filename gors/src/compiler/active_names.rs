use proc_macro2::Span;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};

thread_local! {
    static ACTIVE_ITEM_NAMES: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    static ACTIVE_LOCAL_NAMES: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    static ACTIVE_LOCAL_RENAMES: RefCell<BTreeMap<String, String>> = const { RefCell::new(BTreeMap::new()) };
}

pub(super) struct ActiveLocalNamesGuard {
    previous: HashSet<String>,
    previous_renames: BTreeMap<String, String>,
}

pub(super) struct ActiveItemNamesGuard {
    previous: HashSet<String>,
}

impl ActiveLocalNamesGuard {
    pub(super) fn set(current: HashSet<String>) -> Self {
        let previous =
            ACTIVE_LOCAL_NAMES.with(|names| std::mem::replace(&mut *names.borrow_mut(), current));
        let current_renames = local_renames_for_names(
            &ACTIVE_LOCAL_NAMES.with(|names| names.borrow().iter().cloned().collect::<Vec<_>>()),
        );
        let previous_renames = ACTIVE_LOCAL_RENAMES
            .with(|renames| std::mem::replace(&mut *renames.borrow_mut(), current_renames));
        Self {
            previous,
            previous_renames,
        }
    }

    pub(super) fn push_scope() -> Self {
        let current = ACTIVE_LOCAL_NAMES.with(|names| names.borrow().clone());
        Self::set(current)
    }
}

impl Drop for ActiveLocalNamesGuard {
    fn drop(&mut self) {
        ACTIVE_LOCAL_NAMES.with(|names| {
            *names.borrow_mut() = std::mem::take(&mut self.previous);
        });
        ACTIVE_LOCAL_RENAMES.with(|renames| {
            *renames.borrow_mut() = std::mem::take(&mut self.previous_renames);
        });
    }
}

impl ActiveItemNamesGuard {
    pub(super) fn set(current: HashSet<String>) -> Self {
        let previous =
            ACTIVE_ITEM_NAMES.with(|names| std::mem::replace(&mut *names.borrow_mut(), current));
        Self { previous }
    }
}

impl Drop for ActiveItemNamesGuard {
    fn drop(&mut self) {
        ACTIVE_ITEM_NAMES.with(|names| {
            *names.borrow_mut() = std::mem::take(&mut self.previous);
        });
    }
}

pub(super) fn clear() {
    ACTIVE_ITEM_NAMES.with(|names| names.borrow_mut().clear());
    ACTIVE_LOCAL_NAMES.with(|names| names.borrow_mut().clear());
    ACTIVE_LOCAL_RENAMES.with(|renames| renames.borrow_mut().clear());
}

pub(super) fn add_active_local_names(names: impl IntoIterator<Item = String>) {
    ACTIVE_LOCAL_NAMES.with(|active| {
        let names = names
            .into_iter()
            .filter(|name| !name.is_empty() && name != "_")
            .collect::<Vec<_>>();
        active.borrow_mut().extend(names.iter().cloned());
        ACTIVE_LOCAL_RENAMES.with(|renames| {
            let mut renames = renames.borrow_mut();
            for name in names {
                let renamed = local_binding_rust_name_from_safe_base(&name);
                if renamed == name {
                    renames.remove(&name);
                } else {
                    renames.insert(name, renamed);
                }
            }
        });
    });
}

pub(super) fn active_local_shadows_unqualified_name(name: &str) -> bool {
    !name.contains('.') && is_active_local_name(&super::rust_safe_ident_name(name))
}

pub(super) fn local_binding_ident(name: &str) -> syn::Ident {
    syn::Ident::new(&local_binding_rust_name(name), Span::mixed_site())
}

pub(super) fn value_ident(name: &str) -> syn::Ident {
    syn::Ident::new(&value_ident_rust_name(name), Span::mixed_site())
}

pub(super) fn value_ident_rust_name(name: &str) -> String {
    let base = super::rust_safe_ident_name(name);
    ACTIVE_LOCAL_RENAMES
        .with(|renames| renames.borrow().get(&base).cloned())
        .unwrap_or(base)
}

fn is_active_local_name(name: &str) -> bool {
    ACTIVE_LOCAL_NAMES.with(|names| names.borrow().contains(name))
}

fn local_renames_for_names(names: &[String]) -> BTreeMap<String, String> {
    names
        .iter()
        .filter_map(|name| {
            let renamed = local_binding_rust_name_from_safe_base(name);
            (renamed != *name).then(|| (name.clone(), renamed))
        })
        .collect()
}

pub(super) fn local_binding_rust_name(name: &str) -> String {
    local_binding_rust_name_from_safe_base(&super::rust_safe_ident_name(name))
}

fn local_binding_rust_name_from_safe_base(base: &str) -> String {
    ACTIVE_ITEM_NAMES.with(|names| {
        let names = names.borrow();
        if !names.contains(base) {
            return base.to_string();
        }
        let mut candidate = format!("{base}__local");
        let mut i = 0usize;
        while names.contains(&candidate) {
            i += 1;
            candidate = format!("{base}__local_{i}");
        }
        candidate
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_bindings_rename_when_they_collide_with_active_items() {
        let _items = ActiveItemNamesGuard::set(HashSet::from([
            "print".to_string(),
            "print__local".to_string(),
        ]));
        let _locals = ActiveLocalNamesGuard::set(HashSet::from(["print".to_string()]));

        assert_eq!(local_binding_ident("print").to_string(), "print__local_1");
        assert_eq!(value_ident("print").to_string(), "print__local_1");
        assert!(active_local_shadows_unqualified_name("print"));
    }

    #[test]
    fn adding_active_local_names_updates_value_renames() {
        let _items = ActiveItemNamesGuard::set(HashSet::from(["String".to_string()]));
        let _locals = ActiveLocalNamesGuard::set(HashSet::new());

        add_active_local_names(["String".to_string(), "_".to_string(), String::new()]);

        assert_eq!(value_ident("String").to_string(), "String__local");
        assert!(!active_local_shadows_unqualified_name("_"));
    }
}
