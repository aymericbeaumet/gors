use super::syn_helpers::{ImplSelfType, has_impl, has_struct, has_trait, trait_methods};
use crate::generated_names::{
    AS_ANY_METHOD, CLONE_BOX_METHOD, ERROR_EXT_TRAIT, NOOP_INTERFACE, error_ext_trait_ident,
    noop_interface_ident,
};

const STATE_TRAIT: &str = "State";

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
    let noop_ident = noop_interface_ident();
    syn::parse_quote! {
        #[derive(Clone, Default)]
        struct #noop_ident;
    }
}

fn noop_trait_impl(trait_name: &str, methods: &[syn::TraitItemFn]) -> syn::Item {
    let trait_ident = syn::Ident::new(trait_name, proc_macro2::Span::mixed_site());
    let noop_ident = noop_interface_ident();
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
    if sig.ident == AS_ANY_METHOD {
        syn::parse_quote!({ None })
    } else if sig.ident == CLONE_BOX_METHOD {
        syn::parse_quote!({ Box::new(Self::default()) as Box<dyn #trait_ident> })
    } else if matches!(sig.output, syn::ReturnType::Default) {
        syn::parse_quote!({})
    } else {
        syn::parse_quote!({ Default::default() })
    }
}

fn inject_error_ext(items: &mut Vec<syn::Item>) {
    let trait_ident = error_ext_trait_ident();
    let noop_ident = noop_interface_ident();

    if !has_trait(items, ERROR_EXT_TRAIT) {
        items.insert(
            0,
            syn::parse_quote! {
                trait #trait_ident {
                    fn Error(&mut self) -> String;
                }
            },
        );
    }

    if !has_impl(items, ERROR_EXT_TRAIT, ImplSelfType::Named("String")) {
        items.insert(
            0,
            syn::parse_quote! {
                impl #trait_ident for String {
                    fn Error(&mut self) -> String { self.clone() }
                }
            },
        );
    }

    if !has_impl(items, ERROR_EXT_TRAIT, ImplSelfType::Named(NOOP_INTERFACE)) {
        items.insert(0, noop_error_ext_impl(&trait_ident, &noop_ident));
    }
}

fn noop_error_ext_impl(trait_ident: &syn::Ident, noop_ident: &syn::Ident) -> syn::Item {
    let method: syn::TraitItemFn = syn::parse_quote! {
        fn Error(&mut self) -> String;
    };
    let impl_method = noop_impl_method(trait_ident, &method);

    syn::parse_quote! {
        impl #trait_ident for #noop_ident {
            #impl_method
        }
    }
}

struct FmtInterfaceFacts {
    traits: std::collections::BTreeSet<String>,
}

impl FmtInterfaceFacts {
    fn collect(items: &[syn::Item]) -> Self {
        let traits = items
            .iter()
            .filter_map(|item| match item {
                syn::Item::Trait(item_trait) => Some(item_trait.ident.to_string()),
                _ => None,
            })
            .collect();
        Self { traits }
    }

    fn has_noop_targets(&self) -> bool {
        FMT_NOOP_TRAITS
            .iter()
            .any(|target| self.should_inject(target))
    }

    fn should_inject(&self, target: &FmtNoopTrait) -> bool {
        self.traits.contains(target.name)
            && (!target.requires_state || self.traits.contains(STATE_TRAIT))
    }
}
