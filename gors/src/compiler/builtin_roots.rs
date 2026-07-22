pub(super) fn expand(
    roots: &std::collections::HashSet<String>,
) -> std::collections::HashSet<String> {
    let mut expanded = roots.clone();
    if needs_channel_methods(roots) {
        for root in [
            "Chan",
            "ChanIter",
            "ChanInner",
            "Chan::new",
            "Chan::send",
            "Chan::recv",
            "Chan::recv_with_ok",
            "Chan::try_send",
            "Chan::try_recv",
            "Chan::try_recv_with_ok",
            "Chan::is_nil",
            "new",
            "send",
            "recv",
            "recv_with_ok",
            "try_send",
            "try_recv",
            "try_recv_with_ok",
        ] {
            expanded.insert(root.to_string());
        }
    }
    if needs_gors_ptr_methods(roots) {
        for root in [
            "GorsPtr",
            "GorsPtrGuard",
            "GorsPtrInner",
            "GorsNilPointer",
            "ProjectedCell",
            "ProjectedFieldCell",
            "ProjectedFieldGuard",
            "ProjectedIndexCell",
            "ProjectedIndexGuard",
            "ProjectedGuard",
            "IdentityProjectedFieldCell",
            "UnsupportedProjectedGuard",
            "GorsPtr::nil",
            "GorsPtr::new",
            "GorsPtr::from_arc",
            "GorsPtr::from_arc_field",
            "GorsPtr::from_ptr_field",
            "GorsPtr::from_ptr_field_identity",
            "GorsPtr::from_ptr_index",
            "GorsPtr::is_nil",
            "GorsPtr::interface_key",
            "GorsPtr::lock",
            "GorsPtr::ptr_eq",
            "GorsPtr::ptr_id",
        ] {
            expanded.insert(root.to_string());
        }
    }
    if needs_gors_map_methods(roots) {
        for root in [
            "GorsMap",
            "GorsMap::new",
            "GorsMap::with_capacity",
            "GorsMap::is_nil",
            "GorsMap::is_empty",
            "GorsMap::len",
            "GorsMap::capacity",
            "GorsMap::get_with",
            "GorsMap::collect_entries",
            "GorsMap::insert",
            "GorsMap::update_or_insert_with",
            "GorsMap::delete",
            "GorsMap::clear",
            "GorsMap::deep_clone",
        ] {
            expanded.insert(root.to_string());
        }
    }
    if needs_gors_slice_alias_transaction_methods(roots) {
        for root in [
            "GorsSliceAliasResliceErased",
            "GorsSliceAliasReslice",
            "GorsSliceAliasTransaction",
            "GorsSliceAliasTransaction::take_events",
            "GorsSliceAliasTransaction::finish",
            "begin_gors_slice_alias_transaction",
            "record_gors_slice_alias_detach",
            "record_gors_slice_alias_reslice",
        ] {
            expanded.insert(root.to_string());
        }
    }
    if needs_gors_slice_storage_methods(roots) {
        for root in [
            "GorsSliceParam",
            "GorsSliceParamTarget",
            "GorsOwnedSliceStorage",
            "GorsOwnedSliceStorage::gors_owned_slice_storage_mut",
            "GorsSliceStorage",
            "GorsSliceParam::from_storage",
            "GorsSliceParam::from_param",
            "GorsSliceParam::from_owned_storage",
            "GorsSliceParam::reslice",
            "GorsSliceParam::clone",
            "GorsSliceStorage::from_initialized_backing",
            "GorsSliceStorage::len",
            "GorsSliceStorage::is_empty",
            "GorsSliceStorage::capacity",
            "GorsSliceStorage::visible",
            "GorsSliceStorage::visible_mut",
            "GorsSliceStorage::full",
            "GorsSliceStorage::full_mut",
            "GorsSliceStorage::visible_range",
            "GorsSliceStorage::visible_range_mut",
            "GorsSliceStorage::full_range",
            "GorsSliceStorage::full_range_mut",
            "GorsSliceStorage::reslice",
            "GorsSliceStorage::resliced",
            "GorsSliceStorage::into_visible_vec",
            "GorsSliceStorage::checked_full_limit",
            "GorsSliceStorage::absolute_range",
            "GorsSliceStorage::check_bounds",
            "GorsSliceStorage::from_vec",
            "GorsSliceStorage::take_vec",
            "GorsSliceStorage::with_len_capacity",
            "GorsSliceStorage::with_len_capacity_by",
            "GorsSliceStorage::push_visible",
            "GorsSliceStorage::extend_visible",
            "Len",
            "Cap",
            "ByteSeq",
            "Append",
            "StringValue",
            "Clear",
            "IntoIterator",
            "Extend",
            "FromIterator",
        ] {
            expanded.insert(root.to_string());
        }
    }
    if needs_len_trait(roots) {
        expanded.insert("Len".to_string());
    }
    if needs_byte_seq_trait(roots) {
        expanded.insert("ByteSeq".to_string());
    }
    if needs_reflect_value_methods(roots) {
        for root in [
            "__GorsReflectKind",
            "GorsReflectOps",
            "GorsReflectSlice",
            "GorsReflectValue",
            "GorsReflectValue::kind",
            "GorsReflectValue::len",
            "GorsReflectValue::slice",
            "GorsReflectValue::swap",
            "lock_reflect_ops",
            "reflect_type_comparable",
        ] {
            expanded.insert(root.to_string());
        }
    }
    expanded
}

fn needs_channel_methods(roots: &std::collections::HashSet<String>) -> bool {
    roots.iter().any(|root| {
        matches!(
            root.as_str(),
            "Chan"
                | "ChanIter"
                | "ChanInner"
                | "make_chan"
                | "close"
                | "send"
                | "recv"
                | "recv_with_ok"
                | "try_send"
                | "try_recv"
                | "try_recv_with_ok"
                | "Chan::send"
                | "Chan::recv"
                | "Chan::recv_with_ok"
                | "Chan::try_send"
                | "Chan::try_recv"
                | "Chan::try_recv_with_ok"
                | "Chan::len"
                | "Chan::cap"
                | "Chan::is_nil"
        )
    })
}

fn needs_gors_ptr_methods(roots: &std::collections::HashSet<String>) -> bool {
    roots.iter().any(|root| {
        matches!(
            root.as_str(),
            "len" | "Len" | "cap" | "Cap" | "panic_value" | "string_from_byte_seq"
        ) || root == "GorsPtr"
            || root == "GorsNilPointer"
            || root.starts_with("GorsPtr::")
            || root.starts_with("GorsNilPointer::")
    })
}

fn needs_gors_map_methods(roots: &std::collections::HashSet<String>) -> bool {
    roots
        .iter()
        .any(|root| root == "GorsMap" || root.starts_with("GorsMap::"))
}

fn needs_gors_slice_alias_transaction_methods(roots: &std::collections::HashSet<String>) -> bool {
    roots.iter().any(|root| {
        root == "GorsSliceAliasTransaction"
            || root.starts_with("GorsSliceAliasTransaction::")
            || matches!(
                root.as_str(),
                "begin_gors_slice_alias_transaction"
                    | "record_gors_slice_alias_detach"
                    | "record_gors_slice_alias_reslice"
            )
    })
}

fn needs_gors_slice_storage_methods(roots: &std::collections::HashSet<String>) -> bool {
    roots.iter().any(|root| {
        root == "GorsSliceParam"
            || root == "GorsSliceParamTarget"
            || root == "GorsOwnedSliceStorage"
            || root == "GorsSliceStorage"
            || root.starts_with("GorsSliceParam::")
            || root.starts_with("GorsOwnedSliceStorage::")
            || root.starts_with("GorsSliceStorage::")
    })
}

fn needs_len_trait(roots: &std::collections::HashSet<String>) -> bool {
    roots
        .iter()
        .any(|root| matches!(root.as_str(), "len" | "Len" | "string_from_byte_seq"))
}

fn needs_byte_seq_trait(roots: &std::collections::HashSet<String>) -> bool {
    roots.iter().any(|root| {
        matches!(
            root.as_str(),
            "ByteSeq" | "byte_at" | "byte_slice" | "string_from_byte_seq"
        )
    })
}

fn needs_reflect_value_methods(roots: &std::collections::HashSet<String>) -> bool {
    roots.iter().any(|root| {
        matches!(
            root.as_str(),
            "__GorsReflectKind"
                | "GorsReflectOps"
                | "GorsReflectSlice"
                | "GorsReflectValue"
                | "reflect_kind_of_any"
                | "reflect_slice_any"
                | "reflect_type_comparable"
                | "reflect_value_kind"
                | "reflect_value_len"
                | "reflect_value_swapper"
        ) || root.starts_with("GorsReflectOps::")
            || root.starts_with("GorsReflectSlice::")
            || root.starts_with("GorsReflectValue::")
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn reachable_map_storage_keeps_its_runtime_method_family() {
        let roots = std::collections::HashSet::from(["GorsMap".to_string()]);
        let expanded = super::expand(&roots);

        for method in [
            "get_with",
            "collect_entries",
            "insert",
            "update_or_insert_with",
            "delete",
            "clear",
        ] {
            assert!(
                expanded.contains(&format!("GorsMap::{method}")),
                "{expanded:?}"
            );
        }
    }

    #[test]
    fn slice_alias_transaction_roots_keep_finish_and_cleanup() {
        let roots =
            std::collections::HashSet::from(["begin_gors_slice_alias_transaction".to_string()]);
        let expanded = super::expand(&roots);

        assert!(expanded.contains("GorsSliceAliasTransaction::finish"));
        assert!(expanded.contains("GorsSliceAliasTransaction::take_events"));
        assert!(expanded.contains("record_gors_slice_alias_detach"));
        assert!(expanded.contains("record_gors_slice_alias_reslice"));
    }

    #[test]
    fn capacity_slice_storage_roots_keep_the_param_and_owned_storage_families() {
        for root in ["GorsSliceParam::from_owned_storage", "GorsSliceStorage"] {
            let roots = std::collections::HashSet::from([root.to_string()]);
            let expanded = super::expand(&roots);

            for expected in [
                "GorsSliceParamTarget",
                "GorsSliceParam::from_owned_storage",
                "GorsSliceStorage::len",
                "GorsSliceStorage::capacity",
                "GorsSliceStorage::with_len_capacity_by",
                "GorsSliceStorage::full",
                "GorsSliceStorage::full_mut",
                "GorsSliceStorage::take_vec",
                "GorsSliceStorage::absolute_range",
                "GorsSliceStorage::check_bounds",
                "Len",
                "Cap",
                "ByteSeq",
                "Append",
                "StringValue",
                "Clear",
                "IntoIterator",
                "Extend",
            ] {
                assert!(expanded.contains(expected), "{root}: {expanded:?}");
            }
        }
    }

    #[test]
    fn generated_builtin_keeps_the_complete_owned_slice_storage_family() {
        let mut file = syn::parse_file(include_str!("../../../gors-builtin/src/lib.rs"))
            .expect("builtin source should parse");
        let roots = std::collections::HashSet::from(["GorsSliceStorage".to_string()]);
        let roots = super::expand(&roots);
        let module_names = std::collections::HashSet::new();

        let reachable =
            super::super::prune_builtin_items_to_roots(&mut file.items, &roots, &module_names);
        super::super::builtin_pruning::prune_unneeded_traits(&mut file.items, &reachable);

        let has_method = |method: &str| {
            file.items.iter().any(|item| {
                let syn::Item::Impl(item_impl) = item else {
                    return false;
                };
                item_impl.trait_.is_none()
                    && super::super::syn_inspect::named_self_type(&item_impl.self_ty).as_deref()
                        == Some("GorsSliceStorage")
                    && item_impl.items.iter().any(
                        |item| matches!(item, syn::ImplItem::Fn(func) if func.sig.ident == method),
                    )
            })
        };
        for method in [
            "len",
            "is_empty",
            "capacity",
            "full",
            "full_mut",
            "take_vec",
            "reslice",
            "push_visible",
            "extend_visible",
        ] {
            assert!(has_method(method), "missing GorsSliceStorage::{method}");
        }

        let has_trait_impl = |self_name: &str, trait_name: &str| {
            file.items.iter().any(|item| {
                let syn::Item::Impl(item_impl) = item else {
                    return false;
                };
                super::super::syn_inspect::named_self_type(&item_impl.self_ty).as_deref()
                    == Some(self_name)
                    && item_impl
                        .trait_
                        .as_ref()
                        .and_then(|(_, path, _)| path.segments.last())
                        .is_some_and(|segment| segment.ident == trait_name)
            })
        };
        for trait_name in [
            "GorsOwnedSliceStorage",
            "Len",
            "Cap",
            "ByteSeq",
            "Append",
            "StringValue",
            "Clear",
            "IntoIterator",
            "Extend",
        ] {
            assert!(
                has_trait_impl("GorsSliceStorage", trait_name),
                "missing {trait_name} for GorsSliceStorage"
            );
        }
        assert!(
            has_trait_impl("GorsSliceParam", "Drop"),
            "missing Drop for GorsSliceParam"
        );
    }
}
