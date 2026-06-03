use super::{ImplSelfType, has_impl, has_struct, has_trait, trait_methods};

const NOOP_INTERFACE: &str = "__GorsNoopInterface";
const ERROR_EXT: &str = "__GorsErrorExt";

#[derive(Clone, Copy)]
struct FmtNoopTrait {
    name: &'static str,
    requires_state: bool,
}

const FMT_NOOP_TRAITS: &[FmtNoopTrait] = &[
    FmtNoopTrait {
        name: "Formatter",
        requires_state: true,
    },
    FmtNoopTrait {
        name: "Stringer",
        requires_state: false,
    },
    FmtNoopTrait {
        name: "GoStringer",
        requires_state: false,
    },
];

pub(super) fn inject(items: &mut Vec<syn::Item>) {
    let facts = FmtInterfaceFacts::collect(items);

    if !facts.has_noop_targets() {
        return;
    }

    if !has_struct(items, NOOP_INTERFACE) {
        items.insert(0, noop_struct_item());
    }

    for target in FMT_NOOP_TRAITS {
        if !facts.should_inject(target)
            || has_impl(items, target.name, ImplSelfType::Named(NOOP_INTERFACE))
        {
            continue;
        }
        let Some(methods) = trait_methods(items, target.name) else {
            continue;
        };
        items.insert(0, noop_trait_impl(target.name, &methods));
    }

    inject_error_ext(items);
}

fn noop_struct_item() -> syn::Item {
    syn::parse_quote! {
        #[derive(Clone, Default)]
        struct __GorsNoopInterface;
    }
}

fn noop_trait_impl(trait_name: &str, methods: &[syn::TraitItemFn]) -> syn::Item {
    let trait_ident = syn::Ident::new(trait_name, proc_macro2::Span::mixed_site());
    let noop_ident = syn::Ident::new(NOOP_INTERFACE, proc_macro2::Span::mixed_site());
    let impl_methods = methods
        .iter()
        .map(|method| noop_impl_method(&trait_ident, method))
        .collect::<Vec<_>>();

    syn::parse_quote! {
        impl #trait_ident for #noop_ident {
            #(#impl_methods)*
        }
    }
}

fn noop_impl_method(trait_ident: &syn::Ident, method: &syn::TraitItemFn) -> syn::ImplItemFn {
    let sig = method.sig.clone();
    let block = noop_method_body(trait_ident, &sig);
    syn::ImplItemFn {
        attrs: vec![],
        vis: syn::Visibility::Inherited,
        defaultness: None,
        sig,
        block,
    }
}

fn noop_method_body(trait_ident: &syn::Ident, sig: &syn::Signature) -> syn::Block {
    if sig.ident == "__gors_as_any" {
        syn::parse_quote!({ None })
    } else if sig.ident == "__gors_clone_box" {
        syn::parse_quote!({ Box::new(Self::default()) as Box<dyn #trait_ident> })
    } else if matches!(sig.output, syn::ReturnType::Default) {
        syn::parse_quote!({})
    } else {
        syn::parse_quote!({ Default::default() })
    }
}

fn inject_error_ext(items: &mut Vec<syn::Item>) {
    if !has_trait(items, ERROR_EXT) {
        items.insert(
            0,
            syn::parse_quote! {
                trait __GorsErrorExt {
                    fn Error(&mut self) -> String;
                }
            },
        );
    }

    if !has_impl(items, ERROR_EXT, ImplSelfType::Named("String")) {
        items.insert(
            0,
            syn::parse_quote! {
                impl __GorsErrorExt for String {
                    fn Error(&mut self) -> String { self.clone() }
                }
            },
        );
    }

    if !has_impl(items, ERROR_EXT, ImplSelfType::Named(NOOP_INTERFACE)) {
        items.insert(0, noop_error_ext_impl());
    }
}

fn noop_error_ext_impl() -> syn::Item {
    let method: syn::TraitItemFn = syn::parse_quote! {
        fn Error(&mut self) -> String;
    };
    let trait_ident = syn::Ident::new(ERROR_EXT, proc_macro2::Span::mixed_site());
    let noop_ident = syn::Ident::new(NOOP_INTERFACE, proc_macro2::Span::mixed_site());
    let impl_method = noop_impl_method(&trait_ident, &method);

    syn::parse_quote! {
        impl #trait_ident for #noop_ident {
            #impl_method
        }
    }
}

#[derive(Clone, Copy)]
struct FmtInterfaceFacts {
    has_formatter: bool,
    has_stringer: bool,
    has_go_stringer: bool,
    has_state: bool,
}

impl FmtInterfaceFacts {
    fn collect(items: &[syn::Item]) -> Self {
        Self {
            has_formatter: has_trait(items, "Formatter"),
            has_stringer: has_trait(items, "Stringer"),
            has_go_stringer: has_trait(items, "GoStringer"),
            has_state: has_trait(items, "State"),
        }
    }

    fn has_noop_targets(self) -> bool {
        FMT_NOOP_TRAITS
            .iter()
            .any(|target| self.should_inject(target))
    }

    fn should_inject(self, target: &FmtNoopTrait) -> bool {
        let has_trait = match target.name {
            "Formatter" => self.has_formatter,
            "Stringer" => self.has_stringer,
            "GoStringer" => self.has_go_stringer,
            _ => false,
        };
        has_trait && (!target.requires_state || self.has_state)
    }
}
