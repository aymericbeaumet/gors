mod fmt_flush;
mod mut_ref_forwarders;
mod noop_interfaces;

pub(super) fn inject(items: &mut Vec<syn::Item>) {
    noop_interfaces::inject(items);
    mut_ref_forwarders::inject_state(items);
    fmt_flush::inject(items);
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

#[derive(Clone, Copy)]
enum ImplSelfType<'a> {
    Named(&'a str),
    MutableReferenceToNamed(&'a str),
    PointerCellToNamed(&'a str),
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
        ImplSelfType::PointerCellToNamed(name) => type_path_is_pointer_cell_to_name(ty, name),
    }
}

fn type_path_is_pointer_cell_to_name(ty: &syn::Type, name: &str) -> bool {
    let syn::Type::Path(type_path) = ty else {
        return false;
    };
    if type_path.qself.is_some() {
        return false;
    }
    let Some(segment) = type_path.path.segments.last() else {
        return false;
    };
    if segment.ident != "GorsPtr" {
        return false;
    }
    let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return false;
    };
    let mut args = arguments.args.iter();
    let Some(syn::GenericArgument::Type(inner)) = args.next() else {
        return false;
    };
    if args.next().is_some() {
        return false;
    }
    type_path_matches_name(inner, name)
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
            syn::parse_quote! {
                impl State for crate::builtin::GorsPtr<pp> {}
            },
        ];

        assert!(has_impl(
            &items,
            "State",
            ImplSelfType::MutableReferenceToNamed("pp")
        ));
        assert!(has_impl(
            &items,
            "State",
            ImplSelfType::PointerCellToNamed("pp")
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
                struct pp {
                    fmt: fmtState,
                    buf: byteBuffer,
                }
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
    fn fmt_flush_injection_uses_receiver_shape() {
        let mut items: Vec<syn::Item> = vec![
            syn::parse_quote! {
                trait State {
                    fn Write(&mut self, b: Vec<u8>) -> usize;
                }
            },
            syn::parse_quote! {
                struct Printer {
                    fmt: fmtState,
                    buf: byteBuffer,
                }
            },
            syn::parse_quote! {
                impl State for crate::builtin::GorsPtr<Printer> {
                    fn Write(&mut self, b: Vec<u8>) -> usize { b.len() }
                }
            },
        ];

        inject(&mut items);

        assert!(has_method(&items, "Printer", "__gors_flush_fmt"));
        assert!(!has_method(&items, "pp", "__gors_flush_fmt"));
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
