use std::cell::RefCell;

thread_local! {
    static MAIN_PACKAGE_TOP_LEVEL_VARS_ARE_LOCALS: RefCell<bool> = const { RefCell::new(false) };
    static CURRENT_GO_PACKAGE_NAME: RefCell<Option<String>> = const { RefCell::new(None) };
    static CURRENT_RUST_MODULE_NAME: RefCell<Option<String>> = const { RefCell::new(None) };
}

pub(super) struct MainPackageVarModeGuard {
    previous: bool,
}

pub(super) struct CurrentGoPackageNameGuard {
    previous: Option<String>,
}

pub(super) struct CurrentRustModuleNameGuard {
    previous: Option<String>,
}

impl MainPackageVarModeGuard {
    pub(super) fn set(current: bool) -> Self {
        let previous = MAIN_PACKAGE_TOP_LEVEL_VARS_ARE_LOCALS.with(|value| {
            let previous = *value.borrow();
            *value.borrow_mut() = current;
            previous
        });
        Self { previous }
    }
}

impl Drop for MainPackageVarModeGuard {
    fn drop(&mut self) {
        MAIN_PACKAGE_TOP_LEVEL_VARS_ARE_LOCALS.with(|value| {
            *value.borrow_mut() = self.previous;
        });
    }
}

impl CurrentGoPackageNameGuard {
    pub(super) fn set(current: String) -> Self {
        let previous = CURRENT_GO_PACKAGE_NAME.with(|name| {
            let previous = name.borrow().clone();
            *name.borrow_mut() = Some(current);
            previous
        });
        Self { previous }
    }
}

impl Drop for CurrentGoPackageNameGuard {
    fn drop(&mut self) {
        CURRENT_GO_PACKAGE_NAME.with(|name| {
            *name.borrow_mut() = self.previous.clone();
        });
    }
}

impl CurrentRustModuleNameGuard {
    pub(super) fn set(current: String) -> Self {
        let previous = CURRENT_RUST_MODULE_NAME.with(|name| {
            let previous = name.borrow().clone();
            *name.borrow_mut() = Some(current);
            previous
        });
        Self { previous }
    }
}

impl Drop for CurrentRustModuleNameGuard {
    fn drop(&mut self) {
        CURRENT_RUST_MODULE_NAME.with(|name| {
            *name.borrow_mut() = self.previous.clone();
        });
    }
}

pub(super) fn main_package_vars_are_locals() -> bool {
    MAIN_PACKAGE_TOP_LEVEL_VARS_ARE_LOCALS.with(|value| *value.borrow())
}

pub(super) fn qualify_interface_name(interface_name: &str) -> String {
    if interface_name == "error" || interface_name.contains('.') {
        return canonicalize_current_package_qualified_name(interface_name);
    }
    current_package_identity()
        .map(|package| format!("{package}.{interface_name}"))
        .unwrap_or_else(|| interface_name.to_string())
}

pub(super) fn local_name_from_current_package_qualified(name: &str) -> Option<String> {
    let (package_name, local_name) = name.rsplit_once('.')?;
    is_current_package_qualifier(package_name).then(|| local_name.to_string())
}

pub(super) fn current_package_qualified_name(name: &str) -> Option<String> {
    if name.contains('.') {
        return None;
    }
    current_package_identity().map(|package_name| format!("{package_name}.{name}"))
}

pub(super) fn current_rust_module_name() -> Option<String> {
    CURRENT_RUST_MODULE_NAME.with(|name| name.borrow().clone())
}

pub(super) fn canonicalize_current_package_qualified_name(name: &str) -> String {
    let Some((qualifier, local_name)) = name.rsplit_once('.') else {
        return name.to_string();
    };
    if !is_current_package_qualifier(qualifier) {
        return name.to_string();
    }
    current_rust_module_name()
        .map(|module| format!("{module}.{local_name}"))
        .unwrap_or_else(|| local_name.to_string())
}

fn current_package_identity() -> Option<String> {
    current_rust_module_name()
        .or_else(|| CURRENT_GO_PACKAGE_NAME.with(|package| package.borrow().clone()))
}

fn is_current_package_qualifier(qualifier: &str) -> bool {
    current_rust_module_name().as_deref() == Some(qualifier)
        || CURRENT_GO_PACKAGE_NAME.with(|package| package.borrow().as_deref() == Some(qualifier))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_var_mode_guard_restores_previous_value() {
        assert!(!main_package_vars_are_locals());
        {
            let _outer = MainPackageVarModeGuard::set(true);
            assert!(main_package_vars_are_locals());
            {
                let _inner = MainPackageVarModeGuard::set(false);
                assert!(!main_package_vars_are_locals());
            }
            assert!(main_package_vars_are_locals());
        }
        assert!(!main_package_vars_are_locals());
    }

    #[test]
    fn package_name_guard_qualifies_local_interfaces_only() {
        assert_eq!(qualify_interface_name("Reader"), "Reader");
        {
            let _package = CurrentGoPackageNameGuard::set("ioish".to_string());
            assert_eq!(qualify_interface_name("Reader"), "ioish.Reader");
            assert_eq!(qualify_interface_name("other.Reader"), "other.Reader");
            assert_eq!(qualify_interface_name("error"), "error");
        }
        assert_eq!(qualify_interface_name("Reader"), "Reader");
    }

    #[test]
    fn package_name_guard_recovers_current_package_local_names() {
        assert_eq!(
            local_name_from_current_package_qualified("ioish.Reader"),
            None
        );
        {
            let _package = CurrentGoPackageNameGuard::set("ioish".to_string());
            assert_eq!(
                local_name_from_current_package_qualified("ioish.Reader").as_deref(),
                Some("Reader")
            );
            assert_eq!(
                local_name_from_current_package_qualified("other.Reader"),
                None
            );
            assert_eq!(local_name_from_current_package_qualified("Reader"), None);
            assert_eq!(
                current_package_qualified_name("Reader").as_deref(),
                Some("ioish.Reader")
            );
            assert_eq!(current_package_qualified_name("other.Reader"), None);
        }
        assert_eq!(
            local_name_from_current_package_qualified("ioish.Reader"),
            None
        );
        assert_eq!(current_package_qualified_name("Reader"), None);
    }

    #[test]
    fn generated_module_guard_canonicalizes_both_self_package_spellings() {
        let _package = CurrentGoPackageNameGuard::set("fs".to_string());
        let _module = CurrentRustModuleNameGuard::set("io__fs".to_string());

        assert_eq!(qualify_interface_name("Reader"), "io__fs.Reader");
        assert_eq!(qualify_interface_name("fs.Reader"), "io__fs.Reader");
        assert_eq!(qualify_interface_name("io__fs.Reader"), "io__fs.Reader");
        assert_eq!(qualify_interface_name("other.Reader"), "other.Reader");
        assert_eq!(
            local_name_from_current_package_qualified("fs.Reader").as_deref(),
            Some("Reader")
        );
        assert_eq!(
            local_name_from_current_package_qualified("io__fs.Reader").as_deref(),
            Some("Reader")
        );
    }
}
