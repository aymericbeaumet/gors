use std::cell::RefCell;

use super::{rust_type_from_inferred_go_type, typeinfer};

thread_local! {
    static RETURN_TYPES: RefCell<Vec<typeinfer::GoType>> = const { RefCell::new(Vec::new()) };
}

pub(super) struct ReturnTypesGuard {
    previous: Vec<typeinfer::GoType>,
}

impl ReturnTypesGuard {
    pub(super) fn set(current: Vec<typeinfer::GoType>) -> Self {
        let previous =
            RETURN_TYPES.with(|types| std::mem::replace(&mut *types.borrow_mut(), current));
        Self { previous }
    }
}

impl Drop for ReturnTypesGuard {
    fn drop(&mut self) {
        RETURN_TYPES.with(|types| {
            *types.borrow_mut() = std::mem::take(&mut self.previous);
        });
    }
}

pub(super) fn expected_types() -> Vec<typeinfer::GoType> {
    RETURN_TYPES.with(|types| types.borrow().clone())
}

pub(super) fn current_syn_type() -> Option<syn::Type> {
    RETURN_TYPES.with(|types| match types.borrow().as_slice() {
        [] => None,
        [ty] => Some(rust_type_from_inferred_go_type(ty)),
        tys => {
            let elems = tys.iter().map(rust_type_from_inferred_go_type);
            Some(syn::parse_quote! { (#(#elems),*) })
        }
    })
}

#[cfg(test)]
mod tests {
    use quote::quote;

    use super::*;

    #[test]
    fn return_types_guard_restores_previous_types() {
        let original = expected_types();
        {
            let _outer = ReturnTypesGuard::set(vec![typeinfer::GoType::String]);
            assert_eq!(expected_types(), vec![typeinfer::GoType::String]);
            {
                let _inner =
                    ReturnTypesGuard::set(vec![typeinfer::GoType::Int, typeinfer::GoType::Bool]);
                assert_eq!(
                    expected_types(),
                    vec![typeinfer::GoType::Int, typeinfer::GoType::Bool]
                );
            }
            assert_eq!(expected_types(), vec![typeinfer::GoType::String]);
        }
        assert_eq!(expected_types(), original);
    }

    #[test]
    fn current_syn_type_matches_active_return_shape() {
        let original = expected_types();
        {
            let _empty = ReturnTypesGuard::set(Vec::new());
            assert!(current_syn_type().is_none());
        }
        {
            let _single = ReturnTypesGuard::set(vec![typeinfer::GoType::Int]);
            let ty = current_syn_type();
            assert!(ty.is_some());
            let ty = ty.unwrap_or_else(|| syn::parse_quote! { __missing });
            assert_eq!(quote!(#ty).to_string(), quote!(isize).to_string());
        }
        {
            let _multi =
                ReturnTypesGuard::set(vec![typeinfer::GoType::String, typeinfer::GoType::Bool]);
            let ty = current_syn_type();
            assert!(ty.is_some());
            let ty = ty.unwrap_or_else(|| syn::parse_quote! { __missing });
            assert_eq!(quote!(#ty).to_string(), quote!((String, bool)).to_string());
        }
        assert_eq!(expected_types(), original);
    }
}
