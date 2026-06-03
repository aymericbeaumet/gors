pub(super) fn inject(items: &mut Vec<syn::Item>) {
    let facts = StructuralHelperFacts::collect(items);

    inject_noop_interface_helpers(items, facts);
    inject_fmt_state_ref_helper(items, facts);
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

fn inject_fmt_state_ref_helper(items: &mut Vec<syn::Item>, facts: StructuralHelperFacts) {
    if facts.has_pp
        && facts.has_state
        && !has_impl(items, "State", ImplSelfType::MutableReferenceToNamed("pp"))
    {
        items.insert(
            0,
            syn::parse_quote! {
                impl<'a> State for &'a mut pp {
                    fn __gors_as_any(&self) -> Option<&dyn std::any::Any> {
                        None
                    }

                    fn Write(&mut self, b: Vec<u8>) -> (isize, Box<dyn crate::builtin::error>) {
                        <pp as State>::Write(&mut **self, b)
                    }

                    fn Width(&mut self) -> (isize, bool) {
                        <pp as State>::Width(&mut **self)
                    }

                    fn Precision(&mut self) -> (isize, bool) {
                        <pp as State>::Precision(&mut **self)
                    }

                    fn Flag(&mut self, c: isize) -> bool {
                        <pp as State>::Flag(&mut **self, c)
                    }
                }
            },
        );
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

fn type_path_matches_name(ty: &syn::Type, name: &str) -> bool {
    let syn::Type::Path(path) = ty else {
        return false;
    };
    path.path
        .segments
        .last()
        .is_some_and(|seg| seg.ident == name)
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
                trait State {}
            },
            syn::parse_quote! {
                struct pp;
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
}
