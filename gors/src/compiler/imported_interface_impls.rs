use super::{interface_impls, interface_method_sets};
use proc_macro2::Span;
use std::collections::{BTreeMap, BTreeSet};

struct EmbeddedPointerImpl<'a> {
    name: &'a str,
    struct_name: &'a str,
    struct_method_list: &'a [String],
    pointer_methods: Option<&'a BTreeSet<String>>,
}

struct ImportedPointerEmitState<'a> {
    methods: &'a BTreeMap<String, Vec<syn::ImplItemFn>>,
    emitted_interface_impls: &'a mut BTreeSet<(String, String, bool)>,
    emitted_borrowed_pointer_interface_impls: &'a mut BTreeSet<(String, String)>,
    items: &'a mut Vec<syn::Item>,
}

pub(super) fn pointer_impls_for_local_structs(
    struct_methods: &BTreeMap<String, Vec<String>>,
    struct_pointer_methods: &BTreeMap<String, BTreeSet<String>>,
    methods: &BTreeMap<String, Vec<syn::ImplItemFn>>,
    method_generics: &BTreeMap<String, Vec<syn::Ident>>,
    emitted_interface_impls: &mut BTreeSet<(String, String, bool)>,
    emitted_borrowed_pointer_interface_impls: &mut BTreeSet<(String, String)>,
) -> Vec<syn::Item> {
    super::TYPE_ENV.with(|env| {
        let env = env.borrow();
        let mut items = Vec::new();
        for interface_name in env.interface_names() {
            if !interface_name.contains('.') {
                continue;
            }
            if !has_direct_import_qualifier(&interface_name) {
                continue;
            }
            let Some(required_methods) = env.get_interface_methods(&interface_name) else {
                continue;
            };
            if required_methods.is_empty() {
                continue;
            }
            let method_set = interface_method_sets::for_impl(&interface_name, &required_methods);
            for (struct_name, struct_method_list) in struct_methods {
                if super::BORROWED_INTERFACE_STRUCTS
                    .with(|structs| structs.borrow().contains_key(struct_name))
                    || method_generics
                        .get(struct_name)
                        .is_some_and(|type_args| !type_args.is_empty())
                {
                    continue;
                }
                let pointer_satisfies = interface_method_sets::pointer_satisfies(
                    struct_method_list,
                    &method_set.required_methods,
                );
                if !pointer_satisfies {
                    continue;
                }
                let pointer_methods = struct_pointer_methods.get(struct_name);
                if emitted_interface_impls.insert((
                    interface_name.clone(),
                    struct_name.clone(),
                    true,
                )) {
                    let trait_path = super::interface_trait_path_from_name(&interface_name);
                    let struct_ident = syn::Ident::new(struct_name, Span::mixed_site());
                    let impl_items = interface_impls::pointer_items(
                        &interface_name,
                        struct_name,
                        &trait_path,
                        &method_set.direct_methods,
                        methods,
                        pointer_methods,
                    );
                    items.push(syn::parse_quote! {
                        impl #trait_path for crate::builtin::GorsPtr<#struct_ident> {
                            #(#impl_items)*
                        }
                    });
                }
                if interface_impls::borrowed_pointer_can_delegate(
                    &interface_name,
                    &method_set.direct_methods,
                    pointer_methods,
                ) && emitted_borrowed_pointer_interface_impls
                    .insert((interface_name.clone(), struct_name.clone()))
                {
                    let trait_path = super::interface_trait_path_from_name(&interface_name);
                    let struct_ident = syn::Ident::new(struct_name, Span::mixed_site());
                    let impl_items = interface_impls::borrowed_pointer_items(
                        &interface_name,
                        struct_name,
                        &trait_path,
                        &method_set.direct_methods,
                        methods,
                        pointer_methods,
                    );
                    items.push(syn::parse_quote! {
                        impl<'__gors> #trait_path for &'__gors mut #struct_ident {
                            #(#impl_items)*
                        }
                    });
                }

                for embedded_name in &method_set.embedded_interfaces {
                    let mut emit_state = ImportedPointerEmitState {
                        methods,
                        emitted_interface_impls,
                        emitted_borrowed_pointer_interface_impls,
                        items: &mut items,
                    };
                    push_embedded_pointer_impls(
                        EmbeddedPointerImpl {
                            name: embedded_name,
                            struct_name,
                            struct_method_list,
                            pointer_methods,
                        },
                        &mut emit_state,
                    );
                }
            }
        }
        items
    })
}

fn push_embedded_pointer_impls(
    embedded: EmbeddedPointerImpl<'_>,
    state: &mut ImportedPointerEmitState<'_>,
) {
    let embedded_method_set = interface_method_sets::for_impl(embedded.name, &[]);
    if !interface_method_sets::pointer_satisfies(
        embedded.struct_method_list,
        &embedded_method_set.required_methods,
    ) {
        return;
    }
    if !state.emitted_interface_impls.insert((
        embedded.name.to_string(),
        embedded.struct_name.to_string(),
        true,
    )) {
        return;
    }
    let trait_path = super::interface_trait_path_from_name(embedded.name);
    let struct_ident = syn::Ident::new(embedded.struct_name, Span::mixed_site());
    let impl_items = interface_impls::pointer_items(
        embedded.name,
        embedded.struct_name,
        &trait_path,
        &embedded_method_set.direct_methods,
        state.methods,
        embedded.pointer_methods,
    );
    state.items.push(syn::parse_quote! {
        impl #trait_path for crate::builtin::GorsPtr<#struct_ident> {
            #(#impl_items)*
        }
    });
    if interface_impls::borrowed_pointer_can_delegate(
        embedded.name,
        &embedded_method_set.direct_methods,
        embedded.pointer_methods,
    ) && state
        .emitted_borrowed_pointer_interface_impls
        .insert((embedded.name.to_string(), embedded.struct_name.to_string()))
    {
        let trait_path = super::interface_trait_path_from_name(embedded.name);
        let impl_items = interface_impls::borrowed_pointer_items(
            embedded.name,
            embedded.struct_name,
            &trait_path,
            &embedded_method_set.direct_methods,
            state.methods,
            embedded.pointer_methods,
        );
        state.items.push(syn::parse_quote! {
            impl<'__gors> #trait_path for &'__gors mut #struct_ident {
                #(#impl_items)*
            }
        });
    }
}

fn has_direct_import_qualifier(interface_name: &str) -> bool {
    let Some((qualifier, _)) = interface_name.split_once('.') else {
        return false;
    };
    super::IMPORT_NAMES.with(|names| names.borrow().contains(qualifier))
}
