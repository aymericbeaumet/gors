use super::{
    EmbeddedInterfaceField, interface_hooks, interface_type_env, syn_inspect, synthetic_names,
};
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
            let imported_methods = super::TYPE_ENV.with(|env| {
                interface_type_env::trait_method_fns_for_path_from_type_env(
                    &field.trait_path,
                    &env.borrow(),
                )
            });
            let required_methods = if trait_path_is_local_unqualified(&field.trait_path) {
                trait_methods
                    .get(&trait_ident.to_string())
                    .map(Vec::as_slice)
                    .or(imported_methods.as_deref())
            } else {
                imported_methods.as_deref()
            };
            let Some(required_methods) = required_methods else {
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
                    impl_items.push(field_forwarding_impl_item(
                        trait_fn,
                        &field.trait_path,
                        &field.field_ident,
                    ));
                }
            }
            if impl_items.is_empty() {
                continue;
            }
            let trait_path = field.trait_path.clone();
            let generics = synthetic_names::borrowed_interface_generics();
            let lifetime = synthetic_names::borrowed_interface_lifetime();
            let mut attrs = vec![];
            super::generated_attrs::preserve_for_dce(&mut attrs);
            let value_impl = syn::Item::Impl(syn::ItemImpl {
                attrs,
                defaultness: None,
                unsafety: None,
                impl_token: <Token![impl]>::default(),
                generics,
                trait_: Some((None, trait_path, <Token![for]>::default())),
                self_ty: Box::new(syn::parse_quote! { #struct_ident<#lifetime> }),
                brace_token: syn::token::Brace::default(),
                items: impl_items,
            });
            push_if_target_is_new(items, &mut out, value_impl);

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
                let mut item_impl: syn::ItemImpl = syn::parse_quote! {
                    impl #generics #trait_path for crate::builtin::GorsPtr<#struct_ident<#lifetime>> {
                        #(#pointer_impl_items)*
                    }
                };
                super::generated_attrs::preserve_for_dce(&mut item_impl.attrs);
                push_if_target_is_new(items, &mut out, syn::Item::Impl(item_impl));
            }
        }
    }

    out
}

fn push_if_target_is_new(existing: &[syn::Item], out: &mut Vec<syn::Item>, candidate: syn::Item) {
    if existing
        .iter()
        .chain(out.iter())
        .any(|item| syn_inspect::impl_trait_targets_match(item, &candidate))
    {
        return;
    }
    out.push(candidate);
}

fn trait_ident_for_field(field: &EmbeddedInterfaceField) -> Option<&syn::Ident> {
    field
        .trait_path
        .segments
        .last()
        .map(|segment| &segment.ident)
}

fn trait_path_is_local_unqualified(path: &syn::Path) -> bool {
    path.leading_colon.is_none() && path.segments.len() == 1
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
    trait_path: &syn::Path,
    field_ident: &syn::Ident,
) -> syn::ImplItem {
    let mut sig = trait_fn.sig.clone();
    set_receiver_for_trait_forwarding(&mut sig);
    let arg_idents = canonical_forwarding_arg_idents(&mut sig);
    let method_ident = sig.ident.clone();
    let block = match method_ident.to_string().as_str() {
        interface_hooks::AS_ANY_METHOD => syn::parse_quote!({ Some(self) }),
        interface_hooks::CLONE_BOX_METHOD => {
            syn::parse_quote!({ Box::new(self.clone()) as Box<dyn #trait_path> })
        }
        interface_hooks::INTERFACE_KEY_METHOD => {
            syn::parse_quote!({ crate::builtin::GorsInterfaceKey::non_comparable::<Self>() })
        }
        _ if matches!(sig.output, syn::ReturnType::Default) => {
            syn::parse_quote!({ self.#field_ident.#method_ident(#(#arg_idents),*); })
        }
        _ => syn::parse_quote!({ self.#field_ident.#method_ident(#(#arg_idents),*) }),
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
    let arg_idents = canonical_forwarding_arg_idents(&mut sig);
    let method_ident = sig.ident.clone();
    let method_name = method_ident.to_string();
    let has_inherent_method = methods
        .iter()
        .any(|method| method.sig.ident == method_ident);
    let has_pointer_inherent_method =
        pointer_methods.is_some_and(|methods| methods.contains(&method_name));
    let block = match method_name.as_str() {
        interface_hooks::AS_ANY_METHOD => syn::parse_quote!({ Some(self) }),
        interface_hooks::CLONE_BOX_METHOD => {
            syn::parse_quote!({ Box::new(self.clone()) as Box<dyn #trait_path> })
        }
        interface_hooks::INTERFACE_KEY_METHOD => syn::parse_quote!({ self.interface_key() }),
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

/// Give compiler-generated embedded-interface forwarders one package-wide
/// argument spelling. Different Go files can see the same trait through its
/// source declaration or through serialized type facts; normalizing the
/// otherwise irrelevant parameter bindings lets strict structural impl dedup
/// recognize those forwarders without hiding genuinely different bodies.
fn canonical_forwarding_arg_idents(sig: &mut syn::Signature) -> Vec<syn::Ident> {
    let mut arg_idents = Vec::new();
    for input in &mut sig.inputs {
        let syn::FnArg::Typed(arg) = input else {
            continue;
        };
        let ident = synthetic_names::unnamed_arg_ident(arg_idents.len());
        *arg.pat = syn::parse_quote! { #ident };
        arg_idents.push(ident);
    }
    arg_idents
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_embedded_impls_are_preserved_but_never_marked_as_fallbacks() {
        let trait_items = vec![syn::parse_quote! {
            trait Reader {
                fn Read(&mut self);
            }
        }];
        let embedded_structs = BTreeMap::from([(
            "Wrapper".to_string(),
            vec![EmbeddedInterfaceField {
                field_ident: syn::parse_quote!(Reader),
                trait_path: syn::parse_quote!(Reader),
            }],
        )]);

        let generated = impls(
            &trait_items,
            &BTreeMap::new(),
            &BTreeMap::new(),
            embedded_structs,
        );
        let impl_attrs = generated
            .iter()
            .filter_map(|item| match item {
                syn::Item::Impl(item_impl) => Some(item_impl.attrs.as_slice()),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(impl_attrs.len(), 2);
        assert!(impl_attrs.iter().all(|attrs| {
            super::super::generated_attrs::attrs_preserve_for_dce(attrs)
                && !super::super::generated_attrs::attrs_mark_removable_interface_fallback(attrs)
        }));
    }

    #[test]
    fn embedded_impls_do_not_reemit_an_existing_canonical_target() {
        let items = vec![
            syn::parse_quote! {
                trait Reader {
                    fn Read(&mut self);
                }
            },
            syn::parse_quote! {
                impl<'__gors> Reader for Wrapper<'__gors> {
                    fn Read(&mut self) {}
                }
            },
            syn::parse_quote! {
                impl<'__gors> Reader for crate::builtin::GorsPtr<Wrapper<'__gors>> {
                    fn Read(&mut self) {}
                }
            },
        ];
        let embedded_structs = BTreeMap::from([(
            "Wrapper".to_string(),
            vec![EmbeddedInterfaceField {
                field_ident: syn::parse_quote!(Reader),
                trait_path: syn::parse_quote!(Reader),
            }],
        )]);

        let generated = impls(&items, &BTreeMap::new(), &BTreeMap::new(), embedded_structs);

        assert!(generated.is_empty());
    }

    #[test]
    fn qualified_embedded_trait_uses_exact_type_env_methods_over_local_basename() {
        let items = vec![syn::parse_quote! {
            trait Reader {
                fn Local(&mut self);
            }
        }];
        let mut env = super::super::typeinfer::TypeEnv::new();
        env.set_type_kind("io.Reader", super::super::typeinfer::TypeKind::Interface);
        env.set_interface_methods("io.Reader", vec!["Read".to_string()]);
        super::super::set_type_env(env);
        let embedded_structs = BTreeMap::from([(
            "Wrapper".to_string(),
            vec![EmbeddedInterfaceField {
                field_ident: syn::parse_quote!(Reader),
                trait_path: syn::parse_quote!(crate::io::Reader),
            }],
        )]);

        let generated = impls(&items, &BTreeMap::new(), &BTreeMap::new(), embedded_structs);
        super::super::set_type_env(super::super::typeinfer::TypeEnv::new());
        let method_names = generated
            .iter()
            .filter_map(|item| match item {
                syn::Item::Impl(item_impl) => Some(item_impl),
                _ => None,
            })
            .flat_map(|item_impl| &item_impl.items)
            .filter_map(|item| match item {
                syn::ImplItem::Fn(method) => Some(method.sig.ident.to_string()),
                _ => None,
            })
            .collect::<BTreeSet<_>>();

        assert!(method_names.contains("Read"), "{method_names:?}");
        assert!(!method_names.contains("Local"), "{method_names:?}");
    }

    #[test]
    fn embedded_value_runtime_hooks_keep_the_wrapper_dynamic_identity() {
        let items = vec![syn::parse_quote! {
            trait Reader {
                fn Read(&mut self) -> i32;
                fn __gors_as_any(&self) -> Option<&dyn std::any::Any>;
                fn __gors_clone_box(&self) -> Box<dyn Reader>;
                fn __gors_interface_key(&self) -> crate::builtin::GorsInterfaceKey;
            }
        }];
        let embedded_structs = BTreeMap::from([(
            "Wrapper".to_string(),
            vec![EmbeddedInterfaceField {
                field_ident: syn::parse_quote!(Reader),
                trait_path: syn::parse_quote!(Reader),
            }],
        )]);

        let generated = impls(&items, &BTreeMap::new(), &BTreeMap::new(), embedded_structs);
        let value_impl = generated
            .iter()
            .find_map(|item| match item {
                syn::Item::Impl(item_impl)
                    if !matches!(
                        item_impl.self_ty.as_ref(),
                        syn::Type::Path(path)
                            if path.path.segments.last().is_some_and(|segment| segment.ident == "GorsPtr")
                    ) => Some(item_impl),
                _ => None,
            })
            .expect("embedded value impl");
        let tokens = quote::quote! { #value_impl }.to_string();

        assert!(tokens.contains("Some (self)"), "{tokens}");
        assert!(
            tokens.contains("Box :: new (self . clone ()) as Box < dyn Reader >"),
            "{tokens}"
        );
        assert!(
            tokens.contains("GorsInterfaceKey :: non_comparable :: < Self >"),
            "{tokens}"
        );
        for hook in [
            interface_hooks::AS_ANY_METHOD,
            interface_hooks::CLONE_BOX_METHOD,
            interface_hooks::INTERFACE_KEY_METHOD,
        ] {
            assert!(
                !tokens.contains(&format!("self . Reader . {hook}")),
                "{tokens}"
            );
        }
        assert!(tokens.contains("self . Reader . Read"), "{tokens}");
    }
}
