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
        matches!(root.as_str(), "len" | "Len" | "cap" | "Cap" | "panic_value")
            || root == "GorsPtr"
            || root == "GorsNilPointer"
            || root.starts_with("GorsPtr::")
            || root.starts_with("GorsNilPointer::")
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
