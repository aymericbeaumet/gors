use super::{interface_hooks, signature_arg_idents};
use proc_macro2::Span;
use std::collections::{BTreeMap, BTreeSet};
use syn::Token;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PointerCallReceiver {
    ImmutableRef,
    MutableRef,
    Owned,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PointerImplTarget {
    GorsPtr,
    BorrowedMut,
}

struct PointerMethodCall<'a> {
    sig: &'a syn::Signature,
    immutable_error_method: bool,
    receiver_kind: PointerCallReceiver,
    struct_ident: &'a syn::Ident,
    method_ident: &'a syn::Ident,
    call_receiver: &'a syn::Expr,
    arg_idents: &'a [syn::Ident],
}

pub(super) fn call_receiver(
    method: &syn::ImplItemFn,
    method_is_pointer_receiver: bool,
) -> PointerCallReceiver {
    match method.sig.inputs.first() {
        Some(syn::FnArg::Receiver(receiver)) if receiver.reference.is_some() => {
            if receiver.mutability.is_some() {
                PointerCallReceiver::MutableRef
            } else {
                PointerCallReceiver::ImmutableRef
            }
        }
        Some(syn::FnArg::Receiver(_)) => PointerCallReceiver::Owned,
        _ if method_is_pointer_receiver => PointerCallReceiver::MutableRef,
        _ => PointerCallReceiver::Owned,
    }
}

pub(super) fn pointer_items(
    trait_name: &str,
    struct_name: &str,
    trait_path: &syn::Path,
    method_names: &[String],
    methods: &BTreeMap<String, Vec<syn::ImplItemFn>>,
    pointer_methods: Option<&BTreeSet<String>>,
) -> Vec<syn::ImplItem> {
    items_for_target(
        PointerImplTarget::GorsPtr,
        trait_name,
        struct_name,
        trait_path,
        method_names,
        methods,
        pointer_methods,
    )
}

pub(super) fn borrowed_pointer_items(
    trait_name: &str,
    struct_name: &str,
    trait_path: &syn::Path,
    method_names: &[String],
    methods: &BTreeMap<String, Vec<syn::ImplItemFn>>,
    pointer_methods: Option<&BTreeSet<String>>,
) -> Vec<syn::ImplItem> {
    items_for_target(
        PointerImplTarget::BorrowedMut,
        trait_name,
        struct_name,
        trait_path,
        method_names,
        methods,
        pointer_methods,
    )
}

pub(super) fn borrowed_pointer_can_delegate(
    trait_name: &str,
    method_names: &[String],
    pointer_methods: Option<&BTreeSet<String>>,
) -> bool {
    method_names.iter().all(|method_name| {
        !pointer_methods.is_some_and(|methods| methods.contains(method_name))
            || mutable_borrowed_delegate_is_valid(trait_name, method_name)
    })
}

fn items_for_target(
    target: PointerImplTarget,
    trait_name: &str,
    struct_name: &str,
    trait_path: &syn::Path,
    method_names: &[String],
    methods: &BTreeMap<String, Vec<syn::ImplItemFn>>,
    pointer_methods: Option<&BTreeSet<String>>,
) -> Vec<syn::ImplItem> {
    let mut impl_items = vec![target.as_any_item()];
    if trait_name != "error" {
        impl_items.push(interface_hooks::clone_box_impl_item(
            trait_path,
            target.can_clone_self(),
        ));
    }
    let Some(method_list) = methods.get(struct_name) else {
        return impl_items;
    };
    for method in method_list {
        if method_names.contains(&method.sig.ident.to_string()) {
            impl_items.push(target.method_item(trait_name, struct_name, method, pointer_methods));
        }
    }
    impl_items
}

impl PointerImplTarget {
    fn as_any_item(self) -> syn::ImplItem {
        match self {
            Self::GorsPtr => syn::parse_quote! {
                fn __gors_as_any(&self) -> Option<&dyn std::any::Any> {
                    Some(self)
                }
            },
            Self::BorrowedMut => syn::parse_quote! {
                fn __gors_as_any(&self) -> Option<&dyn std::any::Any> {
                    None
                }
            },
        }
    }

    fn can_clone_self(self) -> bool {
        matches!(self, Self::GorsPtr)
    }

    fn method_item(
        self,
        trait_name: &str,
        struct_name: &str,
        method: &syn::ImplItemFn,
        pointer_methods: Option<&BTreeSet<String>>,
    ) -> syn::ImplItem {
        let method_ident = method.sig.ident.clone();
        let method_is_pointer_receiver =
            pointer_methods.is_some_and(|methods| methods.contains(&method_ident.to_string()));
        let call_receiver_kind = call_receiver(method, method_is_pointer_receiver);
        let struct_ident = syn::Ident::new(struct_name, Span::mixed_site());
        let arg_idents = signature_arg_idents(&method.sig);
        let mut sig = method.sig.clone();
        let immutable_error_method = trait_name == "error" && method_ident == "Error";
        set_interface_receiver(&mut sig, immutable_error_method);
        let call_receiver = self.call_receiver_expr(call_receiver_kind);
        let call = PointerMethodCall {
            sig: &sig,
            immutable_error_method,
            receiver_kind: call_receiver_kind,
            struct_ident: &struct_ident,
            method_ident: &method_ident,
            call_receiver: &call_receiver,
            arg_idents: &arg_idents,
        };
        let block = self.method_block(call);
        impl_item_fn(sig, block)
    }

    fn call_receiver_expr(self, kind: PointerCallReceiver) -> syn::Expr {
        match (self, kind) {
            (Self::GorsPtr, PointerCallReceiver::ImmutableRef) => {
                syn::parse_quote! { &*__gors_guard }
            }
            (Self::GorsPtr, PointerCallReceiver::MutableRef) => {
                syn::parse_quote! { &mut *__gors_guard }
            }
            (Self::GorsPtr, PointerCallReceiver::Owned) => {
                syn::parse_quote! { (*__gors_guard).clone() }
            }
            (Self::BorrowedMut, PointerCallReceiver::ImmutableRef) => {
                syn::parse_quote! { &**self }
            }
            (Self::BorrowedMut, PointerCallReceiver::MutableRef) => {
                syn::parse_quote! { &mut **self }
            }
            (Self::BorrowedMut, PointerCallReceiver::Owned) => {
                syn::parse_quote! { (**self).clone() }
            }
        }
    }

    fn method_block(self, call: PointerMethodCall<'_>) -> syn::Block {
        if self == Self::BorrowedMut
            && call.immutable_error_method
            && matches!(call.receiver_kind, PointerCallReceiver::MutableRef)
        {
            let struct_ident = call.struct_ident;
            let method_ident = call.method_ident;
            let arg_idents = call.arg_idents;
            return if matches!(call.sig.output, syn::ReturnType::Default) {
                syn::parse_quote!({
                    let mut __gors_receiver = (**self).clone();
                    #struct_ident::#method_ident(&mut __gors_receiver, #(#arg_idents),*);
                })
            } else {
                syn::parse_quote!({
                    let mut __gors_receiver = (**self).clone();
                    #struct_ident::#method_ident(&mut __gors_receiver, #(#arg_idents),*)
                })
            };
        }

        let struct_ident = call.struct_ident;
        let method_ident = call.method_ident;
        let call_receiver = call.call_receiver;
        let arg_idents = call.arg_idents;
        match self {
            Self::GorsPtr if matches!(call.sig.output, syn::ReturnType::Default) => {
                syn::parse_quote!({
                    let mut __gors_guard = self.lock().unwrap();
                    #struct_ident::#method_ident(#call_receiver, #(#arg_idents),*);
                })
            }
            Self::GorsPtr => {
                syn::parse_quote!({
                    let mut __gors_guard = self.lock().unwrap();
                    #struct_ident::#method_ident(#call_receiver, #(#arg_idents),*)
                })
            }
            Self::BorrowedMut if matches!(call.sig.output, syn::ReturnType::Default) => {
                syn::parse_quote!({
                    #struct_ident::#method_ident(#call_receiver, #(#arg_idents),*);
                })
            }
            Self::BorrowedMut => {
                syn::parse_quote!({
                    #struct_ident::#method_ident(#call_receiver, #(#arg_idents),*)
                })
            }
        }
    }
}

fn set_interface_receiver(sig: &mut syn::Signature, immutable_error_method: bool) {
    if let Some(syn::FnArg::Receiver(receiver)) = sig.inputs.first_mut() {
        if immutable_error_method {
            receiver.mutability = None;
            *receiver.ty = syn::parse_quote! { &Self };
        } else {
            receiver.mutability = Some(<Token![mut]>::default());
            *receiver.ty = syn::parse_quote! { &mut Self };
        }
    }
}

fn mutable_borrowed_delegate_is_valid(trait_name: &str, method_name: &str) -> bool {
    trait_name != "error" || method_name != "Error"
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
