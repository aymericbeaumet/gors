mod fmt_flush;
mod mut_ref_forwarders;
mod noop_interfaces;
mod syn_helpers;

pub(super) fn inject(items: &mut Vec<syn::Item>) {
    mut_ref_forwarders::inject(items);
    noop_interfaces::inject(items);
    fmt_flush::inject(items);
}

#[cfg(test)]
mod tests {
    use super::syn_helpers::{
        ImplSelfType, has_impl, has_method, has_struct, has_trait, type_matches_impl_self,
    };
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
                struct fmtState {
                    buf: crate::builtin::GorsPtr<byteBuffer>,
                }
            },
            syn::parse_quote! {
                struct byteBuffer(Vec<u8>);
            },
            syn::parse_quote! {
                impl fmtState {
                    fn write(&mut self, b: Vec<u8>) -> usize {
                        self.buf.lock().unwrap().0.extend(b);
                        0
                    }
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
                    fn Write(&mut self, b: Vec<u8>) -> usize {
                        self.fmt.write(b)
                    }
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
                struct FormatState {
                    pending: crate::builtin::GorsPtr<ByteBuffer>,
                }
            },
            syn::parse_quote! {
                struct ByteBuffer(Vec<u8>);
            },
            syn::parse_quote! {
                struct Printer {
                    scratch: FormatState,
                    out: ByteBuffer,
                }
            },
            syn::parse_quote! {
                impl FormatState {
                    fn write(&mut self, b: Vec<u8>) -> usize {
                        self.pending.lock().unwrap().0.extend(b);
                        0
                    }
                }
            },
            syn::parse_quote! {
                impl Printer {
                    fn print(&mut self, b: Vec<u8>) -> usize {
                        self.scratch.write(b)
                    }
                }
            },
        ];

        inject(&mut items);

        let tokens = quote::quote!(#(#items)*).to_string();
        assert!(has_method(&items, "Printer", "__gors_flush_fmt"));
        assert!(!has_method(&items, "pp", "__gors_flush_fmt"));
        assert!(
            tokens.contains("self . scratch . pending . lock () . unwrap () . 0")
                && tokens.contains("self . out . 0 . extend (bytes)"),
            "expected flush hook to use detected field names: {tokens}"
        );
    }

    #[test]
    fn fmt_flush_injection_requires_source_buffer_flow() {
        let mut items: Vec<syn::Item> = vec![
            syn::parse_quote! {
                struct FormatState {
                    pending: crate::builtin::GorsPtr<ByteBuffer>,
                }
            },
            syn::parse_quote! {
                struct ByteBuffer(Vec<u8>);
            },
            syn::parse_quote! {
                struct Printer {
                    scratch: FormatState,
                    out: ByteBuffer,
                }
            },
            syn::parse_quote! {
                impl FormatState {
                    fn write(&mut self, b: Vec<u8>) -> usize {
                        b.len()
                    }
                }
            },
            syn::parse_quote! {
                impl Printer {
                    fn print(&mut self, b: Vec<u8>) -> usize {
                        self.scratch.write(b)
                    }
                }
            },
        ];

        inject(&mut items);

        assert!(!has_method(&items, "Printer", "__gors_flush_fmt"));
    }

    #[test]
    fn mut_ref_forwarders_are_derived_from_named_trait_impls() {
        let mut items: Vec<syn::Item> = vec![
            syn::parse_quote! {
                trait State {
                    fn Write(&mut self, b: Vec<u8>) -> usize;
                    fn Width(&self) -> usize;
                }
            },
            syn::parse_quote! {
                trait Sink {
                    fn Push(&mut self, value: isize) -> isize;
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
            syn::parse_quote! {
                impl Sink for Printer {
                    fn Push(&mut self, value: isize) -> isize { value }
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
        let sink_ref_impl = items.iter().find(|item| {
            let syn::Item::Impl(item_impl) = item else {
                return false;
            };
            item_impl.trait_.as_ref().is_some_and(|(_, path, _)| {
                path.segments.last().is_some_and(|seg| seg.ident == "Sink")
            }) && type_matches_impl_self(
                &item_impl.self_ty,
                ImplSelfType::MutableReferenceToNamed("Printer"),
            )
        });
        let tokens = quote::quote!(#(#items)*).to_string();

        assert!(state_ref_impl.is_some(), "{tokens}");
        assert!(sink_ref_impl.is_some(), "{tokens}");
        assert!(
            tokens.contains("< Printer as State > :: Write (& mut * * self , b)")
                && tokens.contains("< Printer as State > :: Width (& * * self)")
                && tokens.contains("< Printer as Sink > :: Push (& mut * * self , value)"),
            "expected generated &mut trait impls to forward through named impls: {tokens}"
        );
    }

    #[test]
    fn noop_interface_impl_is_derived_from_trait_signatures() {
        let mut items: Vec<syn::Item> = vec![
            syn::parse_quote! {
                trait Stringer {
                    fn __gors_as_any(&self) -> Option<&dyn std::any::Any>;
                    fn __gors_clone_box(&self) -> Box<dyn Stringer>;
                    fn String(&mut self) -> String;
                    fn Count(&mut self) -> isize;
                }
            },
            syn::parse_quote! {
                fn use_noop_stringer() {
                    let mut stringer = __GorsNoopInterface::default();
                    stringer.String();
                }
            },
        ];

        inject(&mut items);
        inject(&mut items);

        let stringer_impls = items
            .iter()
            .filter(|item| {
                let syn::Item::Impl(item_impl) = item else {
                    return false;
                };
                item_impl.trait_.as_ref().is_some_and(|(_, path, _)| {
                    path.segments
                        .last()
                        .is_some_and(|seg| seg.ident == "Stringer")
                }) && type_matches_impl_self(
                    &item_impl.self_ty,
                    ImplSelfType::Named("__GorsNoopInterface"),
                )
            })
            .count();
        let tokens = quote::quote!(#(#items)*).to_string();

        assert_eq!(stringer_impls, 1, "{tokens}");
        assert!(has_struct(&items, "__GorsNoopInterface"), "{tokens}");
        assert!(has_trait(&items, "__GorsErrorExt"), "{tokens}");
        assert!(
            tokens.contains(
                "fn __gors_clone_box (& self) -> Box < dyn Stringer > { Box :: new (Self :: default ()) as Box < dyn Stringer > }"
            ),
            "expected noop clone hook to be derived from the trait signature: {tokens}"
        );
        assert!(
            tokens.contains("fn Count (& mut self) -> isize { Default :: default () }"),
            "expected non-void noop methods to use a signature-derived zero value: {tokens}"
        );
        assert!(
            tokens.contains("impl __GorsErrorExt for __GorsNoopInterface")
                && tokens.contains("fn Error (& mut self) -> String { Default :: default () }"),
            "expected noop error extension body to use the shared noop method builder: {tokens}"
        );
    }

    #[test]
    fn formatter_noop_interface_requires_signature_dependencies() {
        let mut items: Vec<syn::Item> = vec![
            syn::parse_quote! {
                trait Formatter {
                    fn Format(&mut self, f: &mut dyn State, verb: i32);
                }
            },
            syn::parse_quote! {
                fn use_noop_formatter() {
                    let (mut formatter, mut ok) = (__GorsNoopInterface::default(), false);
                    if ok {
                        formatter.Format(Default::default(), 0);
                    }
                }
            },
        ];

        inject(&mut items);

        let tokens = quote::quote!(#(#items)*).to_string();
        assert!(has_struct(&items, "__GorsNoopInterface"), "{tokens}");
        assert!(
            !has_impl(
                &items,
                "Formatter",
                ImplSelfType::Named("__GorsNoopInterface")
            ),
            "{tokens}"
        );

        items.insert(
            0,
            syn::parse_quote! {
                trait State {}
            },
        );
        inject(&mut items);

        let tokens = quote::quote!(#(#items)*).to_string();
        assert!(has_struct(&items, "__GorsNoopInterface"), "{tokens}");
        assert!(
            has_impl(
                &items,
                "Formatter",
                ImplSelfType::Named("__GorsNoopInterface")
            ),
            "{tokens}"
        );
    }

    #[test]
    fn noop_interface_requires_actual_noop_default_use() {
        let mut items: Vec<syn::Item> = vec![syn::parse_quote! {
            trait Stringer {
                fn String(&mut self) -> String;
            }
        }];

        inject(&mut items);

        let tokens = quote::quote!(#(#items)*).to_string();
        assert!(!has_struct(&items, "__GorsNoopInterface"), "{tokens}");
        assert!(
            !has_impl(
                &items,
                "Stringer",
                ImplSelfType::Named("__GorsNoopInterface")
            ),
            "{tokens}"
        );
    }

    #[test]
    fn noop_interface_does_not_guess_ambiguous_method_targets() {
        let mut items: Vec<syn::Item> = vec![
            syn::parse_quote! {
                trait Left {
                    fn String(&mut self) -> String;
                }
            },
            syn::parse_quote! {
                trait Right {
                    fn String(&mut self) -> String;
                }
            },
            syn::parse_quote! {
                fn use_noop_stringer() {
                    let mut value = __GorsNoopInterface::default();
                    value.String();
                }
            },
        ];

        inject(&mut items);

        let tokens = quote::quote!(#(#items)*).to_string();
        assert!(has_struct(&items, "__GorsNoopInterface"), "{tokens}");
        assert!(
            !has_impl(&items, "Left", ImplSelfType::Named("__GorsNoopInterface")),
            "{tokens}"
        );
        assert!(
            !has_impl(&items, "Right", ImplSelfType::Named("__GorsNoopInterface")),
            "{tokens}"
        );
    }
}
