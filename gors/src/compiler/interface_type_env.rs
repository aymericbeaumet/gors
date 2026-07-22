use super::{import_context, noop_interfaces, synthetic_names, typeinfer};
use crate::generated_names::{
    as_any_method_ident, clone_box_method_ident, interface_key_method_ident,
};
use syn::Token;

pub(super) fn rust_path_name_candidates(name: &str) -> Vec<String> {
    let mut candidates = vec![name.to_string()];
    if let Some((module, symbol)) = name.rsplit_once('.') {
        candidates.extend(
            import_context::local_names_for_rust_module(module)
                .into_iter()
                .map(|local_name| format!("{local_name}.{symbol}")),
        );
        if let Some(package_name) = module.rsplit("__").next() {
            candidates.push(format!("{package_name}.{symbol}"));
        }
        if let Some((_, package_name)) = module.rsplit_once('/') {
            candidates.push(format!("{package_name}.{symbol}"));
        }
        candidates.push(format!("{}.{symbol}", module.replace("__", "/")));
    }
    candidates.dedup();
    candidates
}

pub(super) fn noop_impl_items_for_interface_name(interface_name: &str) -> Vec<syn::ImplItem> {
    super::TYPE_ENV.with(|env| {
        let env = env.borrow();
        let Some(interface_env_name) = resolve_interface_env_name(interface_name, &env) else {
            return Vec::new();
        };
        let Some(method_names) = env.get_interface_direct_methods(&interface_env_name) else {
            return Vec::new();
        };
        let trait_path = super::interface_trait_path_from_name(interface_name);
        let as_any = as_any_method_ident();
        let interface_key = interface_key_method_ident();
        let clone_box = clone_box_method_ident();
        let mut items = vec![
            noop_interfaces::impl_item_for_signature(syn::parse_quote! {
                fn #as_any(&self) -> Option<&dyn std::any::Any>
            }),
            noop_interfaces::impl_item_for_signature(syn::parse_quote! {
                fn #interface_key(&self) -> crate::builtin::GorsInterfaceKey
            }),
            noop_interfaces::impl_item_for_signature(syn::parse_quote! {
                fn #clone_box(&self) -> Box<dyn #trait_path>
            }),
        ];
        items.extend(method_names.iter().map(|method_name| {
            noop_interfaces::impl_item_for_signature(interface_method_signature_from_type_env(
                &interface_env_name,
                method_name,
                &env,
            ))
        }));
        items
    })
}

pub(super) fn resolve_interface_env_name(name: &str, env: &typeinfer::TypeEnv) -> Option<String> {
    rust_path_name_candidates(name)
        .into_iter()
        .find(|candidate| env.is_interface(candidate))
}

pub(super) fn resolved_interface_method_signature_from_type_env(
    interface_name: &str,
    method_name: &str,
    env: &typeinfer::TypeEnv,
) -> Option<syn::Signature> {
    let interface_name = resolve_interface_env_name(interface_name, env)?;
    env.get_method_func_key(&interface_name, method_name)?;
    Some(interface_method_signature_from_type_env(
        &interface_name,
        method_name,
        env,
    ))
}

pub(super) fn trait_method_fns_for_path_from_type_env(
    trait_path: &syn::Path,
    env: &typeinfer::TypeEnv,
) -> Option<Vec<syn::TraitItemFn>> {
    let path_name = trait_path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .filter(|segment| segment != "crate")
        .collect::<Vec<_>>()
        .join(".");
    let interface_name = resolve_interface_env_name(&path_name, env)?;
    let direct_methods = env.get_interface_direct_methods(&interface_name)?;
    let as_any = as_any_method_ident();
    let interface_key = interface_key_method_ident();
    let clone_box = clone_box_method_ident();
    let mut methods = vec![
        syn::parse_quote! {
            fn #as_any(&self) -> Option<&dyn std::any::Any>;
        },
        syn::parse_quote! {
            fn #interface_key(&self) -> crate::builtin::GorsInterfaceKey;
        },
        syn::parse_quote! {
            fn #clone_box(&self) -> Box<dyn #trait_path>;
        },
    ];
    methods.extend(
        direct_methods
            .into_iter()
            .map(|method_name| syn::TraitItemFn {
                attrs: vec![],
                sig: interface_method_signature_from_type_env(&interface_name, &method_name, env),
                default: None,
                semi_token: Some(<Token![;]>::default()),
            }),
    );
    Some(methods)
}

fn interface_trait_type_from_go_type(
    go_type: &typeinfer::GoType,
    env: &typeinfer::TypeEnv,
) -> Option<syn::Type> {
    match go_type {
        typeinfer::GoType::Interface(name) => {
            let path = super::interface_trait_path_from_name(name);
            Some(syn::parse_quote! { #path })
        }
        typeinfer::GoType::Named(name) if resolve_interface_env_name(name, env).is_some() => {
            let path = super::interface_trait_path_from_name(name);
            Some(syn::parse_quote! { #path })
        }
        typeinfer::GoType::Instantiated { name, .. }
            if resolve_interface_env_name(name, env).is_some() =>
        {
            Some(super::rust_type_preserving_named_go_type(go_type))
        }
        typeinfer::GoType::Named(_) | typeinfer::GoType::Instantiated { .. } => {
            let resolved = env.resolve_alias(go_type);
            (resolved != *go_type)
                .then(|| interface_trait_type_from_go_type(&resolved, env))
                .flatten()
        }
        _ => None,
    }
}

fn owned_abi_type_from_go_type(
    go_type: &typeinfer::GoType,
    shape: Option<&typeinfer::SignatureTypeShape>,
    env: &typeinfer::TypeEnv,
) -> syn::Type {
    if let Some(trait_type) = interface_trait_type_from_go_type(go_type, env) {
        return syn::parse_quote! { Box<dyn #trait_type> };
    }

    match go_type {
        typeinfer::GoType::Slice(elem) => {
            let elem_shape = match shape {
                Some(typeinfer::SignatureTypeShape::Slice(elem)) => Some(elem.as_ref()),
                _ => None,
            };
            let elem = owned_abi_type_from_go_type(elem, elem_shape, env);
            syn::parse_quote! { crate::builtin::GorsSliceStorage<#elem> }
        }
        typeinfer::GoType::Array(elem) => {
            let (length, elem_shape) = match shape {
                Some(typeinfer::SignatureTypeShape::Array {
                    length: Some(length),
                    elem,
                }) => (length.as_str(), elem.as_ref()),
                Some(typeinfer::SignatureTypeShape::Array { length: None, .. }) | None => {
                    return syn::parse_quote! {
                        compile_error!("gors: missing exact fixed-array signature shape")
                    };
                }
                Some(_) => {
                    return syn::parse_quote! {
                        compile_error!("gors: mismatched fixed-array signature shape")
                    };
                }
            };
            let elem = owned_abi_type_from_go_type(elem, Some(elem_shape), env);
            let length = syn::LitInt::new(length, proc_macro2::Span::mixed_site());
            syn::parse_quote! { [#elem; #length] }
        }
        typeinfer::GoType::Map(key, value) => {
            let (key_shape, value_shape) = match shape {
                Some(typeinfer::SignatureTypeShape::Map { key, value }) => {
                    (Some(key.as_ref()), Some(value.as_ref()))
                }
                _ => (None, None),
            };
            let key_type = if super::go_type_is_interface_map_key(key) {
                super::interface_map_key_type()
            } else {
                owned_abi_type_from_go_type(key, key_shape, env)
            };
            let value_type = owned_abi_type_from_go_type(value, value_shape, env);
            let value_type = super::rust_map_storage_value_type(key, value_type);
            syn::parse_quote! { crate::builtin::GorsMap<#key_type, #value_type> }
        }
        typeinfer::GoType::Pointer(inner) => {
            let inner_shape = match shape {
                Some(typeinfer::SignatureTypeShape::Pointer(inner)) => Some(inner.as_ref()),
                _ => None,
            };
            let inner = owned_abi_type_from_go_type(inner, inner_shape, env);
            syn::parse_quote! { crate::builtin::GorsPtr<#inner> }
        }
        typeinfer::GoType::Chan { elem, .. } => {
            let elem_shape = match shape {
                Some(typeinfer::SignatureTypeShape::Chan(elem)) => Some(elem.as_ref()),
                _ => None,
            };
            let elem = owned_abi_type_from_go_type(elem, elem_shape, env);
            syn::parse_quote! { crate::builtin::Chan<#elem> }
        }
        typeinfer::GoType::Func {
            params, results, ..
        } => {
            let (param_shapes, result_shapes) = match shape {
                Some(typeinfer::SignatureTypeShape::Func { params, results }) => {
                    (Some(params.as_slice()), Some(results.as_slice()))
                }
                _ => (None, None),
            };
            let params = params
                .iter()
                .enumerate()
                .map(|(index, param)| {
                    let shape = param_shapes.and_then(|shapes| shapes.get(index));
                    if let typeinfer::GoType::Slice(elem) = env.resolve_alias(param)
                        && *elem == typeinfer::GoType::Uint8
                    {
                        let elem_shape = match shape {
                            Some(typeinfer::SignatureTypeShape::Slice(elem)) => Some(elem.as_ref()),
                            _ => None,
                        };
                        let elem = owned_abi_type_from_go_type(&elem, elem_shape, env);
                        return syn::parse_quote! { &mut [#elem] };
                    }
                    owned_abi_type_from_go_type(param, shape, env)
                })
                .collect::<Vec<_>>();
            let results = results
                .iter()
                .enumerate()
                .map(|(index, result)| {
                    owned_abi_type_from_go_type(
                        result,
                        result_shapes.and_then(|shapes| shapes.get(index)),
                        env,
                    )
                })
                .collect::<Vec<_>>();
            let result: syn::Type = match results.as_slice() {
                [] => syn::parse_quote! { () },
                [single] => single.clone(),
                many => syn::parse_quote! { (#(#many),*) },
            };
            syn::parse_quote! {
                std::sync::Arc<
                    std::sync::Mutex<
                        Option<std::sync::Arc<dyn Fn(#(#params),*) -> #result + Send + Sync>>
                    >
                >
            }
        }
        typeinfer::GoType::Instantiated { name, args } => {
            let base = super::named_go_type_path(name);
            let arg_shapes = match shape {
                Some(typeinfer::SignatureTypeShape::Instantiated(args)) => Some(args.as_slice()),
                _ => None,
            };
            let args = args
                .iter()
                .enumerate()
                .map(|(index, arg)| {
                    owned_abi_type_from_go_type(
                        arg,
                        arg_shapes.and_then(|shapes| shapes.get(index)),
                        env,
                    )
                })
                .collect();
            super::type_with_generic_args(base, args)
        }
        _ => super::rust_type_preserving_named_go_type(go_type),
    }
}

fn return_type_from_go_results(
    results: &[typeinfer::GoType],
    shapes: Option<&[typeinfer::SignatureTypeShape]>,
    env: &typeinfer::TypeEnv,
) -> syn::ReturnType {
    match results {
        [] => syn::ReturnType::Default,
        [single] => {
            let ty =
                owned_abi_type_from_go_type(single, shapes.and_then(|shapes| shapes.first()), env);
            syn::parse_quote! { -> #ty }
        }
        many => {
            let tys = many
                .iter()
                .enumerate()
                .map(|(index, result)| {
                    owned_abi_type_from_go_type(
                        result,
                        shapes.and_then(|shapes| shapes.get(index)),
                        env,
                    )
                })
                .collect::<Vec<_>>();
            syn::parse_quote! { -> (#(#tys),*) }
        }
    }
}

pub(super) fn interface_method_signature_from_type_env(
    interface_name: &str,
    method_name: &str,
    env: &typeinfer::TypeEnv,
) -> syn::Signature {
    let resolved_interface_name = resolve_interface_env_name(interface_name, env);
    let interface_name = resolved_interface_name.as_deref().unwrap_or(interface_name);
    let method_ident = syn::Ident::new(
        &super::rust_safe_ident_name(method_name),
        proc_macro2::Span::mixed_site(),
    );
    let mut inputs = syn::punctuated::Punctuated::new();
    inputs.push(syn::FnArg::Receiver(syn::Receiver {
        attrs: vec![],
        reference: Some((<Token![&]>::default(), None)),
        mutability: Some(<Token![mut]>::default()),
        self_token: <Token![self]>::default(),
        colon_token: None,
        ty: Box::new(syn::parse_quote! { &mut Self }),
    }));
    let method_key = env
        .get_method_func_key(interface_name, method_name)
        .unwrap_or_else(|| format!("{interface_name}.{method_name}"));
    let shapes = env.get_func_signature_shapes(&method_key);
    for (idx, param) in env
        .get_method_params(interface_name, method_name)
        .into_iter()
        .enumerate()
    {
        let ident = synthetic_names::unnamed_arg_ident(idx);
        let ty = interface_param_type_from_go_type(
            &method_key,
            idx,
            &param,
            shapes.as_ref().and_then(|shapes| shapes.params.get(idx)),
            env,
        );
        inputs.push(syn::FnArg::Typed(syn::PatType {
            attrs: vec![],
            pat: Box::new(syn::Pat::Ident(syn::PatIdent {
                attrs: vec![],
                by_ref: None,
                subpat: None,
                mutability: None,
                ident,
            })),
            colon_token: <Token![:]>::default(),
            ty: Box::new(ty),
        }));
    }
    syn::Signature {
        constness: None,
        asyncness: None,
        unsafety: None,
        abi: None,
        fn_token: <Token![fn]>::default(),
        ident: method_ident,
        generics: syn::Generics::default(),
        paren_token: syn::token::Paren::default(),
        inputs,
        variadic: None,
        output: return_type_from_go_results(
            &env.get_method_returns(interface_name, method_name),
            shapes.as_ref().map(|shapes| shapes.results.as_slice()),
            env,
        ),
    }
}

fn interface_param_type_from_go_type(
    method_key: &str,
    index: usize,
    param: &typeinfer::GoType,
    shape: Option<&typeinfer::SignatureTypeShape>,
    env: &typeinfer::TypeEnv,
) -> syn::Type {
    // Interface slice parameters use one stable Rust ABI. Unlike concrete
    // function parameters, whose borrowed form is selected by mutation
    // analysis, every interface method must match the trait declaration that
    // compile_type_spec emits from the Go signature.
    if env.get_func_variadic_start(method_key) != Some(index)
        && let typeinfer::GoType::Slice(elem) = env.resolve_alias(param)
    {
        let elem_shape = match shape {
            Some(typeinfer::SignatureTypeShape::Slice(elem)) => Some(elem.as_ref()),
            _ => None,
        };
        let elem = owned_abi_type_from_go_type(&elem, elem_shape, env);
        return syn::parse_quote! { &mut [#elem] };
    }
    if let Some(trait_type) = interface_trait_type_from_go_type(param, env) {
        if env.func_param_needs_owned_interface(method_key, index) {
            return syn::parse_quote! { Box<dyn #trait_type> };
        }
        return syn::parse_quote! { &mut dyn #trait_type };
    }
    owned_abi_type_from_go_type(param, shape, env)
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used)]
mod tests {
    use super::*;
    use quote::quote;
    use std::collections::HashSet;

    #[test]
    fn canonical_import_path_candidates_include_the_go_package_name() {
        let candidates = rust_path_name_candidates("io/fs.File");

        assert!(
            candidates.contains(&"fs.File".to_string()),
            "{candidates:?}"
        );
    }

    #[test]
    fn canonical_import_path_resolves_package_local_interface_method_abi() {
        let mut env = typeinfer::TypeEnv::new();
        env.set_type_kind("fs.FileInfo", typeinfer::TypeKind::Interface);
        env.set_interface_methods("fs.FileInfo", vec!["Name".to_string()]);
        env.set_func("fs.FileInfo.Name", vec![typeinfer::GoType::String]);
        env.set_type_kind("fs.File", typeinfer::TypeKind::Interface);
        env.set_interface_methods("fs.File", vec!["Read".to_string(), "Stat".to_string()]);
        env.set_func_params(
            "fs.File.Read",
            vec![typeinfer::GoType::Slice(Box::new(typeinfer::GoType::Uint8))],
        );
        env.set_func(
            "fs.File.Read",
            vec![typeinfer::GoType::Int, typeinfer::GoType::Error],
        );
        env.set_func(
            "fs.File.Stat",
            vec![
                typeinfer::GoType::Named("fs.FileInfo".to_string()),
                typeinfer::GoType::Error,
            ],
        );

        let read = resolved_interface_method_signature_from_type_env("io/fs.File", "Read", &env)
            .expect("canonical import path should resolve the package-local interface");
        let stat = resolved_interface_method_signature_from_type_env("io/fs.File", "Stat", &env)
            .expect("canonical import path should preserve interface result ownership");
        let generated_path_read =
            interface_method_signature_from_type_env("io__fs.File", "Read", &env);

        assert_eq!(
            quote! { #read }.to_string(),
            "fn Read (& mut self , __gors_arg_0 : & mut [u8]) -> (isize , Box < dyn crate :: builtin :: error >)"
        );
        assert_eq!(
            quote! { #stat }.to_string(),
            "fn Stat (& mut self) -> (Box < dyn fs :: FileInfo > , Box < dyn crate :: builtin :: error >)"
        );
        assert_eq!(
            quote! { #generated_path_read }.to_string(),
            "fn Read (& mut self , __gors_arg_0 : & mut [u8]) -> (isize , Box < dyn crate :: builtin :: error >)"
        );
    }

    fn typed_arg(sig: &syn::Signature, index: usize) -> &syn::Type {
        let Some(syn::FnArg::Typed(arg)) = sig.inputs.iter().nth(index + 1) else {
            panic!("missing typed argument {index}");
        };
        &arg.ty
    }

    fn output_type(sig: &syn::Signature) -> &syn::Type {
        let syn::ReturnType::Type(_, ty) = &sig.output else {
            panic!("missing return type");
        };
        ty
    }

    #[test]
    fn named_interface_result_uses_owned_trait_object() {
        let mut env = typeinfer::TypeEnv::new();
        env.set_type_kind("FS", typeinfer::TypeKind::Interface);
        env.set_type_kind("File", typeinfer::TypeKind::Interface);
        env.set_func_params("FS.Open", vec![typeinfer::GoType::String]);
        env.set_func(
            "FS.Open",
            vec![typeinfer::GoType::Named("File".to_string())],
        );

        let sig = interface_method_signature_from_type_env("FS", "Open", &env);

        assert_eq!(
            quote! { #sig }.to_string(),
            "fn Open (& mut self , __gors_arg_0 : String) -> Box < dyn File >"
        );
        assert!(!quote! { #sig }.to_string().contains("'_"));
    }

    #[test]
    fn named_interface_param_uses_borrowed_or_owned_method_abi() {
        let mut env = typeinfer::TypeEnv::new();
        env.set_type_kind("Resetter", typeinfer::TypeKind::Interface);
        env.set_type_kind("Reader", typeinfer::TypeKind::Interface);
        env.set_func_params(
            "Resetter.Reset",
            vec![typeinfer::GoType::Named("Reader".to_string())],
        );
        env.set_func("Resetter.Reset", Vec::new());

        let borrowed = interface_method_signature_from_type_env("Resetter", "Reset", &env);
        let borrowed_ty = typed_arg(&borrowed, 0);
        assert_eq!(quote! { #borrowed_ty }.to_string(), "& mut dyn Reader");

        env.set_owned_interface_params("Resetter.Reset", HashSet::from([0]));
        let owned = interface_method_signature_from_type_env("Resetter", "Reset", &env);
        let owned_ty = typed_arg(&owned, 0);
        assert_eq!(quote! { #owned_ty }.to_string(), "Box < dyn Reader >");
    }

    #[test]
    fn interface_slice_params_are_borrowed_without_concrete_mutation_facts() {
        let mut env = typeinfer::TypeEnv::new();
        env.set_type_kind("Writer", typeinfer::TypeKind::Interface);
        env.set_interface_methods("Writer", vec!["Write".to_string()]);
        env.set_func_params(
            "Writer.Write",
            vec![typeinfer::GoType::Slice(Box::new(typeinfer::GoType::Uint8))],
        );
        env.set_func("Writer.Write", vec![typeinfer::GoType::Int]);

        let sig = interface_method_signature_from_type_env("Writer", "Write", &env);

        assert_eq!(
            quote! { #sig }.to_string(),
            "fn Write (& mut self , __gors_arg_0 : & mut [u8]) -> isize"
        );
        assert!(!env.func_param_needs_borrowed_slice("Writer.Write", 0));
    }

    #[test]
    fn fixed_array_interface_signatures_keep_exact_lengths() {
        let file = crate::parser::parse_file(
            "arrays.go",
            r#"
                package arrays

                type Two interface {
                    RoundTrip([2]byte) [2]byte
                }

                type Three interface {
                    RoundTrip([3]byte) [3]byte
                }
            "#,
        )
        .unwrap();
        let mut env = typeinfer::TypeEnv::new();
        env.scan_file(&file);

        let two = interface_method_signature_from_type_env("Two", "RoundTrip", &env);
        let three = interface_method_signature_from_type_env("Three", "RoundTrip", &env);
        assert_eq!(
            quote! { #two }.to_string(),
            "fn RoundTrip (& mut self , __gors_arg_0 : [u8 ; 2]) -> [u8 ; 2]"
        );
        assert_eq!(
            quote! { #three }.to_string(),
            "fn RoundTrip (& mut self , __gors_arg_0 : [u8 ; 3]) -> [u8 ; 3]"
        );
        assert!(!quote! { #two #three }.to_string().contains("; _"));
    }

    #[test]
    fn missing_fixed_array_signature_shape_emits_compile_error_instead_of_vec() {
        let mut env = typeinfer::TypeEnv::new();
        env.set_type_kind("RoundTripper", typeinfer::TypeKind::Interface);
        env.set_interface_methods("RoundTripper", vec!["RoundTrip".to_string()]);
        env.set_func_params(
            "RoundTripper.RoundTrip",
            vec![typeinfer::GoType::Array(Box::new(typeinfer::GoType::Uint8))],
        );
        env.set_func(
            "RoundTripper.RoundTrip",
            vec![typeinfer::GoType::Array(Box::new(typeinfer::GoType::Uint8))],
        );

        let signature = interface_method_signature_from_type_env("RoundTripper", "RoundTrip", &env);
        let rendered = quote! { #signature }.to_string();

        assert!(
            rendered
                .contains("compile_error ! (\"gors: missing exact fixed-array signature shape\")"),
            "{rendered}"
        );
        assert!(!rendered.contains("Vec"), "{rendered}");
    }

    #[test]
    fn function_adapter_owns_interface_values_recursively() {
        let mut env = typeinfer::TypeEnv::new();
        env.set_type_kind("Adapter", typeinfer::TypeKind::Interface);
        env.set_type_kind("Reader", typeinfer::TypeKind::Interface);
        env.set_type_kind("File", typeinfer::TypeKind::Interface);
        env.set_func_params(
            "Adapter.Apply",
            vec![typeinfer::GoType::Func {
                params: vec![typeinfer::GoType::Named("Reader".to_string())],
                results: vec![typeinfer::GoType::Named("File".to_string())],
                variadic_start: None,
            }],
        );
        env.set_func("Adapter.Apply", Vec::new());

        let sig = interface_method_signature_from_type_env("Adapter", "Apply", &env);
        let ty = typed_arg(&sig, 0);
        let rendered = quote! { #ty }.to_string();

        assert!(
            rendered.contains("Fn (Box < dyn Reader >) -> Box < dyn File >"),
            "{rendered}"
        );
        assert!(!rendered.contains("Fn (Reader)"), "{rendered}");
    }

    #[test]
    fn interface_results_inside_tuples_and_slices_are_owned() {
        let mut env = typeinfer::TypeEnv::new();
        env.set_type_kind("Catalog", typeinfer::TypeKind::Interface);
        env.set_type_kind("File", typeinfer::TypeKind::Interface);
        env.set_func_params("Catalog.Files", Vec::new());
        env.set_func(
            "Catalog.Files",
            vec![
                typeinfer::GoType::Slice(Box::new(typeinfer::GoType::Named("File".to_string()))),
                typeinfer::GoType::Error,
            ],
        );

        let sig = interface_method_signature_from_type_env("Catalog", "Files", &env);
        let ty = output_type(&sig);

        assert_eq!(
            quote! { #ty }.to_string(),
            "(Vec < Box < dyn File > > , Box < dyn crate :: builtin :: error >)"
        );
    }
}
