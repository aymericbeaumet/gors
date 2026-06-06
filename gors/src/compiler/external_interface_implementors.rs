use std::cell::RefCell;
use std::collections::BTreeMap;

use super::typeinfer;

#[derive(Clone)]
pub(super) struct ExternalInterfaceImplementor {
    pub(super) go_name: String,
    pub(super) rust_ty: syn::Type,
    pub(super) include_pointer_receiver_methods: bool,
}

thread_local! {
    static EXTERNAL_INTERFACE_IMPLEMENTORS: RefCell<BTreeMap<String, Vec<ExternalInterfaceImplementor>>> = const { RefCell::new(BTreeMap::new()) };
}

pub(super) struct ExternalInterfaceImplementorsGuard {
    previous: BTreeMap<String, Vec<ExternalInterfaceImplementor>>,
}

impl ExternalInterfaceImplementorsGuard {
    pub(super) fn set(current: BTreeMap<String, Vec<ExternalInterfaceImplementor>>) -> Self {
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

#[cfg(test)]
pub(super) fn implementors_for_interface(qualified_name: &str) -> Vec<syn::Type> {
    EXTERNAL_INTERFACE_IMPLEMENTORS.with(|implementors| {
        implementors
            .borrow()
            .get(qualified_name)
            .map(|records| {
                records
                    .iter()
                    .map(|record| record.rust_ty.clone())
                    .collect()
            })
            .unwrap_or_default()
    })
}

pub(super) fn implementors_for_interface_filtered(
    qualified_name: &str,
    source_interface: Option<&str>,
    env: &typeinfer::TypeEnv,
) -> Vec<syn::Type> {
    EXTERNAL_INTERFACE_IMPLEMENTORS.with(|implementors| {
        let implementors = implementors.borrow();
        let source_records = source_interface.and_then(|name| implementors.get(name));
        implementors
            .get(qualified_name)
            .map(|records| {
                records
                    .iter()
                    .filter(|record| {
                        record_matches_source_interface(
                            record,
                            source_interface,
                            source_records,
                            env,
                        )
                    })
                    .map(|record| record.rust_ty.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    })
}

fn record_matches_source_interface(
    record: &ExternalInterfaceImplementor,
    source_interface: Option<&str>,
    source_records: Option<&Vec<ExternalInterfaceImplementor>>,
    env: &typeinfer::TypeEnv,
) -> bool {
    let Some(source_interface) = source_interface else {
        return true;
    };
    if let Some(source_records) = source_records {
        return source_records.iter().any(|source_record| {
            source_record.go_name == record.go_name
                && source_record.include_pointer_receiver_methods
                    == record.include_pointer_receiver_methods
        });
    }
    env.named_type_implements_interface(
        &record.go_name,
        source_interface,
        record.include_pointer_receiver_methods,
    )
}

pub(super) fn records_for_interface(qualified_name: &str) -> Vec<ExternalInterfaceImplementor> {
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
            vec![ExternalInterfaceImplementor {
                go_name: "local.File".to_string(),
                rust_ty: syn::parse_quote! { crate::local::File },
                include_pointer_receiver_methods: false,
            }],
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
