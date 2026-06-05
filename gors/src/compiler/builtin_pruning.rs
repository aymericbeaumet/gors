use std::collections::HashSet;

use super::syn_inspect::{item_name, named_self_type};

pub(super) fn prune_channel_helpers(items: &mut Vec<syn::Item>, roots: &HashSet<String>) {
    if roots.iter().any(|root| {
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
                | "Chan::send"
                | "Chan::recv"
                | "Chan::recv_with_ok"
                | "Chan::len"
                | "Chan::cap"
        )
    }) {
        return;
    }

    let channel_names = HashSet::from([
        "Chan".to_string(),
        "ChanIter".to_string(),
        "ChanInner".to_string(),
        "lock_chan".to_string(),
        "wait_chan".to_string(),
        "make_chan".to_string(),
        "close".to_string(),
    ]);
    items.retain(|item| {
        if item_name(item).is_some_and(|name| channel_names.contains(&name)) {
            return false;
        }
        let syn::Item::Impl(item_impl) = item else {
            return true;
        };
        !named_self_type(&item_impl.self_ty).is_some_and(|name| channel_names.contains(&name))
    });
}

pub(super) fn prune_complex_helpers(items: &mut Vec<syn::Item>, roots: &HashSet<String>) {
    let needs_complex64 = roots.iter().any(|root| {
        matches!(
            root.as_str(),
            "Complex64" | "complex64" | "real64" | "imag64" | "to_complex64"
        )
    });
    let needs_complex_conversions = roots.iter().any(|root| {
        matches!(
            root.as_str(),
            "to_complex64" | "to_complex128" | "Complex64Value" | "Complex128Value"
        )
    });

    items.retain(|item| {
        if let Some(name) = item_name(item)
            && name == "impl_real_complex_conversions"
        {
            return needs_complex_conversions;
        }
        if let syn::Item::Struct(item_struct) = item
            && item_struct.ident == "Complex64"
        {
            return needs_complex64 || needs_complex_conversions;
        }
        if let syn::Item::Trait(item_trait) = item
            && matches!(
                item_trait.ident.to_string().as_str(),
                "Complex64Value" | "Complex128Value"
            )
        {
            return needs_complex_conversions;
        }
        let syn::Item::Impl(item_impl) = item else {
            return true;
        };
        if named_self_type(&item_impl.self_ty).is_some_and(|name| name == "Complex64") {
            return needs_complex64 || needs_complex_conversions;
        }
        if let Some((_, path, _)) = &item_impl.trait_
            && path.segments.last().is_some_and(|seg| {
                matches!(
                    seg.ident.to_string().as_str(),
                    "Complex64Value" | "Complex128Value"
                )
            })
        {
            return needs_complex_conversions;
        }
        true
    });
}

pub(super) fn prune_bitcast_helpers(items: &mut Vec<syn::Item>, roots: &HashSet<String>) {
    if roots.contains("bitcast_ref") {
        return;
    }

    items.retain(|item| {
        if let syn::Item::Trait(item_trait) = item
            && item_trait.ident == "BitcastFrom"
        {
            return false;
        }
        let syn::Item::Impl(item_impl) = item else {
            return true;
        };
        item_impl
            .trait_
            .as_ref()
            .and_then(|(_, path, _)| path.segments.last())
            .is_none_or(|seg| seg.ident != "BitcastFrom")
    });
}

pub(super) fn prune_unneeded_traits(items: &mut Vec<syn::Item>, builtin_roots: &HashSet<String>) {
    items.retain(|item| {
        if let syn::Item::Trait(item_trait) = item
            && let Some(needed_root) = builtin_trait_required_root(&item_trait.ident.to_string())
        {
            return builtin_roots.contains(needed_root)
                || builtin_roots.contains(&item_trait.ident.to_string());
        }

        let syn::Item::Impl(item_impl) = item else {
            return true;
        };
        let Some((_, trait_path, _)) = &item_impl.trait_ else {
            return true;
        };
        let Some(trait_name) = trait_path
            .segments
            .last()
            .map(|segment| segment.ident.to_string())
        else {
            return true;
        };
        let needed_root = builtin_trait_required_root(&trait_name);
        let Some(needed_root) = needed_root else {
            return true;
        };
        builtin_roots.contains(needed_root) || builtin_roots.contains(&trait_name)
    });
}

fn builtin_trait_required_root(trait_name: &str) -> Option<&'static str> {
    match trait_name {
        "Append" => Some("append"),
        "Cap" => Some("cap"),
        "Len" => Some("len"),
        "StringValue" => Some("string"),
        _ => None,
    }
}
