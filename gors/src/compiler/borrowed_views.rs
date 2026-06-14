use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};

thread_local! {
    static BORROWED_POINTER_PARAM_NAMES: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    static BORROWED_POINTER_VIEW_NAMES: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    static BORROWED_POINTER_VIEW_METHODS: RefCell<BTreeMap<String, BorrowedPointerViewReturn>> = const { RefCell::new(BTreeMap::new()) };
    static MUTABLE_SLICE_VIEW_METHODS: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    static BORROWED_POINTER_VIEW_RETURN: RefCell<Option<BorrowedPointerViewReturn>> = const { RefCell::new(None) };
    static MUTABLE_SLICE_VIEW_RETURN: RefCell<Option<MutableSliceViewReturn>> = const { RefCell::new(None) };
    static BORROWED_SLICE_PARAM_NAMES: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    static SLICE_ALIAS_TARGETS: RefCell<BTreeMap<String, SliceAliasTarget>> = const { RefCell::new(BTreeMap::new()) };
}

#[derive(Clone)]
pub(super) struct BorrowedPointerViewReturn {
    pub(super) target_name: String,
    pub(super) target_ty: syn::Type,
}

#[derive(Clone)]
pub(super) struct MutableSliceViewReturn {
    pub(super) borrowed_return_elem_ty: Option<syn::Type>,
}

#[derive(Clone)]
pub(super) struct SliceAliasTarget {
    pub(super) base_name: String,
    pub(super) base_expr: syn::Expr,
    pub(super) offset: syn::Expr,
}

pub(super) struct BorrowedPointerParamNamesGuard {
    previous: HashSet<String>,
}

impl BorrowedPointerParamNamesGuard {
    pub(super) fn set(names: HashSet<String>) -> Self {
        let previous = BORROWED_POINTER_PARAM_NAMES.with(|borrowed| {
            let previous = borrowed.borrow().clone();
            *borrowed.borrow_mut() = names;
            previous
        });
        Self { previous }
    }
}

impl Drop for BorrowedPointerParamNamesGuard {
    fn drop(&mut self) {
        BORROWED_POINTER_PARAM_NAMES.with(|borrowed| {
            *borrowed.borrow_mut() = self.previous.clone();
        });
    }
}

pub(super) struct BorrowedPointerViewNamesGuard {
    previous: HashSet<String>,
}

impl BorrowedPointerViewNamesGuard {
    pub(super) fn clear() -> Self {
        let previous = BORROWED_POINTER_VIEW_NAMES
            .with(|borrowed| std::mem::take(&mut *borrowed.borrow_mut()));
        Self { previous }
    }
}

impl Drop for BorrowedPointerViewNamesGuard {
    fn drop(&mut self) {
        BORROWED_POINTER_VIEW_NAMES.with(|borrowed| {
            *borrowed.borrow_mut() = self.previous.clone();
        });
    }
}

pub(super) struct BorrowedPointerViewReturnGuard {
    previous: Option<BorrowedPointerViewReturn>,
}

impl BorrowedPointerViewReturnGuard {
    pub(super) fn set(current: Option<BorrowedPointerViewReturn>) -> Self {
        let previous = BORROWED_POINTER_VIEW_RETURN
            .with(|info| std::mem::replace(&mut *info.borrow_mut(), current));
        Self { previous }
    }
}

impl Drop for BorrowedPointerViewReturnGuard {
    fn drop(&mut self) {
        BORROWED_POINTER_VIEW_RETURN.with(|info| {
            *info.borrow_mut() = self.previous.clone();
        });
    }
}

pub(super) struct MutableSliceViewReturnGuard {
    previous: Option<MutableSliceViewReturn>,
}

impl MutableSliceViewReturnGuard {
    pub(super) fn set(current: Option<MutableSliceViewReturn>) -> Self {
        let previous = MUTABLE_SLICE_VIEW_RETURN
            .with(|info| std::mem::replace(&mut *info.borrow_mut(), current));
        Self { previous }
    }
}

impl Drop for MutableSliceViewReturnGuard {
    fn drop(&mut self) {
        MUTABLE_SLICE_VIEW_RETURN.with(|info| {
            *info.borrow_mut() = self.previous.clone();
        });
    }
}

pub(super) struct BorrowedSliceParamNamesGuard {
    previous: HashSet<String>,
}

impl BorrowedSliceParamNamesGuard {
    pub(super) fn set(names: HashSet<String>) -> Self {
        let previous = BORROWED_SLICE_PARAM_NAMES.with(|borrowed| {
            let previous = borrowed.borrow().clone();
            *borrowed.borrow_mut() = names;
            previous
        });
        Self { previous }
    }
}

impl Drop for BorrowedSliceParamNamesGuard {
    fn drop(&mut self) {
        BORROWED_SLICE_PARAM_NAMES.with(|borrowed| {
            *borrowed.borrow_mut() = self.previous.clone();
        });
    }
}

pub(super) struct SliceAliasTargetsGuard;

impl SliceAliasTargetsGuard {
    pub(super) fn clear() -> Self {
        SLICE_ALIAS_TARGETS.with(|aliases| {
            aliases.borrow_mut().clear();
        });
        Self
    }
}

impl Drop for SliceAliasTargetsGuard {
    fn drop(&mut self) {
        SLICE_ALIAS_TARGETS.with(|aliases| {
            aliases.borrow_mut().clear();
        });
    }
}

pub(super) fn is_borrowed_pointer_param_name(name: &str) -> bool {
    BORROWED_POINTER_PARAM_NAMES.with(|borrowed| borrowed.borrow().contains(name))
}

pub(super) fn is_borrowed_slice_param_name(name: &str) -> bool {
    BORROWED_SLICE_PARAM_NAMES.with(|borrowed| borrowed.borrow().contains(name))
}

pub(super) fn is_borrowed_pointer_view_name(name: &str) -> bool {
    BORROWED_POINTER_VIEW_NAMES.with(|borrowed| borrowed.borrow().contains(name))
}

pub(super) fn remove_borrowed_pointer_view_names(names: &[String]) {
    BORROWED_POINTER_VIEW_NAMES.with(|borrowed| {
        let mut borrowed = borrowed.borrow_mut();
        for name in names {
            borrowed.remove(name);
        }
    });
}

pub(super) fn insert_borrowed_pointer_view_name(name: String) {
    BORROWED_POINTER_VIEW_NAMES.with(|borrowed| {
        borrowed.borrow_mut().insert(name);
    });
}

pub(super) fn clear_view_methods() {
    BORROWED_POINTER_VIEW_METHODS.with(|methods| {
        methods.borrow_mut().clear();
    });
    MUTABLE_SLICE_VIEW_METHODS.with(|methods| {
        methods.borrow_mut().clear();
    });
}

pub(super) fn extend_view_methods(
    borrowed_pointer_methods: BTreeMap<String, BorrowedPointerViewReturn>,
    mutable_slice_methods: HashSet<String>,
) {
    BORROWED_POINTER_VIEW_METHODS.with(|methods| {
        methods.borrow_mut().extend(borrowed_pointer_methods);
    });
    MUTABLE_SLICE_VIEW_METHODS.with(|methods| {
        methods.borrow_mut().extend(mutable_slice_methods);
    });
}

pub(super) fn has_borrowed_pointer_view_method(key: &str) -> bool {
    BORROWED_POINTER_VIEW_METHODS.with(|methods| methods.borrow().contains_key(key))
}

pub(super) fn borrowed_pointer_view_method_return(key: &str) -> Option<BorrowedPointerViewReturn> {
    BORROWED_POINTER_VIEW_METHODS.with(|methods| methods.borrow().get(key).cloned())
}

pub(super) fn has_mutable_slice_view_method(key: &str) -> bool {
    MUTABLE_SLICE_VIEW_METHODS.with(|methods| methods.borrow().contains(key))
}

pub(super) fn current_borrowed_pointer_view_return() -> Option<BorrowedPointerViewReturn> {
    BORROWED_POINTER_VIEW_RETURN.with(|info| info.borrow().clone())
}

pub(super) fn current_mutable_slice_view_return() -> Option<MutableSliceViewReturn> {
    MUTABLE_SLICE_VIEW_RETURN.with(|info| info.borrow().clone())
}

pub(super) fn retain_slice_aliases_after_assignment(names: &[String]) {
    SLICE_ALIAS_TARGETS.with(|aliases| {
        let mut aliases = aliases.borrow_mut();
        for name in names {
            aliases.remove(name);
            aliases.retain(|_, target| target.base_name != *name);
        }
    });
}

pub(super) fn insert_slice_alias_target(name: String, target: SliceAliasTarget) {
    SLICE_ALIAS_TARGETS.with(|aliases| {
        aliases.borrow_mut().insert(name, target);
    });
}

pub(super) fn slice_alias_target(name: &str) -> Option<SliceAliasTarget> {
    SLICE_ALIAS_TARGETS.with(|aliases| aliases.borrow().get(name).cloned())
}

pub(super) fn slice_alias_targets_for_base(base_name: &str) -> Vec<(String, SliceAliasTarget)> {
    SLICE_ALIAS_TARGETS.with(|aliases| {
        aliases
            .borrow()
            .iter()
            .filter(|(_, target)| target.base_name == base_name)
            .map(|(alias_name, target)| (alias_name.clone(), target.clone()))
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use quote::quote;

    use super::*;

    #[test]
    fn borrowed_pointer_view_names_guard_restores_previous_names() {
        {
            let _outer = BorrowedPointerViewNamesGuard::clear();
            insert_borrowed_pointer_view_name("outer".to_string());
            assert!(is_borrowed_pointer_view_name("outer"));
            {
                let _inner = BorrowedPointerViewNamesGuard::clear();
                assert!(!is_borrowed_pointer_view_name("outer"));
                insert_borrowed_pointer_view_name("inner".to_string());
                assert!(is_borrowed_pointer_view_name("inner"));
            }
            assert!(is_borrowed_pointer_view_name("outer"));
            assert!(!is_borrowed_pointer_view_name("inner"));
        }
    }

    #[test]
    fn slice_alias_updates_remove_assigned_aliases_and_dependents() {
        let _guard = SliceAliasTargetsGuard::clear();
        insert_slice_alias_target(
            "tail".to_string(),
            SliceAliasTarget {
                base_name: "values".to_string(),
                base_expr: syn::parse_quote! { values },
                offset: syn::parse_quote! { 1usize },
            },
        );
        insert_slice_alias_target(
            "window".to_string(),
            SliceAliasTarget {
                base_name: "tail".to_string(),
                base_expr: syn::parse_quote! { tail },
                offset: syn::parse_quote! { 2usize },
            },
        );

        retain_slice_aliases_after_assignment(&["tail".to_string()]);

        assert!(slice_alias_target("tail").is_none());
        assert!(slice_alias_target("window").is_none());
        assert!(slice_alias_targets_for_base("values").is_empty());
    }

    #[test]
    fn slice_alias_targets_for_base_returns_stored_target() {
        let _guard = SliceAliasTargetsGuard::clear();
        insert_slice_alias_target(
            "tail".to_string(),
            SliceAliasTarget {
                base_name: "values".to_string(),
                base_expr: syn::parse_quote! { values },
                offset: syn::parse_quote! { 3usize },
            },
        );

        let targets = slice_alias_targets_for_base("values");
        assert_eq!(targets.len(), 1);
        assert_eq!(targets.first().map(|(name, _)| name.as_str()), Some("tail"));
        let offset = targets.first().map(|(_, target)| {
            let offset = &target.offset;
            quote!(#offset).to_string()
        });
        let expected = quote!(3usize).to_string();
        assert_eq!(offset.as_deref(), Some(expected.as_str()));
    }
}
