use super::{
    TYPE_ENV, go_package_rust_module_name, interface_type_env,
    item_reachability::impl_method_reachability_name,
    resolved_go_type,
    syn_inspect::{
        first_type_arg_for_path_last_ident_any, is_path_call_expr, is_receiver_type_wrapper_method,
        named_self_type,
    },
    typeinfer,
};

#[derive(Clone)]
pub(super) struct ReceiverTypeRef {
    pub(super) module: Option<String>,
    pub(super) name: String,
    pub(super) type_arg: Option<Box<ReceiverTypeRef>>,
}

impl ReceiverTypeRef {
    pub(super) fn new(module: Option<String>, name: String) -> Self {
        Self {
            module,
            name,
            type_arg: None,
        }
    }

    fn with_type_arg(mut self, type_arg: Option<ReceiverTypeRef>) -> Self {
        self.type_arg = type_arg.map(Box::new);
        self
    }
}

pub(super) type ReceiverTypeMap = std::collections::HashMap<String, ReceiverTypeRef>;
pub(super) type ReceiverFieldTypeMap = std::collections::HashMap<String, ReceiverTypeMap>;
pub(super) type ReceiverTupleTypes = Vec<Option<ReceiverTypeRef>>;
pub(super) type ReceiverTupleReturnMap = std::collections::HashMap<String, ReceiverTupleTypes>;

pub(super) struct ReceiverTypeContext<'a> {
    pub(super) module_names: &'a std::collections::HashSet<String>,
    pub(super) item_names: &'a std::collections::HashSet<String>,
    pub(super) top_level_return_types: &'a ReceiverTypeMap,
    pub(super) top_level_field_types: &'a ReceiverFieldTypeMap,
    pub(super) scopes: &'a [std::collections::BTreeMap<String, ReceiverTypeRef>],
    pub(super) current_self_type: Option<&'a ReceiverTypeRef>,
}

pub(super) fn top_level_item_types(
    items: &[syn::Item],
    module_names: &std::collections::HashSet<String>,
) -> std::collections::HashMap<String, ReceiverTypeRef> {
    let mut types = std::collections::HashMap::new();
    for item in items {
        match item {
            syn::Item::Const(item_const) => {
                if let Some(ty) = receiver_type_from_type(&item_const.ty, module_names) {
                    types.insert(item_const.ident.to_string(), ty);
                }
            }
            syn::Item::Static(item_static) => {
                if let Some(ty) = receiver_type_from_type(&item_static.ty, module_names) {
                    types.insert(item_static.ident.to_string(), ty);
                }
            }
            _ => {}
        }
    }
    types
}

pub(super) fn top_level_item_field_types(
    items: &[syn::Item],
    module_names: &std::collections::HashSet<String>,
) -> std::collections::HashMap<String, std::collections::HashMap<String, ReceiverTypeRef>> {
    let mut types = std::collections::HashMap::new();
    for item in items {
        let syn::Item::Struct(item_struct) = item else {
            continue;
        };
        let mut fields = std::collections::HashMap::new();
        if let syn::Fields::Named(named_fields) = &item_struct.fields {
            for field in &named_fields.named {
                let Some(field_ident) = &field.ident else {
                    continue;
                };
                if let Some(ty) = receiver_type_from_type(&field.ty, module_names) {
                    fields.insert(field_ident.to_string(), ty);
                }
            }
        }
        if !fields.is_empty() {
            types.insert(item_struct.ident.to_string(), fields);
        }
    }
    types
}

pub(super) fn top_level_collection_element_types(
    items: &[syn::Item],
    module_names: &std::collections::HashSet<String>,
) -> std::collections::HashMap<String, ReceiverTypeRef> {
    let mut types = std::collections::HashMap::new();
    for item in items {
        let syn::Item::Struct(item_struct) = item else {
            continue;
        };
        let syn::Fields::Unnamed(fields) = &item_struct.fields else {
            continue;
        };
        let Some(field) = fields.unnamed.first() else {
            continue;
        };
        if fields.unnamed.len() != 1 {
            continue;
        }
        if let Some(element_type) =
            collection_element_receiver_type_from_type(&field.ty, module_names)
        {
            types.insert(item_struct.ident.to_string(), element_type);
        }
    }
    types
}

fn collection_element_receiver_type_from_type(
    ty: &syn::Type,
    module_names: &std::collections::HashSet<String>,
) -> Option<ReceiverTypeRef> {
    match ty {
        syn::Type::Group(group) => {
            collection_element_receiver_type_from_type(&group.elem, module_names)
        }
        syn::Type::Paren(paren) => {
            collection_element_receiver_type_from_type(&paren.elem, module_names)
        }
        syn::Type::Path(path) => {
            let segment = path.path.segments.last()?;
            if segment.ident != "Vec" {
                return None;
            }
            let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
                return None;
            };
            args.args.iter().find_map(|arg| match arg {
                syn::GenericArgument::Type(ty) => receiver_type_from_type(ty, module_names),
                _ => None,
            })
        }
        syn::Type::Reference(reference) => {
            collection_element_receiver_type_from_type(&reference.elem, module_names)
        }
        _ => None,
    }
}

pub(super) fn top_level_item_return_types(
    items: &[syn::Item],
    module_names: &std::collections::HashSet<String>,
) -> std::collections::HashMap<String, ReceiverTypeRef> {
    let mut types = std::collections::HashMap::new();
    for item in items {
        match item {
            syn::Item::Fn(item_fn) => {
                let syn::ReturnType::Type(_, ty) = &item_fn.sig.output else {
                    continue;
                };
                if let Some(return_type) = receiver_type_from_type(ty, module_names) {
                    types.insert(item_fn.sig.ident.to_string(), return_type);
                }
            }
            syn::Item::Impl(item_impl) => {
                let Some(self_name) = named_self_type(&item_impl.self_ty) else {
                    continue;
                };
                for impl_item in &item_impl.items {
                    let syn::ImplItem::Fn(func) = impl_item else {
                        continue;
                    };
                    let syn::ReturnType::Type(_, ty) = &func.sig.output else {
                        continue;
                    };
                    if let Some(return_type) = receiver_type_from_type(ty, module_names) {
                        types.insert(
                            impl_method_reachability_name(&self_name, &func.sig.ident.to_string()),
                            return_type,
                        );
                    }
                }
            }
            syn::Item::Trait(item_trait) => {
                let trait_name = item_trait.ident.to_string();
                for trait_item in &item_trait.items {
                    let syn::TraitItem::Fn(func) = trait_item else {
                        continue;
                    };
                    let syn::ReturnType::Type(_, ty) = &func.sig.output else {
                        continue;
                    };
                    if let Some(return_type) = receiver_type_from_type(ty, module_names) {
                        types.insert(
                            impl_method_reachability_name(&trait_name, &func.sig.ident.to_string()),
                            return_type,
                        );
                    }
                }
            }
            _ => {}
        }
    }
    types
}

pub(super) fn top_level_item_tuple_return_types(
    items: &[syn::Item],
    module_names: &std::collections::HashSet<String>,
) -> std::collections::HashMap<String, ReceiverTupleTypes> {
    let mut types = std::collections::HashMap::new();
    for item in items {
        match item {
            syn::Item::Fn(item_fn) => {
                if let Some(tuple_types) =
                    receiver_tuple_types_from_return_type(&item_fn.sig.output, module_names)
                {
                    types.insert(item_fn.sig.ident.to_string(), tuple_types);
                }
            }
            syn::Item::Impl(item_impl) => {
                let Some(self_name) = named_self_type(&item_impl.self_ty) else {
                    continue;
                };
                for impl_item in &item_impl.items {
                    let syn::ImplItem::Fn(func) = impl_item else {
                        continue;
                    };
                    if let Some(tuple_types) =
                        receiver_tuple_types_from_return_type(&func.sig.output, module_names)
                    {
                        types.insert(
                            impl_method_reachability_name(&self_name, &func.sig.ident.to_string()),
                            tuple_types,
                        );
                    }
                }
            }
            _ => {}
        }
    }
    types
}

fn receiver_tuple_types_from_return_type(
    output: &syn::ReturnType,
    module_names: &std::collections::HashSet<String>,
) -> Option<ReceiverTupleTypes> {
    let syn::ReturnType::Type(_, ty) = output else {
        return None;
    };
    let syn::Type::Tuple(tuple) = ty.as_ref() else {
        return None;
    };
    let tuple_types = tuple
        .elems
        .iter()
        .map(|ty| receiver_type_from_type(ty, module_names))
        .collect::<Vec<_>>();
    tuple_types
        .iter()
        .any(Option::is_some)
        .then_some(tuple_types)
}

pub(super) fn method_receiver_type_from_expr(
    expr: &syn::Expr,
    context: &ReceiverTypeContext<'_>,
) -> Option<ReceiverTypeRef> {
    match expr {
        syn::Expr::Call(call) => {
            if is_path_call_expr(&call.func, &["crate", "builtin", "GorsPtr", "new"])
                || is_path_call_expr(&call.func, &["crate", "builtin", "GorsPtr", "from_arc"])
            {
                return call.args.first().and_then(|arg| {
                    method_receiver_type_from_expr(arg, context).or_else(|| {
                        receiver_type_from_init_expr(
                            arg,
                            context.module_names,
                            context.item_names,
                            context.top_level_return_types,
                        )
                    })
                });
            }
            receiver_type_from_init_expr(
                expr,
                context.module_names,
                context.item_names,
                context.top_level_return_types,
            )
            .or_else(|| {
                if is_path_call_expr(&call.func, &["std", "mem", "take"]) {
                    call.args
                        .first()
                        .and_then(|arg| method_receiver_type_from_expr(arg, context))
                } else {
                    None
                }
            })
        }
        syn::Expr::Cast(cast) => {
            receiver_type_from_trait_object_cast_type(&cast.ty, context.module_names)
                .or_else(|| method_receiver_type_from_expr(&cast.expr, context))
        }
        syn::Expr::Field(field) => {
            let base_type = method_receiver_type_from_expr(&field.base, context)?;
            let syn::Member::Named(member) = &field.member else {
                return None;
            };
            context
                .top_level_field_types
                .get(&base_type.name)
                .and_then(|fields| fields.get(&member.to_string()))
                .cloned()
        }
        syn::Expr::Group(group) => method_receiver_type_from_expr(&group.expr, context),
        syn::Expr::MethodCall(method) if is_receiver_type_wrapper_method(&method.method) => {
            method_receiver_type_from_expr(&method.receiver, context)
        }
        syn::Expr::Paren(paren) => method_receiver_type_from_expr(&paren.expr, context),
        syn::Expr::Path(path)
            if path.path.leading_colon.is_none() && path.path.segments.len() == 1 =>
        {
            let name = path.path.segments.first()?.ident.to_string();
            if name == "self" {
                return context.current_self_type.cloned();
            }
            context
                .scopes
                .iter()
                .rev()
                .find_map(|scope| scope.get(&name).cloned())
        }
        syn::Expr::Reference(reference) => method_receiver_type_from_expr(&reference.expr, context),
        syn::Expr::Struct(expr_struct) => {
            receiver_type_from_path(&expr_struct.path, context.module_names)
        }
        syn::Expr::Unary(unary) => method_receiver_type_from_expr(&unary.expr, context),
        _ => None,
    }
}

pub(super) fn receiver_type_from_type(
    ty: &syn::Type,
    module_names: &std::collections::HashSet<String>,
) -> Option<ReceiverTypeRef> {
    match ty {
        syn::Type::Group(group) => receiver_type_from_type(&group.elem, module_names),
        syn::Type::ImplTrait(impl_trait) => {
            impl_trait.bounds.iter().find_map(|bound| match bound {
                syn::TypeParamBound::Trait(trait_bound) => {
                    receiver_type_from_path(&trait_bound.path, module_names)
                }
                _ => None,
            })
        }
        syn::Type::Paren(paren) => receiver_type_from_type(&paren.elem, module_names),
        syn::Type::Path(path) => receiver_type_from_path(&path.path, module_names),
        syn::Type::Reference(reference) => receiver_type_from_type(&reference.elem, module_names),
        syn::Type::Ptr(ptr) => receiver_type_from_type(&ptr.elem, module_names),
        syn::Type::TraitObject(trait_object) => trait_object.bounds.iter().find_map(|bound| {
            if let syn::TypeParamBound::Trait(trait_bound) = bound {
                receiver_type_from_path(&trait_bound.path, module_names)
            } else {
                None
            }
        }),
        _ => None,
    }
}

fn receiver_type_ref_from_go_type(go_type: typeinfer::GoType) -> Option<ReceiverTypeRef> {
    receiver_type_ref_from_go_type_with_default_module(go_type, None)
}

fn receiver_type_ref_from_go_type_with_default_module(
    go_type: typeinfer::GoType,
    default_module: Option<&str>,
) -> Option<ReceiverTypeRef> {
    match resolved_go_type(&go_type) {
        typeinfer::GoType::Named(name) | typeinfer::GoType::Interface(name) => {
            if let Some((package, ty)) = name.rsplit_once('.') {
                Some(ReceiverTypeRef::new(
                    Some(go_package_rust_module_name(package)),
                    ty.to_string(),
                ))
            } else if let Some(module) = default_module {
                Some(ReceiverTypeRef::new(Some(module.to_string()), name))
            } else {
                Some(ReceiverTypeRef::new(None, name))
            }
        }
        typeinfer::GoType::Pointer(inner) => {
            receiver_type_ref_from_go_type_with_default_module(*inner, default_module)
        }
        _ => None,
    }
}

pub(super) fn external_receiver_method_return_type(
    receiver_type: &ReceiverTypeRef,
    method: &str,
) -> Option<ReceiverTypeRef> {
    let module = receiver_type.module.as_ref()?;
    TYPE_ENV
        .with(|env| {
            let env = env.borrow();
            let receiver_names = interface_type_env::rust_path_name_candidates(&format!(
                "{module}.{}",
                receiver_type.name
            ));
            receiver_names.into_iter().find_map(|receiver_name| {
                let ret = env.get_method_return(&receiver_name, method);
                if matches!(ret, typeinfer::GoType::Unknown) {
                    None
                } else {
                    receiver_type_ref_from_go_type(ret)
                        .map(|ret| specialize_receiver_return_type(receiver_type, ret))
                }
            })
        })
        .or_else(|| {
            let import_path = module.replace("__", "/");
            let (_, package_env) = crate::resolve::scan_type_env(&import_path)?;
            let ret = package_env.get_method_return(&receiver_type.name, method);
            if matches!(ret, typeinfer::GoType::Unknown) {
                None
            } else {
                receiver_type_ref_from_go_type_with_default_module(ret, Some(module))
                    .map(|ret| specialize_receiver_return_type(receiver_type, ret))
            }
        })
}

fn specialize_receiver_return_type(
    receiver_type: &ReceiverTypeRef,
    return_type: ReceiverTypeRef,
) -> ReceiverTypeRef {
    if return_type.module == receiver_type.module
        && return_type.name == "T"
        && let Some(type_arg) = &receiver_type.type_arg
    {
        return (**type_arg).clone();
    }
    return_type
}

fn external_function_return_type(module: &str, function: &str) -> Option<ReceiverTypeRef> {
    TYPE_ENV
        .with(|env| {
            let env = env.borrow();
            let function_names =
                interface_type_env::rust_path_name_candidates(&format!("{module}.{function}"));
            function_names.into_iter().find_map(|function_name| {
                let ret = env.get_func_return(&function_name);
                if matches!(ret, typeinfer::GoType::Unknown) {
                    None
                } else {
                    receiver_type_ref_from_go_type(ret)
                }
            })
        })
        .or_else(|| {
            let import_path = module.replace("__", "/");
            let (_, package_env) = crate::resolve::scan_type_env(&import_path)?;
            let ret = package_env.get_func_return(function);
            if matches!(ret, typeinfer::GoType::Unknown) {
                None
            } else {
                receiver_type_ref_from_go_type_with_default_module(ret, Some(module))
            }
        })
}

pub(super) fn receiver_type_from_init_expr(
    expr: &syn::Expr,
    module_names: &std::collections::HashSet<String>,
    item_names: &std::collections::HashSet<String>,
    top_level_return_types: &std::collections::HashMap<String, ReceiverTypeRef>,
) -> Option<ReceiverTypeRef> {
    match expr {
        syn::Expr::Call(call) => {
            if is_path_call_expr(&call.func, &["Box", "new"]) {
                return call.args.first().and_then(|arg| {
                    receiver_type_from_init_expr(
                        arg,
                        module_names,
                        item_names,
                        top_level_return_types,
                    )
                });
            }
            if is_path_call_expr(&call.func, &["std", "sync", "Arc", "new"])
                || is_path_call_expr(&call.func, &["std", "sync", "Mutex", "new"])
                || is_path_call_expr(&call.func, &["crate", "builtin", "GorsPtr", "new"])
            {
                return call.args.first().and_then(|arg| {
                    receiver_type_from_init_expr(
                        arg,
                        module_names,
                        item_names,
                        top_level_return_types,
                    )
                });
            }
            if let syn::Expr::Path(path) = &*call.func
                && let Some(first) = path.path.segments.first()
            {
                if let Some(qself) = &path.qself
                    && let Some(receiver_type) = receiver_type_from_type(&qself.ty, module_names)
                {
                    return Some(receiver_type);
                }
                if let Some(receiver_type) =
                    external_function_return_type_from_path(&path.path, module_names)
                {
                    return Some(receiver_type);
                }
                if is_external_module_value_call_path(&path.path, module_names) {
                    return None;
                }
                if let Some(receiver_type) =
                    receiver_type_from_associated_call_path(&path.path, module_names, item_names)
                {
                    return Some(receiver_type);
                }
                let name = first.ident.to_string();
                if let Some(return_type) = top_level_return_types.get(&name) {
                    return Some(return_type.clone());
                }
                if item_names.contains(&name) {
                    return Some(ReceiverTypeRef::new(None, name));
                }
            }
            receiver_type_from_init_expr(
                &call.func,
                module_names,
                item_names,
                top_level_return_types,
            )
        }
        syn::Expr::Cast(cast) => receiver_type_from_trait_object_cast_type(&cast.ty, module_names)
            .or_else(|| {
                receiver_type_from_init_expr(
                    &cast.expr,
                    module_names,
                    item_names,
                    top_level_return_types,
                )
            }),
        syn::Expr::Group(group) => receiver_type_from_init_expr(
            &group.expr,
            module_names,
            item_names,
            top_level_return_types,
        ),
        syn::Expr::Paren(paren) => receiver_type_from_init_expr(
            &paren.expr,
            module_names,
            item_names,
            top_level_return_types,
        ),
        syn::Expr::Reference(reference) => receiver_type_from_init_expr(
            &reference.expr,
            module_names,
            item_names,
            top_level_return_types,
        ),
        syn::Expr::Struct(expr_struct) => receiver_type_from_path(&expr_struct.path, module_names)
            .filter(|receiver_type| {
                receiver_type.module.is_some() || item_names.contains(&receiver_type.name)
            }),
        syn::Expr::MethodCall(method) if is_receiver_type_wrapper_method(&method.method) => {
            receiver_type_from_init_expr(
                &method.receiver,
                module_names,
                item_names,
                top_level_return_types,
            )
        }
        syn::Expr::Unary(unary) => receiver_type_from_init_expr(
            &unary.expr,
            module_names,
            item_names,
            top_level_return_types,
        ),
        _ => None,
    }
}

fn receiver_type_from_trait_object_cast_type(
    ty: &syn::Type,
    module_names: &std::collections::HashSet<String>,
) -> Option<ReceiverTypeRef> {
    if !type_is_or_wraps_trait_object(ty) {
        return None;
    }
    receiver_type_from_type(ty, module_names)
}

fn type_is_or_wraps_trait_object(ty: &syn::Type) -> bool {
    match ty {
        syn::Type::Group(group) => type_is_or_wraps_trait_object(&group.elem),
        syn::Type::Paren(paren) => type_is_or_wraps_trait_object(&paren.elem),
        syn::Type::Path(path) => {
            first_type_arg_for_path_last_ident_any(&path.path, &["Arc", "Box", "GorsPtr", "Mutex"])
                .is_some_and(type_is_or_wraps_trait_object)
        }
        syn::Type::Reference(reference) => type_is_or_wraps_trait_object(&reference.elem),
        syn::Type::TraitObject(_) => true,
        _ => false,
    }
}

fn external_function_return_type_from_path(
    path: &syn::Path,
    module_names: &std::collections::HashSet<String>,
) -> Option<ReceiverTypeRef> {
    let segments = path
        .segments
        .iter()
        .map(|seg| seg.ident.to_string())
        .collect::<Vec<_>>();
    match segments.as_slice() {
        [krate, module, function, ..] if krate == "crate" && module_names.contains(module) => {
            external_function_return_type(module, function)
        }
        [module, function, ..] if module_names.contains(module) => {
            external_function_return_type(module, function)
        }
        _ => None,
    }
}

fn is_external_module_value_call_path(
    path: &syn::Path,
    module_names: &std::collections::HashSet<String>,
) -> bool {
    let segments = path
        .segments
        .iter()
        .map(|seg| seg.ident.to_string())
        .collect::<Vec<_>>();
    match segments.as_slice() {
        [krate, module, _] if krate == "crate" => module_names.contains(module),
        [module, _] => module_names.contains(module),
        _ => false,
    }
}

pub(super) fn receiver_type_from_associated_call_path(
    path: &syn::Path,
    module_names: &std::collections::HashSet<String>,
    item_names: &std::collections::HashSet<String>,
) -> Option<ReceiverTypeRef> {
    let segments = path
        .segments
        .iter()
        .map(|seg| seg.ident.to_string())
        .collect::<Vec<_>>();
    match segments.as_slice() {
        [krate, module, name, ..] if krate == "crate" && module_names.contains(module) => {
            Some(ReceiverTypeRef::new(Some(module.clone()), name.clone()))
        }
        [module, name, ..] if module_names.contains(module) => {
            Some(ReceiverTypeRef::new(Some(module.clone()), name.clone()))
        }
        [name, ..] if item_names.contains(name) && segments.len() > 1 => {
            Some(ReceiverTypeRef::new(None, name.clone()))
        }
        _ => None,
    }
}

pub(super) fn receiver_type_from_path(
    path: &syn::Path,
    module_names: &std::collections::HashSet<String>,
) -> Option<ReceiverTypeRef> {
    if let Some(receiver_type) = transparent_receiver_type_from_path(path, module_names) {
        return Some(receiver_type);
    }

    let mut segments = path.segments.iter().map(|seg| seg.ident.to_string());
    let first = segments.next();
    let second = segments.next();
    let third = segments.next();
    match (first.as_deref(), second.as_deref(), third.as_deref()) {
        (Some("crate"), Some(module), Some(name)) if module_names.contains(module) => {
            return Some(
                ReceiverTypeRef::new(Some(module.to_string()), name.to_string())
                    .with_type_arg(first_receiver_type_arg(path, module_names)),
            );
        }
        (Some(module), Some(name), _) if module_names.contains(module) => {
            return Some(
                ReceiverTypeRef::new(Some(module.to_string()), name.to_string())
                    .with_type_arg(first_receiver_type_arg(path, module_names)),
            );
        }
        (Some(name), None, None) => {
            return Some(
                ReceiverTypeRef::new(None, name.to_string())
                    .with_type_arg(first_receiver_type_arg(path, module_names)),
            );
        }
        _ => {}
    }

    path.segments.iter().find_map(|seg| match &seg.arguments {
        syn::PathArguments::AngleBracketed(args) => args.args.iter().find_map(|arg| match arg {
            syn::GenericArgument::Type(ty) => receiver_type_from_type(ty, module_names),
            syn::GenericArgument::AssocType(assoc) => {
                receiver_type_from_type(&assoc.ty, module_names)
            }
            syn::GenericArgument::Constraint(constraint) => {
                constraint.bounds.iter().find_map(|bound| match bound {
                    syn::TypeParamBound::Trait(trait_bound) => {
                        receiver_type_from_path(&trait_bound.path, module_names)
                    }
                    _ => None,
                })
            }
            _ => None,
        }),
        syn::PathArguments::Parenthesized(args) => args
            .inputs
            .iter()
            .find_map(|ty| receiver_type_from_type(ty, module_names))
            .or_else(|| match &args.output {
                syn::ReturnType::Type(_, ty) => receiver_type_from_type(ty, module_names),
                syn::ReturnType::Default => None,
            }),
        syn::PathArguments::None => None,
    })
}

fn transparent_receiver_type_from_path(
    path: &syn::Path,
    module_names: &std::collections::HashSet<String>,
) -> Option<ReceiverTypeRef> {
    let inner = first_type_arg_for_path_last_ident_any(path, &["Arc", "Box", "GorsPtr", "Mutex"])?;
    receiver_type_from_type(inner, module_names)
}

fn first_receiver_type_arg(
    path: &syn::Path,
    module_names: &std::collections::HashSet<String>,
) -> Option<ReceiverTypeRef> {
    let segment = path.segments.last()?;
    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
        return None;
    };
    args.args.iter().find_map(|arg| match arg {
        syn::GenericArgument::Type(ty) => receiver_type_from_type(ty, module_names),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn receiver_type_from_path_preserves_first_external_type_arg() {
        let module_names = std::collections::HashSet::from(["sync__atomic".to_string()]);
        let ty: syn::Type = syn::parse_quote! {
            crate::sync__atomic::Pointer<poolChainElt>
        };

        let receiver = super::receiver_type_from_type(&ty, &module_names);
        assert!(receiver.is_some(), "expected external receiver type");
        let receiver = receiver.unwrap_or_else(|| super::ReceiverTypeRef::new(None, String::new()));

        assert_eq!(receiver.module.as_deref(), Some("sync__atomic"));
        assert_eq!(receiver.name, "Pointer");
        let type_arg = receiver.type_arg.as_deref();
        assert!(type_arg.is_some(), "expected concrete receiver type arg");
        let fallback = super::ReceiverTypeRef::new(None, String::new());
        let type_arg = type_arg.unwrap_or(&fallback);
        assert_eq!(type_arg.module, None);
        assert_eq!(type_arg.name, "poolChainElt");
    }
}
