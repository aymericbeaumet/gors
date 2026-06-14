use std::cell::RefCell;
use std::collections::HashSet;

thread_local! {
    static ACTIVE_REACHABILITY_ROOTS: RefCell<Option<HashSet<String>>> = const { RefCell::new(None) };
}

pub(super) fn with_active_roots<T>(roots: Option<&HashSet<String>>, f: impl FnOnce() -> T) -> T {
    ACTIVE_REACHABILITY_ROOTS.with(|active| {
        let previous = active.replace(roots.cloned());
        let result = f();
        active.replace(previous);
        result
    })
}

pub(super) fn active_roots_allow(name: &str) -> bool {
    ACTIVE_REACHABILITY_ROOTS.with(|roots| {
        roots
            .borrow()
            .as_ref()
            .is_none_or(|roots| roots.contains(name))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_active_roots_allow_any_name() {
        with_active_roots(None, || {
            assert!(active_roots_allow("NewReader"));
        });
    }

    #[test]
    fn active_roots_filter_names_and_restore_previous_scope() {
        let outer = HashSet::from(["NewReader".to_string()]);
        let inner = HashSet::from(["NewWriter".to_string()]);

        with_active_roots(Some(&outer), || {
            assert!(active_roots_allow("NewReader"));
            assert!(!active_roots_allow("NewWriter"));

            with_active_roots(Some(&inner), || {
                assert!(!active_roots_allow("NewReader"));
                assert!(active_roots_allow("NewWriter"));
            });

            assert!(active_roots_allow("NewReader"));
            assert!(!active_roots_allow("NewWriter"));
        });
    }
}
