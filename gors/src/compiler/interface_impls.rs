use super::{interface_hooks, signature_arg_idents, typeinfer};
use crate::generated_names::as_any_method_ident;
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

pub(super) fn concrete_can_emit_methods(
    struct_name: &str,
    method_names: &[String],
    methods: &BTreeMap<String, Vec<syn::ImplItemFn>>,
) -> bool {
    method_names
        .iter()
        .all(|method_name| concrete_can_emit_method(struct_name, method_name, methods))
}

pub(super) fn concrete_items(
    trait_name: &str,
    trait_path: &syn::Path,
    struct_name: &str,
    method_names: &[String],
    methods: &BTreeMap<String, Vec<syn::ImplItemFn>>,
    exposes_any: bool,
) -> Vec<syn::ImplItem> {
    let mut impl_items: Vec<syn::ImplItem> = vec![concrete_as_any_item(exposes_any)];
    if trait_name != "error" {
        let can_clone_self =
            super::NON_CLONE_STRUCTS.with(|structs| !structs.borrow().contains(struct_name));
        impl_items.push(interface_hooks::clone_box_impl_item(
            trait_path,
            can_clone_self,
        ));
    }
    if let Some(method_list) = methods.get(struct_name) {
        for method in method_list {
            if method_names.contains(&method.sig.ident.to_string()) {
                impl_items.push(concrete_direct_method_item(trait_name, struct_name, method));
            }
        }
    }
    let emitted_method_names = impl_items
        .iter()
        .filter_map(|item| match item {
            syn::ImplItem::Fn(func) => Some(func.sig.ident.to_string()),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    for method_name in method_names {
        if emitted_method_names.contains(method_name) {
            continue;
        }
        if let Some(item) = promoted_concrete_item(struct_name, method_name, methods) {
            impl_items.push(item);
        }
    }
    impl_items
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

#[derive(Clone)]
struct PromotedMethodStep {
    field_name: String,
    field_is_pointer: bool,
}

#[derive(Clone)]
struct PromotedMethodInfo {
    owner_type: String,
    steps: Vec<PromotedMethodStep>,
    method_is_pointer_receiver: bool,
}

fn direct_method_key_for_impl(
    env: &typeinfer::TypeEnv,
    type_name: &str,
    method_name: &str,
    include_pointer_receiver_methods: bool,
) -> Option<String> {
    let method_key = format!("{type_name}.{method_name}");
    let has_method = if include_pointer_receiver_methods {
        env.has_func(&method_key)
    } else {
        env.has_value_method(&method_key)
    };
    has_method.then_some(method_key)
}

fn promoted_method_info(
    struct_name: &str,
    method_name: &str,
    include_pointer_receiver_methods: bool,
) -> Option<PromotedMethodInfo> {
    super::TYPE_ENV.with(|env| {
        let env = env.borrow();
        promoted_method_info_inner(
            &env,
            struct_name,
            method_name,
            include_pointer_receiver_methods,
            &mut std::collections::HashSet::new(),
        )
    })
}

fn promoted_method_info_inner(
    env: &typeinfer::TypeEnv,
    struct_name: &str,
    method_name: &str,
    include_pointer_receiver_methods: bool,
    visiting: &mut std::collections::HashSet<String>,
) -> Option<PromotedMethodInfo> {
    if !visiting.insert(struct_name.to_string()) {
        return None;
    }
    for (field_name, field_ty) in env.get_struct_fields(struct_name) {
        if !env.is_struct_embedded_field(struct_name, &field_name) {
            continue;
        }
        let Some((owner_type, field_is_pointer)) = super::embedded_field_target(&field_ty, env)
        else {
            continue;
        };
        let include_owner_pointer_methods = include_pointer_receiver_methods || field_is_pointer;
        if let Some(method_key) =
            direct_method_key_for_impl(env, &owner_type, method_name, include_owner_pointer_methods)
        {
            visiting.remove(struct_name);
            return Some(PromotedMethodInfo {
                owner_type,
                steps: vec![PromotedMethodStep {
                    field_name,
                    field_is_pointer,
                }],
                method_is_pointer_receiver: env.method_has_pointer_receiver(&method_key),
            });
        }
        if let Some(mut nested) = promoted_method_info_inner(
            env,
            &owner_type,
            method_name,
            include_owner_pointer_methods,
            visiting,
        ) {
            let mut steps = vec![PromotedMethodStep {
                field_name,
                field_is_pointer,
            }];
            steps.append(&mut nested.steps);
            nested.steps = steps;
            visiting.remove(struct_name);
            return Some(nested);
        }
    }
    visiting.remove(struct_name);
    None
}

fn promoted_method_receiver_expr(
    steps: &[PromotedMethodStep],
    call_receiver_kind: PointerCallReceiver,
) -> syn::Expr {
    let mut expr: syn::Expr = syn::parse_quote! { self };
    for step in steps {
        let field_ident = syn::Ident::new(
            &super::rust_safe_ident_name(&step.field_name),
            Span::mixed_site(),
        );
        expr = if step.field_is_pointer {
            syn::parse_quote! { (*#expr.#field_ident.lock().unwrap()) }
        } else {
            syn::parse_quote! { #expr.#field_ident }
        };
    }
    match call_receiver_kind {
        PointerCallReceiver::ImmutableRef => syn::parse_quote! { &#expr },
        PointerCallReceiver::MutableRef => syn::parse_quote! { &mut #expr },
        PointerCallReceiver::Owned => syn::parse_quote! { (#expr).clone() },
    }
}

fn promoted_concrete_item(
    struct_name: &str,
    method_name: &str,
    methods: &BTreeMap<String, Vec<syn::ImplItemFn>>,
) -> Option<syn::ImplItem> {
    let promoted = promoted_method_info(struct_name, method_name, false)?;
    let method = methods
        .get(&promoted.owner_type)?
        .iter()
        .find(|method| method.sig.ident == method_name)?
        .clone();
    let method_ident = method.sig.ident.clone();
    let method_is_pointer_receiver = promoted.method_is_pointer_receiver;
    let call_receiver_kind = call_receiver(&method, method_is_pointer_receiver);
    let owner_ident = syn::Ident::new(
        &super::rust_safe_ident_name(&promoted.owner_type),
        Span::mixed_site(),
    );
    let arg_idents = signature_arg_idents(&method.sig);
    let mut sig = method.sig;
    set_mut_self_receiver(&mut sig);
    let receiver = promoted_method_receiver_expr(&promoted.steps, call_receiver_kind);
    let block = if matches!(sig.output, syn::ReturnType::Default) {
        syn::parse_quote!({
            #owner_ident::#method_ident(#receiver, #(#arg_idents),*);
        })
    } else {
        syn::parse_quote!({
            #owner_ident::#method_ident(#receiver, #(#arg_idents),*)
        })
    };
    Some(impl_item_fn(sig, block))
}

fn concrete_can_emit_method(
    struct_name: &str,
    method_name: &str,
    methods: &BTreeMap<String, Vec<syn::ImplItemFn>>,
) -> bool {
    methods.get(struct_name).is_some_and(|method_list| {
        method_list
            .iter()
            .any(|method| method.sig.ident == method_name)
    }) || promoted_method_info(struct_name, method_name, false).is_some_and(|promoted| {
        methods
            .get(&promoted.owner_type)
            .is_some_and(|method_list| {
                method_list
                    .iter()
                    .any(|method| method.sig.ident == method_name)
            })
    })
}

fn concrete_as_any_item(exposes_any: bool) -> syn::ImplItem {
    let as_any = as_any_method_ident();
    if exposes_any {
        syn::parse_quote! {
            fn #as_any(&self) -> Option<&dyn std::any::Any> {
                Some(self)
            }
        }
    } else {
        syn::parse_quote! {
            fn #as_any(&self) -> Option<&dyn std::any::Any> {
                None
            }
        }
    }
}

fn concrete_direct_method_item(
    trait_name: &str,
    struct_name: &str,
    method: &syn::ImplItemFn,
) -> syn::ImplItem {
    let mut method = method.clone();
    method.vis = syn::Visibility::Inherited;
    let method_ident = method.sig.ident.clone();
    let immutable_error_method = trait_name == "error" && method_ident == "Error";
    let original_receiver = method
        .sig
        .inputs
        .first()
        .and_then(|arg| match arg {
            syn::FnArg::Receiver(receiver) => {
                Some((receiver.reference.is_some(), receiver.mutability.is_some()))
            }
            syn::FnArg::Typed(_) => None,
        })
        .unwrap_or((true, false));
    set_interface_receiver(&mut method.sig, immutable_error_method);
    if immutable_error_method {
        let struct_ident = syn::Ident::new(struct_name, Span::mixed_site());
        let arg_idents = signature_arg_idents(&method.sig);
        let (original_receiver_is_ref, original_receiver_is_mut) = original_receiver;
        method.block = concrete_error_method_block(
            &method.sig,
            &struct_ident,
            &method_ident,
            original_receiver_is_ref,
            original_receiver_is_mut,
            &arg_idents,
        );
    }
    syn::ImplItem::Fn(method)
}

fn concrete_error_method_block(
    sig: &syn::Signature,
    struct_ident: &syn::Ident,
    method_ident: &syn::Ident,
    original_receiver_is_ref: bool,
    original_receiver_is_mut: bool,
    arg_idents: &[syn::Ident],
) -> syn::Block {
    if original_receiver_is_mut {
        let receiver: syn::Expr = if original_receiver_is_ref {
            syn::parse_quote! { &mut __gors_receiver }
        } else {
            syn::parse_quote! { __gors_receiver }
        };
        if matches!(sig.output, syn::ReturnType::Default) {
            syn::parse_quote!({
                let mut __gors_receiver = (*self).clone();
                #struct_ident::#method_ident(#receiver, #(#arg_idents),*);
            })
        } else {
            syn::parse_quote!({
                let mut __gors_receiver = (*self).clone();
                #struct_ident::#method_ident(#receiver, #(#arg_idents),*)
            })
        }
    } else if matches!(sig.output, syn::ReturnType::Default) {
        syn::parse_quote!({
            #struct_ident::#method_ident(self, #(#arg_idents),*);
        })
    } else {
        syn::parse_quote!({
            #struct_ident::#method_ident(self, #(#arg_idents),*)
        })
    }
}

impl PointerImplTarget {
    fn as_any_item(self) -> syn::ImplItem {
        let as_any = as_any_method_ident();
        match self {
            Self::GorsPtr => syn::parse_quote! {
                fn #as_any(&self) -> Option<&dyn std::any::Any> {
                    Some(self)
                }
            },
            Self::BorrowedMut => syn::parse_quote! {
                fn #as_any(&self) -> Option<&dyn std::any::Any> {
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

fn set_mut_self_receiver(sig: &mut syn::Signature) {
    if let Some(syn::FnArg::Receiver(receiver)) = sig.inputs.first_mut() {
        receiver.mutability = Some(<Token![mut]>::default());
        *receiver.ty = syn::parse_quote! { &mut Self };
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
