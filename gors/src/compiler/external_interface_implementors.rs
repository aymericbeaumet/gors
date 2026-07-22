use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use super::typeinfer;

#[derive(Clone)]
pub(super) struct ExternalInterfaceImplementor {
    pub(super) go_name: String,
    pub(super) rust_ty: syn::Type,
    pub(super) include_pointer_receiver_methods: bool,
    /// Exact methods whose generated receiver ABI consumes `GorsPtr<Self>`.
    ///
    /// The broader boolean above identifies the pointer method set as a whole;
    /// it cannot distinguish pointer receivers from value receivers when an
    /// opaque external implementor mixes both.
    pub(super) pointer_receiver_methods: BTreeSet<String>,
}

thread_local! {
    static EXTERNAL_INTERFACE_IMPLEMENTORS: RefCell<ExternalInterfaceImplementorMap> =
        const { RefCell::new(BTreeMap::new()) };
}

pub(super) type ExternalInterfaceImplementorMap =
    BTreeMap<String, Vec<ExternalInterfaceImplementor>>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ExternalInterfaceImplementorWire {
    go_name: String,
    rust_ty: String,
    include_pointer_receiver_methods: bool,
    pointer_receiver_methods: BTreeSet<String>,
}

pub(super) type ExternalInterfaceImplementorWireMap =
    BTreeMap<String, Vec<ExternalInterfaceImplementorWire>>;
pub(super) type ExternalInterfaceImplementorsSnapshot = Arc<ExternalInterfaceImplementorWireMap>;

pub(super) struct ExternalInterfaceImplementorsGuard {
    previous: ExternalInterfaceImplementorMap,
}

impl ExternalInterfaceImplementorsGuard {
    pub(super) fn set(current: ExternalInterfaceImplementorMap) -> Self {
        let previous = EXTERNAL_INTERFACE_IMPLEMENTORS
            .with(|implementors| std::mem::replace(&mut *implementors.borrow_mut(), current));
        Self { previous }
    }

    pub(super) fn set_snapshot(current: ExternalInterfaceImplementorsSnapshot) -> Self {
        let current = current
            .iter()
            .map(|(interface, records)| {
                let records = records
                    .iter()
                    .map(|record| ExternalInterfaceImplementor {
                        go_name: record.go_name.clone(),
                        rust_ty: syn::parse_str(&record.rust_ty).unwrap_or_else(|error| {
                            panic!(
                                "invalid compiler-owned external implementor type `{}`: {error}",
                                record.rust_ty
                            )
                        }),
                        include_pointer_receiver_methods: record.include_pointer_receiver_methods,
                        pointer_receiver_methods: record.pointer_receiver_methods.clone(),
                    })
                    .collect();
                (interface.clone(), records)
            })
            .collect();
        Self::set(current)
    }
}

impl Drop for ExternalInterfaceImplementorsGuard {
    fn drop(&mut self) {
        EXTERNAL_INTERFACE_IMPLEMENTORS.with(|implementors| {
            *implementors.borrow_mut() = std::mem::take(&mut self.previous);
        });
    }
}

pub(super) fn snapshot() -> ExternalInterfaceImplementorsSnapshot {
    EXTERNAL_INTERFACE_IMPLEMENTORS.with(|implementors| {
        Arc::new(
            implementors
                .borrow()
                .iter()
                .map(|(interface, records)| {
                    let records = records
                        .iter()
                        .map(|record| {
                            let rust_ty = &record.rust_ty;
                            ExternalInterfaceImplementorWire {
                                go_name: record.go_name.clone(),
                                rust_ty: quote::quote! { #rust_ty }.to_string(),
                                include_pointer_receiver_methods: record
                                    .include_pointer_receiver_methods,
                                pointer_receiver_methods: record.pointer_receiver_methods.clone(),
                            }
                        })
                        .collect();
                    (interface.clone(), records)
                })
                .collect(),
        )
    })
}

pub(super) fn snapshot_has_any(snapshot: &ExternalInterfaceImplementorsSnapshot) -> bool {
    !snapshot.is_empty()
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
    // An exact whole-program target record can name a concrete type that is
    // opaque to the package performing the assertion. In that case the local
    // environment cannot disprove the source-interface relationship: the
    // source interface's dynamic-value contract already makes an incompatible
    // candidate unreachable. Keep the target record and only apply the local
    // structural filter when declaration facts for the concrete type exist.
    if env.get_type_kind(&record.go_name).is_none() {
        return true;
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

pub(super) fn contains_record(qualified_name: &str, record: &ExternalInterfaceImplementor) -> bool {
    EXTERNAL_INTERFACE_IMPLEMENTORS.with(|implementors| {
        implementors
            .borrow()
            .get(qualified_name)
            .is_some_and(|records| {
                records.iter().any(|candidate| {
                    let candidate_ty = &candidate.rust_ty;
                    let record_ty = &record.rust_ty;
                    candidate.include_pointer_receiver_methods
                        == record.include_pointer_receiver_methods
                        && quote::quote! { #candidate_ty }.to_string()
                            == quote::quote! { #record_ty }.to_string()
                })
            })
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
                pointer_receiver_methods: BTreeSet::new(),
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

    #[test]
    fn opaque_external_target_record_survives_missing_source_census() {
        let opaque = ExternalInterfaceImplementor {
            go_name: "values.Store".to_string(),
            rust_ty: syn::parse_quote! { crate::values::Store },
            include_pointer_receiver_methods: false,
            pointer_receiver_methods: BTreeSet::new(),
        };
        let _guard = ExternalInterfaceImplementorsGuard::set(BTreeMap::from([(
            "contracts.__gors_anonymous_interface_writer".to_string(),
            vec![opaque],
        )]));
        let mut env = typeinfer::TypeEnv::new();
        env.set_type_kind("contracts.Reader", typeinfer::TypeKind::Interface);
        env.set_interface_methods("contracts.Reader", vec!["Read".to_string()]);

        let implementors = implementors_for_interface_filtered(
            "contracts.__gors_anonymous_interface_writer",
            Some("contracts.Reader"),
            &env,
        );

        assert_eq!(implementors.len(), 1);
        let implementor = &implementors[0];
        assert_eq!(
            quote::quote! { #implementor }.to_string(),
            quote::quote! { crate::values::Store }.to_string()
        );
    }

    #[cfg(all(feature = "parallel", not(target_family = "wasm")))]
    #[test]
    fn snapshot_installs_identical_implementors_on_rayon_workers() {
        use rayon::prelude::*;

        let expected = quote!(crate::local::File).to_string();
        let records = BTreeMap::from([(
            "main.Reader".to_string(),
            vec![ExternalInterfaceImplementor {
                go_name: "local.File".to_string(),
                rust_ty: syn::parse_quote! { crate::local::File },
                include_pointer_receiver_methods: true,
                pointer_receiver_methods: BTreeSet::from(["Read".to_string()]),
            }],
        )]);
        let _outer = ExternalInterfaceImplementorsGuard::set(records);
        let snapshot = snapshot();
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(4)
            .build()
            .unwrap();

        let observed = pool.install(|| {
            (0..16)
                .into_par_iter()
                .map(|_| {
                    let _worker =
                        ExternalInterfaceImplementorsGuard::set_snapshot(snapshot.clone());
                    assert_eq!(
                        records_for_interface("main.Reader")[0].pointer_receiver_methods,
                        BTreeSet::from(["Read".to_string()]),
                    );
                    implementors_for_interface("main.Reader")
                        .into_iter()
                        .map(|ty| quote!(#ty).to_string())
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        });

        assert!(
            observed
                .iter()
                .all(|implementors| implementors == &vec![expected.clone()]),
            "{observed:?}"
        );
        assert_eq!(
            implementors_for_interface("main.Reader")
                .into_iter()
                .map(|ty| quote!(#ty).to_string())
                .collect::<Vec<_>>(),
            vec![expected]
        );
    }
}
