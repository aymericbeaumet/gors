use super::{ImplSelfType, has_impl, type_path_ident_name};

pub(super) fn inject_state(items: &mut Vec<syn::Item>) {
    let Some(methods) = trait_methods(items, "State") else {
        return;
    };
    let forwarders = named_trait_impl_self_types(items, "State")
        .into_iter()
        .filter(|self_ty| {
            !has_impl(
                items,
                "State",
                ImplSelfType::MutableReferenceToNamed(self_ty),
            )
        })
        .filter_map(|self_ty| mutable_ref_trait_forwarder("State", &self_ty, &methods))
        .collect::<Vec<_>>();

    for forwarder in forwarders {
        items.insert(0, forwarder);
    }
}

fn trait_methods(items: &[syn::Item], trait_name: &str) -> Option<Vec<syn::TraitItemFn>> {
    items.iter().find_map(|item| {
        let syn::Item::Trait(item_trait) = item else {
            return None;
        };
        (item_trait.ident == trait_name).then(|| {
            item_trait
                .items
                .iter()
                .filter_map(|item| {
                    let syn::TraitItem::Fn(func) = item else {
                        return None;
                    };
                    Some(func.clone())
                })
                .collect()
        })
    })
}

fn named_trait_impl_self_types(items: &[syn::Item], trait_name: &str) -> Vec<String> {
    let mut names = std::collections::BTreeSet::new();
    for item in items {
        let syn::Item::Impl(item_impl) = item else {
            continue;
        };
        let Some((_, path, _)) = &item_impl.trait_ else {
            continue;
        };
        if path
            .segments
            .last()
            .is_none_or(|seg| seg.ident != trait_name)
        {
            continue;
        }
        if let Some(name) = type_path_ident_name(&item_impl.self_ty) {
            names.insert(name);
        }
    }
    names.into_iter().collect()
}

fn mutable_ref_trait_forwarder(
    trait_name: &str,
    self_ty: &str,
    methods: &[syn::TraitItemFn],
) -> Option<syn::Item> {
    let trait_ident = syn::Ident::new(trait_name, proc_macro2::Span::mixed_site());
    let self_ty_ident = syn::Ident::new(self_ty, proc_macro2::Span::mixed_site());
    let methods = methods
        .iter()
        .map(|method| forwarding_impl_method(&trait_ident, &self_ty_ident, method))
        .collect::<Option<Vec<_>>>()?;

    Some(syn::parse_quote! {
        impl<'a> #trait_ident for &'a mut #self_ty_ident {
            #(#methods)*
        }
    })
}

fn forwarding_impl_method(
    trait_ident: &syn::Ident,
    self_ty_ident: &syn::Ident,
    method: &syn::TraitItemFn,
) -> Option<syn::ImplItemFn> {
    let sig = method.sig.clone();
    let method_ident = &sig.ident;
    let args = forwarding_call_args(&sig)?;

    Some(syn::parse_quote! {
        #sig {
            <#self_ty_ident as #trait_ident>::#method_ident(#(#args),*)
        }
    })
}

fn forwarding_call_args(sig: &syn::Signature) -> Option<Vec<syn::Expr>> {
    let mut args = Vec::new();
    for input in &sig.inputs {
        match input {
            syn::FnArg::Receiver(receiver) => {
                args.push(forwarding_receiver_arg(receiver)?);
            }
            syn::FnArg::Typed(typed) => {
                args.push(pat_ident_expr(&typed.pat)?);
            }
        }
    }
    Some(args)
}

fn forwarding_receiver_arg(receiver: &syn::Receiver) -> Option<syn::Expr> {
    match (receiver.reference.is_some(), receiver.mutability.is_some()) {
        (true, true) => Some(syn::parse_quote! { &mut **self }),
        (true, false) => Some(syn::parse_quote! { &**self }),
        (false, _) => None,
    }
}

fn pat_ident_expr(pat: &syn::Pat) -> Option<syn::Expr> {
    let syn::Pat::Ident(ident) = pat else {
        return None;
    };
    let ident = &ident.ident;
    Some(syn::parse_quote! { #ident })
}
