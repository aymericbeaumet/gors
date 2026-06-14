use crate::generated_names::{AS_ANY_METHOD, CLONE_BOX_METHOD, INTERFACE_KEY_METHOD};

pub enum CloneBoxPolicy<'a> {
    BoxDefault {
        default_expr: &'a syn::Expr,
        trait_path: &'a syn::Path,
    },
    UseNonHookReturn,
}

pub enum NonHookReturnPolicy {
    Default,
    Panic,
}

pub struct MethodPolicy<'a> {
    pub clone_box: CloneBoxPolicy<'a>,
    pub non_hook_return: NonHookReturnPolicy,
}

pub fn impl_fn_for_trait_method(
    method: &syn::TraitItemFn,
    policy: &MethodPolicy<'_>,
) -> syn::ImplItemFn {
    impl_fn_for_signature(method.sig.clone(), policy)
}

pub fn impl_item_for_signature(sig: syn::Signature, policy: &MethodPolicy<'_>) -> syn::ImplItem {
    syn::ImplItem::Fn(impl_fn_for_signature(sig, policy))
}

fn impl_fn_for_signature(sig: syn::Signature, policy: &MethodPolicy<'_>) -> syn::ImplItemFn {
    let block = method_body(&sig, policy);
    syn::ImplItemFn {
        attrs: vec![],
        vis: syn::Visibility::Inherited,
        defaultness: None,
        sig,
        block,
    }
}

fn method_body(sig: &syn::Signature, policy: &MethodPolicy<'_>) -> syn::Block {
    if sig.ident == AS_ANY_METHOD {
        return syn::parse_quote!({ None });
    }
    if sig.ident == INTERFACE_KEY_METHOD {
        return syn::parse_quote!({ crate::builtin::GorsInterfaceKey::nil() });
    }
    if sig.ident == CLONE_BOX_METHOD
        && let CloneBoxPolicy::BoxDefault {
            default_expr,
            trait_path,
        } = policy.clone_box
    {
        return syn::parse_quote!({ Box::new(#default_expr) as Box<dyn #trait_path> });
    }
    if matches!(sig.output, syn::ReturnType::Default) {
        return syn::parse_quote!({});
    }
    match policy.non_hook_return {
        NonHookReturnPolicy::Default => syn::parse_quote!({ Default::default() }),
        NonHookReturnPolicy::Panic => {
            syn::parse_quote!({ crate::builtin::panic_value("called no-op interface method") })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;

    #[test]
    fn clone_box_hook_can_box_the_declared_noop_type() {
        let sig: syn::Signature = syn::parse_quote! {
            fn __gors_clone_box(&self) -> Box<dyn Reader>
        };
        let default_expr: syn::Expr = syn::parse_quote! { __GorsNoopReader::default() };
        let trait_path: syn::Path = syn::parse_quote! { Reader };
        let policy = MethodPolicy {
            clone_box: CloneBoxPolicy::BoxDefault {
                default_expr: &default_expr,
                trait_path: &trait_path,
            },
            non_hook_return: NonHookReturnPolicy::Panic,
        };

        let item = impl_item_for_signature(sig, &policy);
        let tokens = quote!(#item).to_string();

        assert!(
            tokens.contains("Box :: new (__GorsNoopReader :: default ()) as Box < dyn Reader >"),
            "{tokens}"
        );
    }

    #[test]
    fn non_hook_return_policy_is_explicit() {
        let sig: syn::Signature = syn::parse_quote! {
            fn Count(&mut self) -> isize
        };
        let default_policy = MethodPolicy {
            clone_box: CloneBoxPolicy::UseNonHookReturn,
            non_hook_return: NonHookReturnPolicy::Default,
        };
        let panic_policy = MethodPolicy {
            clone_box: CloneBoxPolicy::UseNonHookReturn,
            non_hook_return: NonHookReturnPolicy::Panic,
        };

        let default_item = impl_item_for_signature(sig.clone(), &default_policy);
        let panic_item = impl_item_for_signature(sig, &panic_policy);
        let default_tokens = quote!(#default_item).to_string();
        let panic_tokens = quote!(#panic_item).to_string();

        assert!(
            default_tokens.contains("Default :: default ()"),
            "{default_tokens}"
        );
        assert!(
            panic_tokens.contains("called no-op interface method"),
            "{panic_tokens}"
        );
    }
}
