use std::collections::BTreeMap;

use proc_macro2::Span;

use super::{CompiledModule, syn_inspect::type_mentions_name};

pub(super) fn add_fields_for_unused_type_params(modules: &mut BTreeMap<String, CompiledModule>) {
    for module in modules.values_mut() {
        add_fields_for_file(&mut module.file);
    }
}

fn add_fields_for_file(file: &mut syn::File) {
    use syn::visit_mut::VisitMut;

    let mut phantom_fields = BTreeMap::new();
    for item in &mut file.items {
        let syn::Item::Struct(item_struct) = item else {
            continue;
        };
        let type_params = item_struct
            .generics
            .type_params()
            .map(|param| param.ident.to_string())
            .collect::<Vec<_>>();
        if type_params.is_empty() {
            continue;
        }
        let syn::Fields::Named(fields) = &mut item_struct.fields else {
            continue;
        };
        if fields.named.iter().any(|field| {
            field
                .ident
                .as_ref()
                .is_some_and(|ident| ident == "_gors_phantom")
        }) {
            continue;
        }
        let used = fields
            .named
            .iter()
            .filter_map(|field| field.ident.as_ref().map(|ident| (ident, &field.ty)))
            .filter(|(ident, _)| *ident != "_gors_phantom")
            .flat_map(|(_, ty)| {
                type_params
                    .iter()
                    .filter(|param| {
                        let names = std::collections::HashSet::from([(*param).clone()]);
                        type_mentions_name(ty, &names)
                    })
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .collect::<std::collections::HashSet<_>>();
        let unused = type_params
            .into_iter()
            .filter(|param| !used.contains(param))
            .collect::<Vec<_>>();
        if unused.is_empty() {
            continue;
        }
        let unused_idents = unused
            .iter()
            .map(|name| syn::Ident::new(name, Span::mixed_site()))
            .collect::<Vec<_>>();
        let phantom_ty: syn::Type = if let [ident] = unused_idents.as_slice() {
            syn::parse_quote! { std::marker::PhantomData<fn() -> #ident> }
        } else {
            syn::parse_quote! { std::marker::PhantomData<fn() -> (#(#unused_idents),*)> }
        };
        fields.named.push(syn::parse_quote! {
            #[doc(hidden)]
            pub _gors_phantom: #phantom_ty
        });
        phantom_fields.insert(item_struct.ident.to_string(), ());
    }

    if phantom_fields.is_empty() {
        return;
    }

    struct PhantomLiteralUpdater<'a> {
        structs: &'a BTreeMap<String, ()>,
        current_self_struct: Option<String>,
    }

    fn impl_self_type_name(ty: &syn::Type) -> Option<String> {
        let syn::Type::Path(path) = ty else {
            return None;
        };
        if path.qself.is_some() {
            return None;
        }
        path.path
            .segments
            .last()
            .map(|segment| segment.ident.to_string())
    }

    impl VisitMut for PhantomLiteralUpdater<'_> {
        fn visit_item_impl_mut(&mut self, item_impl: &mut syn::ItemImpl) {
            let previous_self_struct = self.current_self_struct.clone();
            self.current_self_struct = impl_self_type_name(&item_impl.self_ty)
                .filter(|name| self.structs.contains_key(name));
            syn::visit_mut::visit_item_impl_mut(self, item_impl);
            self.current_self_struct = previous_self_struct;
        }

        fn visit_expr_struct_mut(&mut self, expr_struct: &mut syn::ExprStruct) {
            syn::visit_mut::visit_expr_struct_mut(self, expr_struct);
            let Some(name) = expr_struct
                .path
                .segments
                .last()
                .map(|segment| segment.ident.to_string())
            else {
                return;
            };
            let target_name = if name == "Self" {
                self.current_self_struct.as_deref().unwrap_or(&name)
            } else {
                &name
            };
            if !self.structs.contains_key(target_name)
                || expr_struct.fields.iter().any(|field| {
                    matches!(&field.member, syn::Member::Named(ident) if ident == "_gors_phantom")
                })
            {
                return;
            }
            expr_struct.fields.push(syn::parse_quote! {
                _gors_phantom: std::marker::PhantomData
            });
        }
    }

    PhantomLiteralUpdater {
        structs: &phantom_fields,
        current_self_struct: None,
    }
    .visit_file_mut(file);
}

#[cfg(test)]
mod tests {
    use quote::quote;
    use syn::parse_quote as rust;

    #[test]
    fn adds_phantom_fields_for_unused_generic_struct_params() {
        let mut file: syn::File = rust! {
            pub struct node<K: Clone, V> {
                isEntry: bool,
            }

            impl<K: Clone, V> Default for node<K, V> {
                fn default() -> Self {
                    Self {
                        isEntry: false,
                    }
                }
            }

            pub fn new<K: Clone, V>() -> node<K, V> {
                node::<K, V> {
                    isEntry: true,
                }
            }
        };

        super::add_fields_for_file(&mut file);
        let tokens = quote!(#file).to_string();

        assert!(tokens.contains("PhantomData < fn () -> (K , V) >"));
        assert!(tokens.contains("_gors_phantom : std :: marker :: PhantomData"));
        assert!(
            tokens.contains(
                "Self { isEntry : false , _gors_phantom : std :: marker :: PhantomData }"
            )
        );
    }

    #[test]
    fn leaves_structs_with_used_generic_params_unchanged() {
        let mut file: syn::File = rust! {
            pub struct node<K, V> {
                key: K,
                value: V,
            }
        };

        super::add_fields_for_file(&mut file);
        let tokens = quote!(#file).to_string();

        assert!(!tokens.contains("_gors_phantom"));
    }
}
