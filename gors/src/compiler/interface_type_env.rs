use super::{import_context, noop_interfaces, synthetic_names, typeinfer};
use crate::generated_names::{
    as_any_method_ident, clone_box_method_ident, interface_key_method_ident,
};
use syn::Token;

pub(super) fn rust_path_name_candidates(name: &str) -> Vec<String> {
    let mut candidates = vec![name.to_string()];
    if let Some((module, symbol)) = name.rsplit_once('.') {
        candidates.extend(
            import_context::local_names_for_rust_module(module)
                .into_iter()
                .map(|local_name| format!("{local_name}.{symbol}")),
        );
        if let Some(package_name) = module.rsplit("__").next() {
            candidates.push(format!("{package_name}.{symbol}"));
        }
        candidates.push(format!("{}.{symbol}", module.replace("__", "/")));
    }
    candidates.dedup();
    candidates
}

pub(super) fn noop_impl_items_for_interface_name(interface_name: &str) -> Vec<syn::ImplItem> {
    super::TYPE_ENV.with(|env| {
        let env = env.borrow();
        let Some(interface_env_name) = resolve_interface_env_name(interface_name, &env) else {
            return Vec::new();
        };
        let Some(method_names) = env.get_interface_direct_methods(&interface_env_name) else {
            return Vec::new();
        };
        let trait_path = super::interface_trait_path_from_name(interface_name);
        let as_any = as_any_method_ident();
        let interface_key = interface_key_method_ident();
        let clone_box = clone_box_method_ident();
        let mut items = vec![
            noop_interfaces::impl_item_for_signature(syn::parse_quote! {
                fn #as_any(&self) -> Option<&dyn std::any::Any>
            }),
            noop_interfaces::impl_item_for_signature(syn::parse_quote! {
                fn #interface_key(&self) -> crate::builtin::GorsInterfaceKey
            }),
            noop_interfaces::impl_item_for_signature(syn::parse_quote! {
                fn #clone_box(&self) -> Box<dyn #trait_path>
            }),
        ];
        items.extend(method_names.iter().map(|method_name| {
            noop_interfaces::impl_item_for_signature(interface_method_signature_from_type_env(
                &interface_env_name,
                method_name,
                &env,
            ))
        }));
        items
    })
}

fn resolve_interface_env_name(name: &str, env: &typeinfer::TypeEnv) -> Option<String> {
    rust_path_name_candidates(name)
        .into_iter()
        .find(|candidate| env.is_interface(candidate))
}

fn return_type_from_go_results(results: &[typeinfer::GoType]) -> syn::ReturnType {
    match results {
        [] => syn::ReturnType::Default,
        [single] => {
            let ty = super::rust_type_preserving_named_go_type(single);
            syn::parse_quote! { -> #ty }
        }
        many => {
            let tys = many
                .iter()
                .map(super::rust_type_preserving_named_go_type)
                .collect::<Vec<_>>();
            syn::parse_quote! { -> (#(#tys),*) }
        }
    }
}

pub(super) fn interface_method_signature_from_type_env(
    interface_name: &str,
    method_name: &str,
    env: &typeinfer::TypeEnv,
) -> syn::Signature {
    let method_ident = syn::Ident::new(
        &super::rust_safe_ident_name(method_name),
        proc_macro2::Span::mixed_site(),
    );
    let mut inputs = syn::punctuated::Punctuated::new();
    inputs.push(syn::FnArg::Receiver(syn::Receiver {
        attrs: vec![],
        reference: Some((<Token![&]>::default(), None)),
        mutability: Some(<Token![mut]>::default()),
        self_token: <Token![self]>::default(),
        colon_token: None,
        ty: Box::new(syn::parse_quote! { &mut Self }),
    }));
    for (idx, param) in env
        .get_method_params(interface_name, method_name)
        .into_iter()
        .enumerate()
    {
        let ident = synthetic_names::unnamed_arg_ident(idx);
        let ty = interface_param_type_from_go_type(interface_name, method_name, idx, &param, env);
        inputs.push(syn::FnArg::Typed(syn::PatType {
            attrs: vec![],
            pat: Box::new(syn::Pat::Ident(syn::PatIdent {
                attrs: vec![],
                by_ref: None,
                subpat: None,
                mutability: None,
                ident,
            })),
            colon_token: <Token![:]>::default(),
            ty: Box::new(ty),
        }));
    }
    syn::Signature {
        constness: None,
        asyncness: None,
        unsafety: None,
        abi: None,
        fn_token: <Token![fn]>::default(),
        ident: method_ident,
        generics: syn::Generics::default(),
        paren_token: syn::token::Paren::default(),
        inputs,
        variadic: None,
        output: return_type_from_go_results(&env.get_method_returns(interface_name, method_name)),
    }
}

fn interface_param_type_from_go_type(
    interface_name: &str,
    method_name: &str,
    index: usize,
    param: &typeinfer::GoType,
    env: &typeinfer::TypeEnv,
) -> syn::Type {
    let method_key = format!("{interface_name}.{method_name}");
    if env.func_param_needs_borrowed_slice(&method_key, index)
        && let typeinfer::GoType::Slice(elem) = env.resolve_alias(param)
    {
        let elem = super::rust_type_preserving_named_go_type(&elem);
        return syn::parse_quote! { &mut [#elem] };
    }
    super::rust_type_preserving_named_go_type(param)
}
