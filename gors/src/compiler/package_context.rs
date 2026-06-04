use std::cell::RefCell;

thread_local! {
    static MAIN_PACKAGE_TOP_LEVEL_VARS_ARE_LOCALS: RefCell<bool> = const { RefCell::new(false) };
    static CURRENT_GO_PACKAGE_NAME: RefCell<Option<String>> = const { RefCell::new(None) };
}

pub(super) struct MainPackageVarModeGuard {
    previous: bool,
}

pub(super) struct CurrentGoPackageNameGuard {
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

pub(super) fn main_package_vars_are_locals() -> bool {
    MAIN_PACKAGE_TOP_LEVEL_VARS_ARE_LOCALS.with(|value| *value.borrow())
}

pub(super) fn qualify_interface_name(interface_name: &str) -> String {
    if interface_name == "error" || interface_name.contains('.') {
        return interface_name.to_string();
    }
    CURRENT_GO_PACKAGE_NAME.with(|package| {
        package
            .borrow()
            .as_ref()
            .map(|package| format!("{package}.{interface_name}"))
            .unwrap_or_else(|| interface_name.to_string())
    })
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
}
