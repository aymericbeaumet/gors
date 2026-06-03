pub(super) fn inject(items: &mut Vec<syn::Item>) {
    let facts = StructuralHelperFacts::collect(items);

    inject_noop_interface_helpers(items, facts);
    inject_mut_ref_state_forwarders(items, facts);
    inject_fmt_flush_helper(items, facts);
}

#[derive(Clone, Copy)]
struct StructuralHelperFacts {
    has_formatter: bool,
    has_stringer: bool,
    has_go_stringer: bool,
    has_state: bool,
    has_pp: bool,
}

impl StructuralHelperFacts {
    fn collect(items: &[syn::Item]) -> Self {
        Self {
            has_formatter: has_trait(items, "Formatter"),
            has_stringer: has_trait(items, "Stringer"),
            has_go_stringer: has_trait(items, "GoStringer"),
            has_state: has_trait(items, "State"),
            has_pp: has_struct(items, "pp"),
        }
    }

    fn has_fmt_interfaces(self) -> bool {
        self.has_formatter || self.has_stringer || self.has_go_stringer
    }
}

fn inject_noop_interface_helpers(items: &mut Vec<syn::Item>, facts: StructuralHelperFacts) {
    if facts.has_fmt_interfaces() && !has_struct(items, "__GorsNoopInterface") {
        items.insert(
            0,
            syn::parse_quote! {
                #[derive(Clone, Default)]
                struct __GorsNoopInterface;
            },
        );
    }

    if facts.has_formatter
        && facts.has_state
        && !has_impl(
            items,
            "Formatter",
            ImplSelfType::Named("__GorsNoopInterface"),
        )
    {
        items.insert(
            0,
            syn::parse_quote! {
                impl Formatter for __GorsNoopInterface {
                    fn __gors_as_any(&self) -> Option<&dyn std::any::Any> { None }
                    fn Format(&mut self, _f: &mut dyn State, _verb: i32) {}
                }
            },
        );
    }

    if facts.has_stringer
        && !has_impl(
            items,
            "Stringer",
            ImplSelfType::Named("__GorsNoopInterface"),
        )
    {
        items.insert(
            0,
            syn::parse_quote! {
                impl Stringer for __GorsNoopInterface {
                    fn __gors_as_any(&self) -> Option<&dyn std::any::Any> { None }
                    fn String(&mut self) -> String { String::new() }
                }
            },
        );
    }

    if facts.has_go_stringer
        && !has_impl(
            items,
            "GoStringer",
            ImplSelfType::Named("__GorsNoopInterface"),
        )
    {
        items.insert(
            0,
            syn::parse_quote! {
                impl GoStringer for __GorsNoopInterface {
                    fn __gors_as_any(&self) -> Option<&dyn std::any::Any> { None }
                    fn GoString(&mut self) -> String { String::new() }
                }
            },
        );
    }

    if facts.has_fmt_interfaces() && !has_trait(items, "__GorsErrorExt") {
        items.insert(
            0,
            syn::parse_quote! {
                trait __GorsErrorExt {
                    fn Error(&mut self) -> String;
                }
            },
        );
        items.insert(
            0,
            syn::parse_quote! {
                impl __GorsErrorExt for String {
                    fn Error(&mut self) -> String { self.clone() }
                }
            },
        );
        items.insert(
            0,
            syn::parse_quote! {
                impl __GorsErrorExt for __GorsNoopInterface {
                    fn Error(&mut self) -> String { String::new() }
                }
            },
        );
    }
}

fn inject_mut_ref_state_forwarders(items: &mut Vec<syn::Item>, facts: StructuralHelperFacts) {
    if !facts.has_state {
        return;
    }
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

fn inject_fmt_flush_helper(items: &mut Vec<syn::Item>, facts: StructuralHelperFacts) {
    if facts.has_pp && !has_method(items, "pp", "__gors_flush_fmt") {
        items.insert(
            0,
            syn::parse_quote! {
                impl pp {
                    fn __gors_flush_fmt(&mut self) {
                        let bytes = std::mem::take(&mut self.fmt.buf.lock().unwrap().0);
                        self.buf.0.extend(bytes);
                    }
                }
            },
        );
    }
}

fn has_trait(items: &[syn::Item], name: &str) -> bool {
    items
        .iter()
        .any(|item| matches!(item, syn::Item::Trait(item_trait) if item_trait.ident == name))
}

fn has_struct(items: &[syn::Item], name: &str) -> bool {
    items
        .iter()
        .any(|item| matches!(item, syn::Item::Struct(item_struct) if item_struct.ident == name))
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

#[derive(Clone, Copy)]
enum ImplSelfType<'a> {
    Named(&'a str),
    MutableReferenceToNamed(&'a str),
}

fn has_impl(items: &[syn::Item], trait_name: &str, self_ty: ImplSelfType<'_>) -> bool {
    items.iter().any(|item| {
        let syn::Item::Impl(item_impl) = item else {
            return false;
        };
        let Some((_, path, _)) = &item_impl.trait_ else {
            return false;
        };
        path.segments
            .last()
            .is_some_and(|seg| seg.ident == trait_name)
            && type_matches_impl_self(&item_impl.self_ty, self_ty)
    })
}

fn type_matches_impl_self(ty: &syn::Type, expected: ImplSelfType<'_>) -> bool {
    match expected {
        ImplSelfType::Named(name) => type_path_matches_name(ty, name),
        ImplSelfType::MutableReferenceToNamed(name) => {
            let syn::Type::Reference(reference) = ty else {
                return false;
            };
            reference.mutability.is_some() && type_path_matches_name(&reference.elem, name)
        }
    }
}

fn type_path_ident_name(ty: &syn::Type) -> Option<String> {
    let syn::Type::Path(path) = ty else {
        return None;
    };
    if path.qself.is_some() || path.path.leading_colon.is_some() || path.path.segments.len() != 1 {
        return None;
    }
    let segment = path.path.segments.first()?;
    if !matches!(segment.arguments, syn::PathArguments::None) {
        return None;
    }
    Some(segment.ident.to_string())
}

fn type_path_matches_name(ty: &syn::Type, name: &str) -> bool {
    type_path_ident_name(ty).is_some_and(|ident| ident == name)
}

fn has_method(items: &[syn::Item], ty_name: &str, method_name: &str) -> bool {
    items.iter().any(|item| {
        let syn::Item::Impl(item_impl) = item else {
            return false;
        };
        let syn::Type::Path(type_path) = &*item_impl.self_ty else {
            return false;
        };
        if type_path
            .path
            .segments
            .last()
            .is_none_or(|seg| seg.ident != ty_name)
        {
            return false;
        }
        item_impl
            .items
            .iter()
            .any(|item| matches!(item, syn::ImplItem::Fn(func) if func.sig.ident == method_name))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn impl_self_type_matching_distinguishes_named_and_mut_ref_self() {
        let items: Vec<syn::Item> = vec![
            syn::parse_quote! {
                trait State {}
            },
            syn::parse_quote! {
                struct pp;
            },
            syn::parse_quote! {
                impl<'a> State for &'a mut pp {}
            },
        ];

        assert!(has_impl(
            &items,
            "State",
            ImplSelfType::MutableReferenceToNamed("pp")
        ));
        assert!(!has_impl(&items, "State", ImplSelfType::Named("pp")));
    }

    #[test]
    fn structural_helper_injection_adds_fmt_pp_helpers_once() {
        let mut items: Vec<syn::Item> = vec![
            syn::parse_quote! {
                trait State {
                    fn Write(&mut self, b: Vec<u8>) -> usize;
                    fn Width(&self) -> usize;
                }
            },
            syn::parse_quote! {
                struct pp;
            },
            syn::parse_quote! {
                impl State for pp {
                    fn Write(&mut self, b: Vec<u8>) -> usize { b.len() }
                    fn Width(&self) -> usize { 0 }
                }
            },
        ];

        inject(&mut items);
        inject(&mut items);

        let state_ref_impls = items
            .iter()
            .filter(|item| {
                let syn::Item::Impl(item_impl) = item else {
                    return false;
                };
                item_impl.trait_.as_ref().is_some_and(|(_, path, _)| {
                    path.segments.last().is_some_and(|seg| seg.ident == "State")
                }) && type_matches_impl_self(
                    &item_impl.self_ty,
                    ImplSelfType::MutableReferenceToNamed("pp"),
                )
            })
            .count();
        let flush_methods = items
            .iter()
            .filter(|item| {
                let syn::Item::Impl(item_impl) = item else {
                    return false;
                };
                type_matches_impl_self(&item_impl.self_ty, ImplSelfType::Named("pp"))
                    && item_impl.items.iter().any(
                        |item| matches!(item, syn::ImplItem::Fn(func) if func.sig.ident == "__gors_flush_fmt"),
                    )
            })
            .count();

        assert_eq!(state_ref_impls, 1);
        assert_eq!(flush_methods, 1);
    }

    #[test]
    fn state_mut_ref_forwarder_is_derived_from_named_impl() {
        let mut items: Vec<syn::Item> = vec![
            syn::parse_quote! {
                trait State {
                    fn Write(&mut self, b: Vec<u8>) -> usize;
                    fn Width(&self) -> usize;
                }
            },
            syn::parse_quote! {
                struct Printer;
            },
            syn::parse_quote! {
                impl State for Printer {
                    fn Write(&mut self, b: Vec<u8>) -> usize { b.len() }
                    fn Width(&self) -> usize { 0 }
                }
            },
        ];

        inject(&mut items);

        let state_ref_impl = items.iter().find(|item| {
            let syn::Item::Impl(item_impl) = item else {
                return false;
            };
            item_impl.trait_.as_ref().is_some_and(|(_, path, _)| {
                path.segments.last().is_some_and(|seg| seg.ident == "State")
            }) && type_matches_impl_self(
                &item_impl.self_ty,
                ImplSelfType::MutableReferenceToNamed("Printer"),
            )
        });
        let tokens = quote::quote!(#(#items)*).to_string();

        assert!(state_ref_impl.is_some(), "{tokens}");
        assert!(
            tokens.contains("< Printer as State > :: Write (& mut * * self , b)")
                && tokens.contains("< Printer as State > :: Width (& * * self)"),
            "expected generated &mut State impl to forward through named impl: {tokens}"
        );
    }
}
