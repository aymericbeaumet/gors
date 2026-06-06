use super::{EmbeddedInterfaceField, interface_hooks, syn_inspect, synthetic_names};
use proc_macro2::Span;
use std::collections::{BTreeMap, BTreeSet};
use syn::Token;

pub(super) fn impls(
    items: &[syn::Item],
    methods: &BTreeMap<String, Vec<syn::ImplItemFn>>,
    pointer_methods: &BTreeMap<String, BTreeSet<String>>,
    embedded_structs: BTreeMap<String, Vec<EmbeddedInterfaceField>>,
) -> Vec<syn::Item> {
    let trait_methods = syn_inspect::trait_method_fns(items);
    let mut out = vec![];

    for (struct_name, fields) in embedded_structs {
        let struct_ident = syn::Ident::new(&struct_name, Span::mixed_site());
        for field in fields {
            let Some(trait_ident) = trait_ident_for_field(&field) else {
                continue;
            };
            let Some(required_methods) = trait_methods.get(&trait_ident.to_string()) else {
                continue;
            };
            let struct_methods = methods
                .get(&struct_name)
                .map(Vec::as_slice)
                .unwrap_or_default();
            let struct_pointer_methods = pointer_methods.get(&struct_name);
            let mut impl_items = vec![];
            for trait_fn in required_methods {
                let method_name = trait_fn.sig.ident.to_string();
                if let Some(method) =
                    inherited_method_impl_item(struct_methods, struct_pointer_methods, &method_name)
                {
                    impl_items.push(method);
                } else {
                    impl_items.push(field_forwarding_impl_item(trait_fn, &field.field_ident));
                }
            }
            if impl_items.is_empty() {
                continue;
            }
            let trait_path = field.trait_path.clone();
            let generics = synthetic_names::borrowed_interface_generics();
            let lifetime = synthetic_names::borrowed_interface_lifetime();
            out.push(syn::Item::Impl(syn::ItemImpl {
                attrs: vec![],
                defaultness: None,
                unsafety: None,
                impl_token: <Token![impl]>::default(),
                generics,
                trait_: Some((None, trait_path, <Token![for]>::default())),
                self_ty: Box::new(syn::parse_quote! { #struct_ident<#lifetime> }),
                brace_token: syn::token::Brace::default(),
                items: impl_items,
            }));

            let mut pointer_impl_items = vec![];
            for trait_fn in required_methods {
                pointer_impl_items.push(pointer_forwarding_impl_item(
                    trait_fn,
                    &field.trait_path,
                    &struct_ident,
                    &field.field_ident,
                    struct_methods,
                    struct_pointer_methods,
                ));
            }
            if !pointer_impl_items.is_empty() {
                let trait_path = field.trait_path.clone();
                let lifetime = synthetic_names::borrowed_interface_lifetime();
                let generics = synthetic_names::borrowed_interface_generics();
                out.push(syn::parse_quote! {
                    impl #generics #trait_path for crate::builtin::GorsPtr<#struct_ident<#lifetime>> {
                        #(#pointer_impl_items)*
                    }
                });
            }
        }
    }

    out
}

fn trait_ident_for_field(field: &EmbeddedInterfaceField) -> Option<&syn::Ident> {
    field
        .trait_path
        .segments
        .last()
        .map(|segment| &segment.ident)
}

fn inherited_method_impl_item(
    methods: &[syn::ImplItemFn],
    pointer_methods: Option<&BTreeSet<String>>,
    method_name: &str,
) -> Option<syn::ImplItem> {
    if pointer_methods.is_some_and(|methods| methods.contains(method_name)) {
        return None;
    }
    let mut method = methods
        .iter()
        .find(|method| method.sig.ident == method_name)?
        .clone();
    method.vis = syn::Visibility::Inherited;
    set_receiver_to_mut_self(&mut method.sig);
    Some(syn::ImplItem::Fn(method))
}

fn field_forwarding_impl_item(
    trait_fn: &syn::TraitItemFn,
    field_ident: &syn::Ident,
) -> syn::ImplItem {
    let mut sig = trait_fn.sig.clone();
    set_receiver_for_trait_forwarding(&mut sig);
    let method_ident = sig.ident.clone();
    let arg_idents = super::signature_arg_idents(&sig);
    let block = if matches!(sig.output, syn::ReturnType::Default) {
        syn::parse_quote!({ self.#field_ident.#method_ident(#(#arg_idents),*); })
    } else {
        syn::parse_quote!({ self.#field_ident.#method_ident(#(#arg_idents),*) })
    };
    impl_item_fn(sig, block)
}

fn pointer_forwarding_impl_item(
    trait_fn: &syn::TraitItemFn,
    trait_path: &syn::Path,
    struct_ident: &syn::Ident,
    field_ident: &syn::Ident,
    methods: &[syn::ImplItemFn],
    pointer_methods: Option<&BTreeSet<String>>,
) -> syn::ImplItem {
    let mut sig = trait_fn.sig.clone();
    set_receiver_for_trait_forwarding(&mut sig);
    let method_ident = sig.ident.clone();
    let method_name = method_ident.to_string();
    let arg_idents = super::signature_arg_idents(&sig);
    let has_inherent_method = methods
        .iter()
        .any(|method| method.sig.ident == method_ident);
    let has_pointer_inherent_method =
        pointer_methods.is_some_and(|methods| methods.contains(&method_name));
    let block = match method_name.as_str() {
        interface_hooks::AS_ANY_METHOD => syn::parse_quote!({ None }),
        interface_hooks::CLONE_BOX_METHOD => {
            syn::parse_quote!({ Box::new(self.clone()) as Box<dyn #trait_path> })
        }
        _ if has_pointer_inherent_method && matches!(sig.output, syn::ReturnType::Default) => {
            syn::parse_quote!({
                #struct_ident::#method_ident(self.clone(), #(#arg_idents),*);
            })
        }
        _ if has_pointer_inherent_method => {
            syn::parse_quote!({
                #struct_ident::#method_ident(self.clone(), #(#arg_idents),*)
            })
        }
        _ if has_inherent_method && matches!(sig.output, syn::ReturnType::Default) => {
            syn::parse_quote!({
                let mut __gors_guard = self.lock().unwrap();
                #struct_ident::#method_ident(&mut *__gors_guard, #(#arg_idents),*);
            })
        }
        _ if has_inherent_method => {
            syn::parse_quote!({
                let mut __gors_guard = self.lock().unwrap();
                #struct_ident::#method_ident(&mut *__gors_guard, #(#arg_idents),*)
            })
        }
        _ if matches!(sig.output, syn::ReturnType::Default) => {
            syn::parse_quote!({
                let mut __gors_guard = self.lock().unwrap();
                __gors_guard.#field_ident.#method_ident(#(#arg_idents),*);
            })
        }
        _ => {
            syn::parse_quote!({
                let mut __gors_guard = self.lock().unwrap();
                __gors_guard.#field_ident.#method_ident(#(#arg_idents),*)
            })
        }
    };
    impl_item_fn(sig, block)
}

fn set_receiver_to_mut_self(sig: &mut syn::Signature) {
    if let Some(syn::FnArg::Receiver(receiver)) = sig.inputs.first_mut() {
        receiver.mutability = Some(<Token![mut]>::default());
        *receiver.ty = syn::parse_quote! { &mut Self };
    }
}

fn set_receiver_for_trait_forwarding(sig: &mut syn::Signature) {
    if interface_hooks::is_runtime_hook(&sig.ident.to_string()) {
        set_receiver_to_self(sig);
    } else {
        set_receiver_to_mut_self(sig);
    }
}

fn set_receiver_to_self(sig: &mut syn::Signature) {
    if let Some(syn::FnArg::Receiver(receiver)) = sig.inputs.first_mut() {
        receiver.mutability = None;
        *receiver.ty = syn::parse_quote! { &Self };
    }
}

fn impl_item_fn(sig: syn::Signature, block: syn::Block) -> syn::ImplItem {
    syn::ImplItem::Fn(syn::ImplItemFn {
        attrs: vec![],
        vis: syn::Visibility::Inherited,
        defaultness: None,
        sig,
        block,
    })
}
