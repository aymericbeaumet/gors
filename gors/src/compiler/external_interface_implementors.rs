use std::cell::RefCell;
use std::collections::BTreeMap;

thread_local! {
    static EXTERNAL_INTERFACE_IMPLEMENTORS: RefCell<BTreeMap<String, Vec<syn::Type>>> = const { RefCell::new(BTreeMap::new()) };
}

pub(super) struct ExternalInterfaceImplementorsGuard {
    previous: BTreeMap<String, Vec<syn::Type>>,
}

impl ExternalInterfaceImplementorsGuard {
    pub(super) fn set(current: BTreeMap<String, Vec<syn::Type>>) -> Self {
        let previous = EXTERNAL_INTERFACE_IMPLEMENTORS
            .with(|implementors| std::mem::replace(&mut *implementors.borrow_mut(), current));
        Self { previous }
    }
}

impl Drop for ExternalInterfaceImplementorsGuard {
    fn drop(&mut self) {
        EXTERNAL_INTERFACE_IMPLEMENTORS.with(|implementors| {
            *implementors.borrow_mut() = self.previous.clone();
        });
    }
}

pub(super) fn has_any() -> bool {
    EXTERNAL_INTERFACE_IMPLEMENTORS.with(|implementors| !implementors.borrow().is_empty())
}

pub(super) fn implementors_for_interface(qualified_name: &str) -> Vec<syn::Type> {
    EXTERNAL_INTERFACE_IMPLEMENTORS.with(|implementors| {
        implementors
            .borrow()
            .get(qualified_name)
            .cloned()
            .unwrap_or_default()
    })
}

#[cfg(test)]
mod tests {
    use quote::quote;

    use super::*;

    #[test]
    fn guard_restores_external_interface_implementors() {
        let mut outer = BTreeMap::new();
        outer.insert(
            "main.Reader".to_string(),
            vec![syn::parse_quote! { crate::local::File }],
        );
        {
            let _outer = ExternalInterfaceImplementorsGuard::set(outer);
            assert!(has_any());
            assert_eq!(
                implementors_for_interface("main.Reader")
                    .into_iter()
                    .map(|ty| quote!(#ty).to_string())
                    .collect::<Vec<_>>(),
                vec![quote!(crate::local::File).to_string()]
            );
            {
                let _inner = ExternalInterfaceImplementorsGuard::set(BTreeMap::new());
                assert!(!has_any());
                assert!(implementors_for_interface("main.Reader").is_empty());
            }
            assert!(has_any());
        }
        assert!(!has_any());
    }

    #[test]
    fn missing_interface_returns_no_implementors() {
        let _guard = ExternalInterfaceImplementorsGuard::set(BTreeMap::new());
        assert!(implementors_for_interface("main.Writer").is_empty());
    }
}
