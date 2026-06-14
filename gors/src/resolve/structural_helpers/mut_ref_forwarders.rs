use super::syn_helpers::{ImplSelfType, type_path_ident_name};
use crate::generated_names::{AS_ANY_METHOD, CLONE_BOX_METHOD};

pub(super) fn inject(items: &mut Vec<syn::Item>) {
    let mut forwarders = Vec::new();
    for trait_impl in forwardable_impls(items) {
        if has_matching_impl(
            items,
            &trait_impl.trait_path,
            ImplSelfType::MutableReferenceToNamed(&trait_impl.self_ty),
        ) {
            continue;
        }
        let Some(forwarder) = mutable_ref_trait_forwarder(
            &trait_impl.trait_path,
            &trait_impl.self_ty,
            &trait_impl.methods,
        ) else {
            continue;
        };
        forwarders.push(forwarder);
    }

    for forwarder in forwarders {
        items.insert(0, forwarder);
    }
}

struct ForwardableImpl {
    trait_path: syn::Path,
    self_ty: String,
    methods: Vec<syn::ImplItemFn>,
}

fn forwardable_impls(items: &[syn::Item]) -> Vec<ForwardableImpl> {
    let mut out = items
        .iter()
        .filter_map(|item| {
            let syn::Item::Impl(item_impl) = item else {
                return None;
            };
            let (_, trait_path, _) = item_impl.trait_.as_ref()?;
            let self_ty = type_path_ident_name(&item_impl.self_ty)?;
            let methods = item_impl
                .items
                .iter()
                .map(|item| match item {
                    syn::ImplItem::Fn(func) => Some(func.clone()),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()?;
            (impl_has_interface_hooks(&methods) && !methods.is_empty()).then(|| ForwardableImpl {
                trait_path: trait_path.clone(),
                self_ty,
                methods,
            })
        })
        .collect::<Vec<_>>();
    out.sort_by(|left, right| {
        path_key(&left.trait_path)
            .cmp(&path_key(&right.trait_path))
            .then_with(|| left.self_ty.cmp(&right.self_ty))
    });
    out
}

fn impl_has_interface_hooks(methods: &[syn::ImplItemFn]) -> bool {
    methods.iter().any(|method| {
        let ident = &method.sig.ident;
        ident == AS_ANY_METHOD || ident == CLONE_BOX_METHOD
    })
}

fn mutable_ref_trait_forwarder(
    trait_path: &syn::Path,
    self_ty: &str,
    methods: &[syn::ImplItemFn],
) -> Option<syn::Item> {
    let self_ty_ident = syn::Ident::new(self_ty, proc_macro2::Span::mixed_site());
    let methods = methods
        .iter()
        .map(|method| forwarding_impl_method(trait_path, method))
        .collect::<Option<Vec<_>>>()?;

    Some(syn::parse_quote! {
        impl<'a> #trait_path for &'a mut #self_ty_ident {
            #(#methods)*
        }
    })
}

fn forwarding_impl_method(
    trait_path: &syn::Path,
    method: &syn::ImplItemFn,
) -> Option<syn::ImplItemFn> {
    let sig = method.sig.clone();
    let method_ident = &sig.ident;
    let args = forwarding_call_args(&sig)?;

    Some(syn::parse_quote! {
        #sig {
            #trait_path::#method_ident(#(#args),*)
        }
    })
}

fn has_matching_impl(
    items: &[syn::Item],
    trait_path: &syn::Path,
    self_ty: ImplSelfType<'_>,
) -> bool {
    items.iter().any(|item| {
        let syn::Item::Impl(item_impl) = item else {
            return false;
        };
        let Some((_, existing_path, _)) = &item_impl.trait_ else {
            return false;
        };
        paths_match(existing_path, trait_path)
            && super::syn_helpers::type_matches_impl_self(&item_impl.self_ty, self_ty)
    })
}

fn paths_match(left: &syn::Path, right: &syn::Path) -> bool {
    left.leading_colon.is_some() == right.leading_colon.is_some()
        && left.segments.len() == right.segments.len()
        && left
            .segments
            .iter()
            .zip(&right.segments)
            .all(|(left, right)| {
                left.ident == right.ident
                    && matches!(
                        (&left.arguments, &right.arguments),
                        (syn::PathArguments::None, syn::PathArguments::None)
                    )
            })
}

fn path_key(path: &syn::Path) -> Vec<String> {
    path.segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect()
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
