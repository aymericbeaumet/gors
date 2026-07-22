//! Reconciles generated trait forwarding adapters with their final concrete ABI.
//!
//! Some compiler-owned post-prune replacements change an inherent method from
//! an owned Go slice ABI to a borrowed Rust slice ABI. Interface adapters are
//! emitted before those replacements, so their original `to_vec()` bridge is
//! stale afterward. This pass derives the final ABI from the generated Rust
//! items themselves and removes only conversions that contradict it. A target
//! is eligible only when both its inherent impl and its call use the canonical
//! module-local `Type::method` path; qualified and trait-qualified calls are
//! never reconciled by terminal identifier spelling.

use std::collections::{BTreeMap, BTreeSet};

use syn::visit_mut::VisitMut;

use super::{CompiledModule, interface_impls};

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct LocalInherentCallTarget {
    receiver: String,
    method: String,
}

type BorrowedSliceTargets = BTreeMap<LocalInherentCallTarget, BTreeSet<usize>>;

pub(super) fn reconcile_program(modules: &mut BTreeMap<String, CompiledModule>) {
    for module in modules.values_mut() {
        let targets = collect_borrowed_slice_targets(&module.file);
        if targets.is_empty() {
            continue;
        }
        let mut reconciler = ForwardingAdapterReconciler {
            targets: &targets,
            changed: false,
        };
        for item in &mut module.file.items {
            let syn::Item::Impl(item_impl) = item else {
                continue;
            };
            if item_impl.trait_.is_none() {
                continue;
            }
            reconciler.visit_item_impl_mut(item_impl);
        }
        if reconciler.changed {
            module.content_hash.clear();
        }
    }
}

fn collect_borrowed_slice_targets(file: &syn::File) -> BorrowedSliceTargets {
    let mut targets = BorrowedSliceTargets::new();
    for item in &file.items {
        let syn::Item::Impl(item_impl) = item else {
            continue;
        };
        if item_impl.trait_.is_some() {
            continue;
        }
        let Some(receiver) = canonical_local_receiver(&item_impl.self_ty) else {
            continue;
        };
        for item in &item_impl.items {
            let syn::ImplItem::Fn(method) = item else {
                continue;
            };
            let borrowed = method
                .sig
                .inputs
                .iter()
                .enumerate()
                .filter_map(|(index, input)| match input {
                    syn::FnArg::Typed(arg)
                        if interface_impls::type_is_mut_slice_reference(&arg.ty) =>
                    {
                        Some(index)
                    }
                    syn::FnArg::Receiver(_) | syn::FnArg::Typed(_) => None,
                })
                .collect::<BTreeSet<_>>();
            if !borrowed.is_empty() {
                targets.insert(
                    LocalInherentCallTarget {
                        receiver: receiver.clone(),
                        method: method.sig.ident.to_string(),
                    },
                    borrowed,
                );
            }
        }
    }
    targets
}

struct ForwardingAdapterReconciler<'a> {
    targets: &'a BorrowedSliceTargets,
    changed: bool,
}

impl VisitMut for ForwardingAdapterReconciler<'_> {
    fn visit_expr_call_mut(&mut self, call: &mut syn::ExprCall) {
        syn::visit_mut::visit_expr_call_mut(self, call);
        let Some(target) = canonical_local_inherent_call_target(&call.func) else {
            return;
        };
        let Some(indices) = self.targets.get(&target) else {
            return;
        };
        for index in indices {
            let Some(arg) = call.args.iter_mut().nth(*index) else {
                continue;
            };
            let Some(receiver) = owned_slice_bridge_source(arg) else {
                continue;
            };
            *arg = receiver;
            self.changed = true;
        }
    }
}

fn canonical_local_receiver(ty: &syn::Type) -> Option<String> {
    let syn::Type::Path(path) = ty else {
        return None;
    };
    if path.qself.is_some() || path.path.leading_colon.is_some() || path.path.segments.len() != 1 {
        return None;
    }
    path.path
        .segments
        .first()
        .map(|segment| segment.ident.to_string())
}

fn canonical_local_inherent_call_target(expr: &syn::Expr) -> Option<LocalInherentCallTarget> {
    let syn::Expr::Path(path) = expr else {
        return None;
    };
    if path.qself.is_some() || path.path.leading_colon.is_some() || path.path.segments.len() != 2 {
        return None;
    }
    let mut segments = path.path.segments.iter();
    let receiver = segments.next()?;
    let method = segments.next()?;
    if !matches!(&receiver.arguments, syn::PathArguments::None)
        || !matches!(&method.arguments, syn::PathArguments::None)
    {
        return None;
    }
    Some(LocalInherentCallTarget {
        receiver: receiver.ident.to_string(),
        method: method.ident.to_string(),
    })
}

fn to_vec_receiver(expr: &syn::Expr) -> Option<syn::Expr> {
    let syn::Expr::MethodCall(call) = expr else {
        return None;
    };
    (call.method == "to_vec" && call.args.is_empty()).then(|| (*call.receiver).clone())
}

pub(super) fn owned_slice_bridge_source(expr: &syn::Expr) -> Option<syn::Expr> {
    if let syn::Expr::Reference(reference) = super::syn_inspect::strip_paren_or_group(expr) {
        reference.mutability.as_ref()?;
        return owned_slice_bridge_source(&reference.expr);
    }
    if let Some(receiver) = to_vec_receiver(expr) {
        return Some(receiver);
    }
    let syn::Expr::Block(block) = expr else {
        return None;
    };
    let [
        syn::Stmt::Local(backing),
        syn::Stmt::Local(len),
        syn::Stmt::Expr(result, None),
    ] = block.block.stmts.as_slice()
    else {
        return None;
    };
    let backing_name = super::syn_inspect::pat_ident_name(&backing.pat)?;
    let len_name = super::syn_inspect::pat_ident_name(&len.pat)?;
    if backing_name != "__gors_owned_slice_backing" || len_name != "__gors_owned_slice_len" {
        return None;
    }
    let backing_init = backing.init.as_ref()?;
    let source = to_vec_receiver(&backing_init.expr)?;
    let len_init = len.init.as_ref()?;
    let syn::Expr::MethodCall(len_call) = super::syn_inspect::strip_paren_or_group(&len_init.expr)
    else {
        return None;
    };
    if len_call.method != "len"
        || !len_call.args.is_empty()
        || !matches!(
            super::syn_inspect::strip_paren_or_group(&len_call.receiver),
            syn::Expr::Path(path) if path.path.is_ident(&backing_name)
        )
    {
        return None;
    }
    let syn::Expr::Call(result) = super::syn_inspect::strip_paren_or_group(result) else {
        return None;
    };
    if !super::syn_inspect::is_path_call_expr(
        &result.func,
        &[
            "crate",
            "builtin",
            "GorsSliceStorage",
            "from_initialized_backing",
        ],
    ) || result.args.len() != 2
        || !matches!(
            result.args.first().map(super::syn_inspect::strip_paren_or_group),
            Some(syn::Expr::Path(path)) if path.path.is_ident(&backing_name)
        )
        || !matches!(
            result.args.iter().nth(1).map(super::syn_inspect::strip_paren_or_group),
            Some(syn::Expr::Path(path)) if path.path.is_ident(&len_name)
        )
    {
        return None;
    }
    Some(source)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn removes_owned_slice_bridge_when_final_target_borrows() {
        let mut modules = BTreeMap::from([(
            "os".to_string(),
            CompiledModule {
                mod_name: "os".to_string(),
                import_path: "os".to_string(),
                file: syn::parse_quote! {
                    pub struct File;

                    impl File {
                        pub fn Read(
                            file: crate::builtin::GorsPtr<Self>,
                            bytes: &mut [u8],
                        ) -> isize {
                            0
                        }
                    }

                    impl crate::io::Reader for crate::builtin::GorsPtr<File> {
                        fn Read(&mut self, bytes: &mut [u8]) -> isize {
                            File::Read(self.clone(), {
                                let __gors_owned_slice_backing = (bytes).to_vec();
                                let __gors_owned_slice_len = __gors_owned_slice_backing.len();
                                crate::builtin::GorsSliceStorage::from_initialized_backing(
                                    __gors_owned_slice_backing,
                                    __gors_owned_slice_len,
                                )
                            })
                        }
                    }
                },
                filename: "os.rs".to_string(),
                content_hash: "stale".to_string(),
                is_main: false,
                is_stdlib: true,
            },
        )]);

        reconcile_program(&mut modules);

        let module = modules.get("os").unwrap();
        let source = prettyplease::unparse(&module.file);
        assert!(!source.contains("to_vec"), "{source}");
        assert!(
            source.contains("File::Read(self.clone(), (bytes))"),
            "{source}"
        );
        assert!(module.content_hash.is_empty());
    }

    #[test]
    fn removes_mutably_borrowed_owned_slice_bridge_when_final_target_borrows() {
        let mut modules = BTreeMap::from([(
            "os".to_string(),
            CompiledModule {
                mod_name: "os".to_string(),
                import_path: "os".to_string(),
                file: syn::parse_quote! {
                    pub struct File;

                    impl File {
                        pub fn Read(
                            file: crate::builtin::GorsPtr<Self>,
                            bytes: &mut [u8],
                        ) -> isize {
                            0
                        }
                    }

                    impl crate::io::Reader for crate::builtin::GorsPtr<File> {
                        fn Read(&mut self, bytes: &mut [u8]) -> isize {
                            File::Read(self.clone(), &mut {
                                let __gors_owned_slice_backing = (bytes).to_vec();
                                let __gors_owned_slice_len = __gors_owned_slice_backing.len();
                                crate::builtin::GorsSliceStorage::from_initialized_backing(
                                    __gors_owned_slice_backing,
                                    __gors_owned_slice_len,
                                )
                            })
                        }
                    }
                },
                filename: "os.rs".to_string(),
                content_hash: "stale".to_string(),
                is_main: false,
                is_stdlib: true,
            },
        )]);

        reconcile_program(&mut modules);

        let module = modules.get("os").unwrap();
        let source = prettyplease::unparse(&module.file);
        assert!(!source.contains("to_vec"), "{source}");
        assert!(
            source.contains("File::Read(self.clone(), (bytes))"),
            "{source}"
        );
        assert!(module.content_hash.is_empty());
    }

    #[test]
    fn preserves_owned_slice_bridge_when_target_still_owns() {
        let mut modules = BTreeMap::from([(
            "pkg".to_string(),
            CompiledModule {
                mod_name: "pkg".to_string(),
                import_path: "pkg".to_string(),
                file: syn::parse_quote! {
                    pub struct Sink;

                    impl Sink {
                        pub fn Write(
                            sink: Sink,
                            bytes: crate::builtin::GorsSliceStorage<u8>,
                        ) {}
                    }

                    impl crate::io::Writer for Sink {
                        fn Write(&mut self, bytes: &mut [u8]) {
                            Sink::Write(self.clone(), {
                                let __gors_owned_slice_backing = (bytes).to_vec();
                                let __gors_owned_slice_len = __gors_owned_slice_backing.len();
                                crate::builtin::GorsSliceStorage::from_initialized_backing(
                                    __gors_owned_slice_backing,
                                    __gors_owned_slice_len,
                                )
                            })
                        }
                    }
                },
                filename: "pkg.rs".to_string(),
                content_hash: "stable".to_string(),
                is_main: false,
                is_stdlib: false,
            },
        )]);

        reconcile_program(&mut modules);

        let module = modules.get("pkg").unwrap();
        let source = prettyplease::unparse(&module.file);
        assert!(
            source.contains("GorsSliceStorage::from_initialized_backing"),
            "{source}"
        );
        assert_eq!(module.content_hash, "stable");
    }

    #[test]
    fn preserves_external_same_named_receiver_bridge() {
        let mut modules = BTreeMap::from([(
            "local".to_string(),
            CompiledModule {
                mod_name: "local".to_string(),
                import_path: "local".to_string(),
                file: syn::parse_quote! {
                    pub struct File;

                    impl File {
                        pub fn Read(
                            file: crate::builtin::GorsPtr<Self>,
                            bytes: &mut [u8],
                        ) -> isize {
                            0
                        }
                    }

                    impl crate::io::Reader for crate::builtin::GorsPtr<File> {
                        fn Read(&mut self, bytes: &mut [u8]) -> isize {
                            let _ = File::Read(self.clone(), (bytes).to_vec());
                            crate::other::File::Read(self.clone(), (bytes).to_vec())
                        }
                    }
                },
                filename: "local.rs".to_string(),
                content_hash: "stale".to_string(),
                is_main: false,
                is_stdlib: false,
            },
        )]);

        reconcile_program(&mut modules);

        let module = modules.get("local").unwrap();
        let source = prettyplease::unparse(&module.file);
        assert!(
            source.contains("File::Read(self.clone(), (bytes))"),
            "{source}"
        );
        assert!(
            source.contains("crate::other::File::Read(self.clone(), (bytes).to_vec())"),
            "{source}"
        );
        assert_eq!(source.matches("to_vec").count(), 1, "{source}");
        assert!(module.content_hash.is_empty());
    }

    #[test]
    fn preserves_local_owned_bridge_when_qualified_same_named_impl_borrows() {
        let mut modules = BTreeMap::from([(
            "local".to_string(),
            CompiledModule {
                mod_name: "local".to_string(),
                import_path: "local".to_string(),
                file: syn::parse_quote! {
                    pub struct File;

                    impl File {
                        pub fn Read(
                            file: crate::builtin::GorsPtr<Self>,
                            bytes: Vec<u8>,
                        ) -> isize {
                            0
                        }
                    }

                    impl crate::other::File {
                        pub fn Read(
                            file: crate::builtin::GorsPtr<Self>,
                            bytes: &mut [u8],
                        ) -> isize {
                            0
                        }
                    }

                    impl crate::io::Reader for crate::builtin::GorsPtr<File> {
                        fn Read(&mut self, bytes: &mut [u8]) -> isize {
                            File::Read(self.clone(), (bytes).to_vec())
                        }
                    }
                },
                filename: "local.rs".to_string(),
                content_hash: "stable".to_string(),
                is_main: false,
                is_stdlib: false,
            },
        )]);

        reconcile_program(&mut modules);

        let module = modules.get("local").unwrap();
        let source = prettyplease::unparse(&module.file);
        assert!(
            source.contains("File::Read(self.clone(), (bytes).to_vec())"),
            "{source}"
        );
        assert_eq!(module.content_hash, "stable");
    }

    #[test]
    fn preserves_qualified_trait_call_bridge() {
        let mut modules = BTreeMap::from([(
            "local".to_string(),
            CompiledModule {
                mod_name: "local".to_string(),
                import_path: "local".to_string(),
                file: syn::parse_quote! {
                    pub struct File;

                    impl File {
                        pub fn Read(
                            file: crate::builtin::GorsPtr<Self>,
                            bytes: &mut [u8],
                        ) -> isize {
                            0
                        }
                    }

                    impl crate::io::Reader for crate::builtin::GorsPtr<File> {
                        fn Read(&mut self, bytes: &mut [u8]) -> isize {
                            <crate::other::File as crate::traits::File>::Read(
                                self.clone(),
                                (bytes).to_vec(),
                            )
                        }
                    }
                },
                filename: "local.rs".to_string(),
                content_hash: "stable".to_string(),
                is_main: false,
                is_stdlib: false,
            },
        )]);

        reconcile_program(&mut modules);

        let module = modules.get("local").unwrap();
        let source = prettyplease::unparse(&module.file);
        assert!(source.contains("to_vec"), "{source}");
        assert!(
            source.contains("<crate::other::File as crate::traits::File>::Read"),
            "{source}"
        );
        assert_eq!(module.content_hash, "stable");
    }
}
