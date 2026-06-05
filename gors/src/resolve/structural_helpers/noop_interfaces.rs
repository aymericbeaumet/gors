use super::syn_helpers::{ImplSelfType, has_impl, has_struct, has_trait, trait_methods};
use crate::generated_names::{
    ERROR_EXT_TRAIT, NOOP_INTERFACE, error_ext_trait_ident, noop_interface_ident,
};
use crate::noop_methods::{CloneBoxPolicy, MethodPolicy, NonHookReturnPolicy};

#[derive(Clone, Copy)]
struct FmtNoopTrait {
    name: &'static str,
}

const FMT_NOOP_TRAITS: &[FmtNoopTrait] = &[
    FmtNoopTrait { name: "Formatter" },
    FmtNoopTrait { name: "Stringer" },
    FmtNoopTrait { name: "GoStringer" },
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
    let default_expr: syn::Expr = syn::parse_quote! { Self::default() };
    let trait_path: syn::Path = syn::parse_quote! { #trait_ident };
    let policy = MethodPolicy {
        clone_box: CloneBoxPolicy::BoxDefault {
            default_expr: &default_expr,
            trait_path: &trait_path,
        },
        non_hook_return: NonHookReturnPolicy::Default,
    };
    let impl_methods = methods
        .iter()
        .map(|method| crate::noop_methods::impl_fn_for_trait_method(method, &policy))
        .collect::<Vec<_>>();

    syn::parse_quote! {
        impl #trait_ident for #noop_ident {
            #(#impl_methods)*
        }
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
    let default_expr: syn::Expr = syn::parse_quote! { Self::default() };
    let trait_path: syn::Path = syn::parse_quote! { #trait_ident };
    let policy = MethodPolicy {
        clone_box: CloneBoxPolicy::BoxDefault {
            default_expr: &default_expr,
            trait_path: &trait_path,
        },
        non_hook_return: NonHookReturnPolicy::Default,
    };
    let impl_method = crate::noop_methods::impl_fn_for_trait_method(&method, &policy);

    syn::parse_quote! {
        impl #trait_ident for #noop_ident {
            #impl_method
        }
    }
}

struct FmtInterfaceFacts {
    traits: std::collections::BTreeSet<String>,
    item_names: std::collections::BTreeSet<String>,
    signature_dependencies: std::collections::BTreeMap<String, std::collections::BTreeSet<String>>,
}

impl FmtInterfaceFacts {
    fn collect(items: &[syn::Item]) -> Self {
        let mut traits = std::collections::BTreeSet::new();
        let mut item_names = std::collections::BTreeSet::new();
        let mut signature_dependencies = std::collections::BTreeMap::new();

        for item in items {
            if let Some(name) = item_name(item) {
                item_names.insert(name);
            }
            let syn::Item::Trait(item_trait) = item else {
                continue;
            };
            let name = item_trait.ident.to_string();
            traits.insert(name.clone());
            signature_dependencies.insert(name, trait_signature_dependencies(item_trait));
        }

        Self {
            traits,
            item_names,
            signature_dependencies,
        }
    }

    fn has_noop_targets(&self) -> bool {
        FMT_NOOP_TRAITS
            .iter()
            .any(|target| self.should_inject(target))
    }

    fn should_inject(&self, target: &FmtNoopTrait) -> bool {
        self.traits.contains(target.name)
            && self
                .signature_dependencies
                .get(target.name)
                .is_none_or(|dependencies| {
                    dependencies
                        .iter()
                        .all(|dependency| self.item_names.contains(dependency))
                })
    }
}

fn item_name(item: &syn::Item) -> Option<String> {
    match item {
        syn::Item::Const(item_const) => Some(item_const.ident.to_string()),
        syn::Item::Enum(item_enum) => Some(item_enum.ident.to_string()),
        syn::Item::Fn(item_fn) => Some(item_fn.sig.ident.to_string()),
        syn::Item::Static(item_static) => Some(item_static.ident.to_string()),
        syn::Item::Struct(item_struct) => Some(item_struct.ident.to_string()),
        syn::Item::Trait(item_trait) => Some(item_trait.ident.to_string()),
        syn::Item::Type(item_type) => Some(item_type.ident.to_string()),
        _ => None,
    }
}

fn trait_signature_dependencies(item_trait: &syn::ItemTrait) -> std::collections::BTreeSet<String> {
    struct Finder {
        dependencies: std::collections::BTreeSet<String>,
    }

    impl syn::visit::Visit<'_> for Finder {
        fn visit_type_param_bound(&mut self, bound: &syn::TypeParamBound) {
            if let syn::TypeParamBound::Trait(trait_bound) = bound {
                collect_signature_dependency_path(&trait_bound.path, &mut self.dependencies);
            }
            syn::visit::visit_type_param_bound(self, bound);
        }

        fn visit_type_path(&mut self, type_path: &syn::TypePath) {
            collect_signature_dependency_path(&type_path.path, &mut self.dependencies);
            syn::visit::visit_type_path(self, type_path);
        }
    }

    let mut finder = Finder {
        dependencies: std::collections::BTreeSet::new(),
    };
    for item in &item_trait.items {
        let syn::TraitItem::Fn(func) = item else {
            continue;
        };
        syn::visit::Visit::visit_signature(&mut finder, &func.sig);
    }
    finder.dependencies
}

fn collect_signature_dependency_path(
    path: &syn::Path,
    dependencies: &mut std::collections::BTreeSet<String>,
) {
    if path.leading_colon.is_some() || path.segments.len() != 1 {
        return;
    }
    let Some(name) = path
        .segments
        .first()
        .map(|segment| segment.ident.to_string())
    else {
        return;
    };
    if is_rust_signature_builtin(&name) {
        return;
    }
    dependencies.insert(name);
}

fn is_rust_signature_builtin(name: &str) -> bool {
    matches!(
        name,
        "Any"
            | "Box"
            | "Option"
            | "Result"
            | "Self"
            | "String"
            | "Vec"
            | "bool"
            | "char"
            | "f32"
            | "f64"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "isize"
            | "str"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "usize"
    )
}
