use syn::visit_mut::{self, VisitMut};

pub fn pass(file: &mut syn::File) {
    InterfaceParam.visit_file_mut(file);
}

struct InterfaceParam;

impl VisitMut for InterfaceParam {
    fn visit_fn_arg_mut(&mut self, arg: &mut syn::FnArg) {
        if let syn::FnArg::Typed(pat_type) = arg {
            if let syn::Type::Path(type_path) = &*pat_type.ty {
                if is_external_interface_path(type_path) {
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

        visit_mut::visit_fn_arg_mut(self, arg);
    }
}

fn is_external_interface_path(type_path: &syn::TypePath) -> bool {
    if type_path.path.segments.len() < 2 {
        return false;
    }

    let Some(last) = type_path.path.segments.last() else {
        return false;
    };

    matches!(last.ident.to_string().as_str(), "Writer")
}
