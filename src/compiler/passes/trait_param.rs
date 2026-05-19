use syn::visit_mut::{self, VisitMut};

pub fn pass(file: &mut syn::File) {
    // Collect all trait names
    let trait_names: Vec<String> = file
        .items
        .iter()
        .filter_map(|item| {
            if let syn::Item::Trait(item_trait) = item {
                Some(item_trait.ident.to_string())
            } else {
                None
            }
        })
        .collect();

    if trait_names.is_empty() {
        return;
    }

    let mut transformer = TraitParam { trait_names };
    transformer.visit_file_mut(file);
}

struct TraitParam {
    trait_names: Vec<String>,
}

impl VisitMut for TraitParam {
    fn visit_fn_arg_mut(&mut self, arg: &mut syn::FnArg) {
        if let syn::FnArg::Typed(pat_type) = arg {
            if let syn::Type::Path(type_path) = &*pat_type.ty {
                if type_path.path.segments.len() == 1 {
                    let seg = &type_path.path.segments[0];
                    let name = seg.ident.to_string();
                    if self.trait_names.contains(&name) {
                        // Replace `Shape` with `impl Shape`
                        let trait_path = type_path.path.clone();
                        *pat_type.ty = syn::Type::ImplTrait(syn::TypeImplTrait {
                            impl_token: <syn::Token![impl]>::default(),
                            bounds: {
                                let mut bounds = syn::punctuated::Punctuated::new();
                                bounds.push(syn::TypeParamBound::Trait(syn::TraitBound {
                                    paren_token: None,
                                    modifier: syn::TraitBoundModifier::None,
                                    lifetimes: None,
                                    path: trait_path,
                                }));
                                bounds
                            },
                        });
                    }
                }
            }
        }

        visit_mut::visit_fn_arg_mut(self, arg);
    }
}
