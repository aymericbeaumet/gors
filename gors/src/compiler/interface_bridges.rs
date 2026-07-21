use std::cell::RefCell;
use std::collections::BTreeSet;

use proc_macro2::Span;

use super::{interface_method_sets, interface_type_env, typeinfer};
use crate::generated_names::{
    as_any_method_ident, clone_box_method_ident, interface_key_method_ident,
};

thread_local! {
    static REQUIRED: RefCell<BTreeSet<(String, String)>> = const { RefCell::new(BTreeSet::new()) };
}

pub(super) fn clear_required() {
    REQUIRED.with(|required| required.borrow_mut().clear());
}

pub(super) fn borrowed_value_expr(
    source: syn::Expr,
    source_interface: &str,
    target_interface: &str,
) -> syn::Expr {
    let value = bridge_value_expr(source, source_interface, target_interface);
    syn::parse_quote! { &mut #value }
}

pub(super) fn owned_value_expr(
    source: syn::Expr,
    source_interface: &str,
    target_interface: &str,
) -> syn::Expr {
    let value = bridge_value_expr(source, source_interface, target_interface);
    let target_trait = super::interface_trait_path_from_name(target_interface);
    syn::parse_quote! {
        Box::new(#value) as Box<dyn #target_trait>
    }
}

fn bridge_value_expr(
    source: syn::Expr,
    source_interface: &str,
    target_interface: &str,
) -> syn::Expr {
    let bridge = record_required(source_interface, target_interface);
    let source_trait = super::interface_trait_path_from_name(source_interface);
    let clone_box = clone_box_method_ident();
    syn::parse_quote! {
        #bridge(#source_trait::#clone_box(&*(#source)))
    }
}

pub(super) fn required_items() -> Vec<syn::Item> {
    let required = REQUIRED.with(|required| required.borrow().iter().cloned().collect::<Vec<_>>());
    if required.is_empty() {
        return Vec::new();
    }

    super::TYPE_ENV.with(|env| {
        let env = env.borrow();
        required
            .into_iter()
            .flat_map(|(source_interface, target_interface)| {
                bridge_items(&source_interface, &target_interface, &env)
            })
            .collect()
    })
}

fn record_required(source_interface: &str, target_interface: &str) -> syn::Ident {
    REQUIRED.with(|required| {
        required
            .borrow_mut()
            .insert((source_interface.to_string(), target_interface.to_string()));
    });
    bridge_ident(source_interface, target_interface)
}

fn bridge_items(
    source_interface: &str,
    target_interface: &str,
    env: &typeinfer::TypeEnv,
) -> Vec<syn::Item> {
    let bridge = bridge_ident(source_interface, target_interface);
    let source_trait = super::interface_trait_path_from_name(source_interface);
    let mut attrs = Vec::new();
    super::generated_attrs::preserve_for_dce(&mut attrs);
    let struct_item: syn::Item = syn::parse_quote! {
        #(#attrs)*
        struct #bridge(Box<dyn #source_trait>);
    };

    let target_method_set = interface_method_sets::for_impl(target_interface, &[]);
    let mut implemented_interfaces = target_method_set.embedded_interfaces;
    implemented_interfaces.push(target_interface.to_string());
    implemented_interfaces.sort();
    implemented_interfaces.dedup();

    let mut items = vec![struct_item];
    items.extend(
        implemented_interfaces.into_iter().map(|interface_name| {
            bridge_impl_item(&bridge, source_interface, &interface_name, env)
        }),
    );
    items
}

fn bridge_impl_item(
    bridge: &syn::Ident,
    source_interface: &str,
    target_interface: &str,
    env: &typeinfer::TypeEnv,
) -> syn::Item {
    let source_trait = super::interface_trait_path_from_name(source_interface);
    let target_trait = super::interface_trait_path_from_name(target_interface);
    let as_any = as_any_method_ident();
    let interface_key = interface_key_method_ident();
    let clone_box = clone_box_method_ident();
    let method_set = interface_method_sets::for_impl(target_interface, &[]);

    let mut impl_items: Vec<syn::ImplItem> = vec![
        syn::parse_quote! {
            fn #as_any(&self) -> Option<&dyn std::any::Any> {
                #source_trait::#as_any(&*self.0)
            }
        },
        syn::parse_quote! {
            fn #interface_key(&self) -> crate::builtin::GorsInterfaceKey {
                #source_trait::#interface_key(&*self.0)
            }
        },
        syn::parse_quote! {
            fn #clone_box(&self) -> Box<dyn #target_trait> {
                Box::new(Self(#source_trait::#clone_box(&*self.0))) as Box<dyn #target_trait>
            }
        },
    ];

    impl_items.extend(method_set.direct_methods.iter().map(|method_name| {
        let sig = interface_type_env::interface_method_signature_from_type_env(
            target_interface,
            method_name,
            env,
        );
        let method = sig.ident.clone();
        let args = super::signature_arg_idents(&sig);
        let block: syn::Block = if matches!(sig.output, syn::ReturnType::Default) {
            syn::parse_quote!({ self.0.#method(#(#args),*); })
        } else {
            syn::parse_quote!({ self.0.#method(#(#args),*) })
        };
        syn::ImplItem::Fn(syn::ImplItemFn {
            attrs: Vec::new(),
            vis: syn::Visibility::Inherited,
            defaultness: None,
            sig,
            block,
        })
    }));

    let mut attrs = Vec::new();
    super::generated_attrs::preserve_for_dce(&mut attrs);
    syn::Item::Impl(syn::ItemImpl {
        attrs,
        defaultness: None,
        unsafety: None,
        impl_token: <syn::Token![impl]>::default(),
        generics: syn::Generics::default(),
        trait_: Some((None, target_trait, <syn::Token![for]>::default())),
        self_ty: Box::new(syn::parse_quote! { #bridge }),
        brace_token: syn::token::Brace::default(),
        items: impl_items,
    })
}

fn bridge_ident(source_interface: &str, target_interface: &str) -> syn::Ident {
    let source = encode_ident_component(source_interface);
    let target = encode_ident_component(target_interface);
    syn::Ident::new(
        &format!("__GorsInterfaceBridge_{source}_to_{target}"),
        Span::mixed_site(),
    )
}

fn encode_ident_component(name: &str) -> String {
    let mut encoded = String::new();
    for byte in name.bytes() {
        if byte.is_ascii_alphanumeric() {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("_{byte:02x}_"));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    #[test]
    fn bridge_names_encode_interface_paths_without_collisions() {
        let dotted = super::bridge_ident("io.fs.File", "io.Reader").to_string();
        let underscored = super::bridge_ident("io_2e_fs.File", "io.Reader").to_string();

        assert_ne!(dotted, underscored);
        assert!(dotted.starts_with("__GorsInterfaceBridge_"));
    }
}
