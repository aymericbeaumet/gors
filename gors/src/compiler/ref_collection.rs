use super::{
    item_reachability::impl_method_reachability_name,
    receiver_type_facts::{
        ReceiverFieldTypeMap, ReceiverTupleReturnMap, ReceiverTupleTypes, ReceiverTypeMap,
        ReceiverTypeRef, external_receiver_method_return_type,
        receiver_type_from_associated_call_path, receiver_type_from_init_expr,
        receiver_type_from_type,
    },
    syn_inspect::{
        is_path_call_expr, is_receiver_type_wrapper_method, item_macro_name,
        macro_token_item_names, named_self_type, pat_ident_name, self_type_reachability_names,
    },
};

pub(super) type ReachabilityNameSet = std::collections::HashSet<String>;

pub(super) struct RefCollectionContext<'a> {
    pub(super) module_names: &'a ReachabilityNameSet,
    pub(super) item_names: &'a ReachabilityNameSet,
    pub(super) top_level_names: &'a ReachabilityNameSet,
    pub(super) top_level_types: &'a ReceiverTypeMap,
    pub(super) top_level_field_types: &'a ReceiverFieldTypeMap,
    pub(super) top_level_element_types: &'a ReceiverTypeMap,
    pub(super) top_level_return_types: &'a ReceiverTypeMap,
    pub(super) top_level_tuple_return_types: &'a ReceiverTupleReturnMap,
}

pub(super) fn collect_refs_from_item(
    item: &mut syn::Item,
    context: &RefCollectionContext<'_>,
) -> (
    std::collections::HashSet<String>,
    std::collections::HashMap<String, std::collections::HashSet<String>>,
) {
    use syn::visit_mut::VisitMut;

    struct BoundCollector<'a> {
        names: std::collections::HashSet<String>,
        types: std::collections::HashMap<String, ReceiverTypeRef>,
        module_names: &'a ReachabilityNameSet,
        item_names: &'a ReachabilityNameSet,
        top_level_field_types: &'a ReceiverFieldTypeMap,
        top_level_element_types: &'a ReceiverTypeMap,
        top_level_return_types: &'a ReceiverTypeMap,
        top_level_tuple_return_types: &'a ReceiverTupleReturnMap,
        current_self_type: Option<ReceiverTypeRef>,
    }

    impl BoundCollector<'_> {
        fn bound_receiver_type_from_expr(&self, expr: &syn::Expr) -> Option<ReceiverTypeRef> {
            match expr {
                syn::Expr::Call(call) => self
                    .bound_receiver_type_from_iife_call(call)
                    .or_else(|| self.bound_receiver_type_from_expr(&call.func)),
                syn::Expr::Cast(cast) => self.bound_receiver_type_from_expr(&cast.expr),
                syn::Expr::Field(field) => {
                    let base_type = self.bound_receiver_type_from_expr(&field.base)?;
                    let syn::Member::Named(member) = &field.member else {
                        return None;
                    };
                    self.top_level_field_types
                        .get(&base_type.name)
                        .and_then(|fields| fields.get(&member.to_string()))
                        .cloned()
                }
                syn::Expr::Group(group) => self.bound_receiver_type_from_expr(&group.expr),
                syn::Expr::Index(index) => {
                    let base_type = self.bound_receiver_type_from_expr(&index.expr)?;
                    self.top_level_element_types.get(&base_type.name).cloned()
                }
                syn::Expr::MethodCall(method)
                    if is_receiver_type_wrapper_method(&method.method) =>
                {
                    receiver_type_from_init_expr(
                        &method.receiver,
                        self.module_names,
                        self.item_names,
                        self.top_level_return_types,
                    )
                    .or_else(|| self.bound_receiver_type_from_expr(&method.receiver))
                }
                syn::Expr::MethodCall(method) => {
                    let receiver_type = self.bound_receiver_type_from_expr(&method.receiver)?;
                    let method_key = impl_method_reachability_name(
                        &receiver_type.name,
                        &method.method.to_string(),
                    );
                    self.top_level_return_types
                        .get(&method_key)
                        .cloned()
                        .or_else(|| {
                            external_receiver_method_return_type(
                                &receiver_type,
                                &method.method.to_string(),
                            )
                        })
                }
                syn::Expr::Paren(paren) => self.bound_receiver_type_from_expr(&paren.expr),
                syn::Expr::Path(path)
                    if path.path.leading_colon.is_none() && path.path.segments.len() == 1 =>
                {
                    let name = path.path.segments.first()?.ident.to_string();
                    if name == "self" {
                        return self.current_self_type.clone();
                    }
                    self.types.get(&name).cloned()
                }
                syn::Expr::Reference(reference) => {
                    self.bound_receiver_type_from_expr(&reference.expr)
                }
                syn::Expr::Unary(unary) => self.bound_receiver_type_from_expr(&unary.expr),
                _ => None,
            }
        }

        fn bound_receiver_type_from_iife_call(
            &self,
            call: &syn::ExprCall,
        ) -> Option<ReceiverTypeRef> {
            if !call.args.is_empty() {
                return None;
            }
            let closure = closure_expr_from_call_func(&call.func)?;
            let tail = tail_expr_from_expr(&closure.body)?;
            self.bound_receiver_type_from_expr(tail)
        }
    }

    fn closure_expr_from_call_func(expr: &syn::Expr) -> Option<&syn::ExprClosure> {
        match expr {
            syn::Expr::Closure(closure) => Some(closure),
            syn::Expr::Group(group) => closure_expr_from_call_func(&group.expr),
            syn::Expr::Paren(paren) => closure_expr_from_call_func(&paren.expr),
            _ => None,
        }
    }

    fn tail_expr_from_expr(expr: &syn::Expr) -> Option<&syn::Expr> {
        match expr {
            syn::Expr::Block(block) => tail_expr_from_block(&block.block),
            syn::Expr::Group(group) => tail_expr_from_expr(&group.expr),
            syn::Expr::Paren(paren) => tail_expr_from_expr(&paren.expr),
            _ => Some(expr),
        }
    }

    fn tail_expr_from_block(block: &syn::Block) -> Option<&syn::Expr> {
        let syn::Stmt::Expr(expr, None) = block.stmts.last()? else {
            return None;
        };
        Some(expr)
    }

    impl VisitMut for BoundCollector<'_> {
        fn visit_pat_ident_mut(&mut self, pat: &mut syn::PatIdent) {
            self.names.insert(pat.ident.to_string());
            syn::visit_mut::visit_pat_ident_mut(self, pat);
        }

        fn visit_fn_arg_mut(&mut self, arg: &mut syn::FnArg) {
            if let syn::FnArg::Typed(pat_type) = arg
                && let Some(name) = pat_ident_name(&pat_type.pat)
                && let Some(ty) = receiver_type_from_type(&pat_type.ty, self.module_names)
            {
                self.types.insert(name, ty);
            }
            syn::visit_mut::visit_fn_arg_mut(self, arg);
        }

        fn visit_expr_closure_mut(&mut self, closure: &mut syn::ExprClosure) {
            for input in &closure.inputs {
                if let syn::Pat::Type(pat_type) = input
                    && let Some(name) = pat_ident_name(&pat_type.pat)
                    && let Some(ty) = receiver_type_from_type(&pat_type.ty, self.module_names)
                {
                    self.types.insert(name, ty);
                }
            }
            syn::visit_mut::visit_expr_closure_mut(self, closure);
        }

        fn visit_local_mut(&mut self, local: &mut syn::Local) {
            if let Some(init) = &local.init
                && let syn::Pat::Tuple(tuple_pat) = &local.pat
                && let Some(tuple_types) = receiver_tuple_types_from_init_expr(
                    &init.expr,
                    self.top_level_tuple_return_types,
                )
            {
                for (pat, receiver_type) in tuple_pat.elems.iter().zip(tuple_types) {
                    if let Some(name) = pat_ident_name(pat)
                        && let Some(receiver_type) = receiver_type
                    {
                        self.types.insert(name, receiver_type);
                    }
                }
            }

            if let syn::Pat::Type(pat_type) = &local.pat
                && let Some(name) = pat_ident_name(&pat_type.pat)
                && let Some(ty) = receiver_type_from_type(&pat_type.ty, self.module_names)
            {
                self.types.insert(name, ty);
            } else if let Some(init) = &local.init
                && let Some(name) = pat_ident_name(&local.pat)
                && let Some(ty) = self.bound_receiver_type_from_expr(&init.expr).or_else(|| {
                    receiver_type_from_init_expr(
                        &init.expr,
                        self.module_names,
                        self.item_names,
                        self.top_level_return_types,
                    )
                })
            {
                self.types.insert(name, ty);
            }
            syn::visit_mut::visit_local_mut(self, local);
        }

        fn visit_item_impl_mut(&mut self, item_impl: &mut syn::ItemImpl) {
            let previous_self_type = self.current_self_type.clone();
            self.current_self_type =
                named_self_type(&item_impl.self_ty).map(|name| ReceiverTypeRef::new(None, name));
            syn::visit_mut::visit_item_impl_mut(self, item_impl);
            self.current_self_type = previous_self_type;
        }
    }

    fn receiver_tuple_types_from_init_expr(
        expr: &syn::Expr,
        top_level_tuple_return_types: &std::collections::HashMap<String, ReceiverTupleTypes>,
    ) -> Option<ReceiverTupleTypes> {
        match expr {
            syn::Expr::Call(call) => {
                if let syn::Expr::Path(path) = &*call.func
                    && let Some(first) = path.path.segments.first()
                    && let Some(types) = top_level_tuple_return_types.get(&first.ident.to_string())
                {
                    return Some(types.clone());
                }
                receiver_tuple_types_from_init_expr(&call.func, top_level_tuple_return_types)
            }
            syn::Expr::Cast(cast) => {
                receiver_tuple_types_from_init_expr(&cast.expr, top_level_tuple_return_types)
            }
            syn::Expr::Group(group) => {
                receiver_tuple_types_from_init_expr(&group.expr, top_level_tuple_return_types)
            }
            syn::Expr::Paren(paren) => {
                receiver_tuple_types_from_init_expr(&paren.expr, top_level_tuple_return_types)
            }
            syn::Expr::Reference(reference) => {
                receiver_tuple_types_from_init_expr(&reference.expr, top_level_tuple_return_types)
            }
            syn::Expr::Unary(unary) => {
                receiver_tuple_types_from_init_expr(&unary.expr, top_level_tuple_return_types)
            }
            _ => None,
        }
    }

    fn external_module_from_expr(
        expr: &syn::Expr,
        module_names: &std::collections::HashSet<String>,
    ) -> Option<String> {
        match expr {
            syn::Expr::Call(_) => None,
            syn::Expr::Cast(cast) => external_module_from_expr(&cast.expr, module_names),
            syn::Expr::Field(field) => external_module_from_expr(&field.base, module_names),
            syn::Expr::Group(group) => external_module_from_expr(&group.expr, module_names),
            syn::Expr::Index(index) => external_module_from_expr(&index.expr, module_names),
            syn::Expr::MethodCall(method) => {
                external_module_from_expr(&method.receiver, module_names)
            }
            syn::Expr::Paren(paren) => external_module_from_expr(&paren.expr, module_names),
            syn::Expr::Path(path) => {
                let mut segments = path.path.segments.iter().map(|seg| seg.ident.to_string());
                let first = segments.next();
                let second = segments.next();
                match (first.as_deref(), second.as_deref()) {
                    (Some(module), None) if module_names.contains(module) => {
                        Some(module.to_string())
                    }
                    (Some("crate"), Some(module)) if module_names.contains(module) => {
                        Some(module.to_string())
                    }
                    (Some(module), Some(_)) if module_names.contains(module) => {
                        Some(module.to_string())
                    }
                    _ => None,
                }
            }
            syn::Expr::Reference(reference) => {
                external_module_from_expr(&reference.expr, module_names)
            }
            syn::Expr::Try(try_expr) => external_module_from_expr(&try_expr.expr, module_names),
            syn::Expr::Unary(unary) => external_module_from_expr(&unary.expr, module_names),
            _ => None,
        }
    }

    fn external_path_symbol_from_expr(
        expr: &syn::Expr,
        module_names: &std::collections::HashSet<String>,
    ) -> Option<(String, String)> {
        match expr {
            syn::Expr::Call(_) => None,
            syn::Expr::Cast(cast) => external_path_symbol_from_expr(&cast.expr, module_names),
            syn::Expr::Group(group) => external_path_symbol_from_expr(&group.expr, module_names),
            syn::Expr::Paren(paren) => external_path_symbol_from_expr(&paren.expr, module_names),
            syn::Expr::Reference(reference) => {
                external_path_symbol_from_expr(&reference.expr, module_names)
            }
            syn::Expr::Try(try_expr) => {
                external_path_symbol_from_expr(&try_expr.expr, module_names)
            }
            syn::Expr::Unary(unary) => external_path_symbol_from_expr(&unary.expr, module_names),
            syn::Expr::MethodCall(method) if is_receiver_type_wrapper_method(&method.method) => {
                external_path_symbol_from_expr(&method.receiver, module_names)
            }
            syn::Expr::Field(field) => {
                let syn::Member::Named(member) = &field.member else {
                    return None;
                };
                external_module_from_expr(&field.base, module_names)
                    .map(|module| (module, member.to_string()))
            }
            syn::Expr::Path(path) => {
                let mut segments = path.path.segments.iter().map(|seg| seg.ident.to_string());
                match (
                    segments.next().as_deref(),
                    segments.next().as_deref(),
                    segments.next().as_deref(),
                ) {
                    (Some("crate"), Some(module), Some(symbol))
                        if module_names.contains(module) =>
                    {
                        Some((module.to_string(), symbol.to_string()))
                    }
                    (Some(module), Some(symbol), _) if module_names.contains(module) => {
                        Some((module.to_string(), symbol.to_string()))
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    struct RefCollector<'a> {
        module_names: &'a std::collections::HashSet<String>,
        item_names: &'a std::collections::HashSet<String>,
        top_level_names: &'a std::collections::HashSet<String>,
        top_level_types: &'a std::collections::HashMap<String, ReceiverTypeRef>,
        top_level_field_types: &'a std::collections::HashMap<
            String,
            std::collections::HashMap<String, ReceiverTypeRef>,
        >,
        top_level_element_types: &'a std::collections::HashMap<String, ReceiverTypeRef>,
        top_level_return_types: &'a std::collections::HashMap<String, ReceiverTypeRef>,
        bound_names: std::collections::HashSet<String>,
        bound_types: std::collections::HashMap<String, ReceiverTypeRef>,
        current_self_type: Option<ReceiverTypeRef>,
        current_self_reachability_names: Vec<String>,
        local_names: std::collections::HashSet<String>,
        external_refs: std::collections::HashMap<String, std::collections::HashSet<String>>,
    }

    impl RefCollector<'_> {
        fn receiver_type_from_expr(&self, expr: &syn::Expr) -> Option<ReceiverTypeRef> {
            match expr {
                syn::Expr::Group(group) => self.receiver_type_from_expr(&group.expr),
                syn::Expr::Paren(paren) => self.receiver_type_from_expr(&paren.expr),
                syn::Expr::Path(path)
                    if path.path.leading_colon.is_none() && path.path.segments.len() == 1 =>
                {
                    let name = path.path.segments.first()?.ident.to_string();
                    if name == "self" {
                        return self.current_self_type.clone();
                    }
                    self.bound_types
                        .get(&name)
                        .cloned()
                        .or_else(|| self.top_level_types.get(&name).cloned())
                }
                syn::Expr::Call(call) => receiver_type_from_init_expr(
                    expr,
                    self.module_names,
                    self.item_names,
                    self.top_level_return_types,
                )
                .or_else(|| self.receiver_type_from_iife_call(call))
                .or_else(|| {
                    if is_path_call_expr(&call.func, &["std", "mem", "take"]) {
                        call.args
                            .first()
                            .and_then(|arg| self.receiver_type_from_expr(arg))
                    } else {
                        None
                    }
                })
                .or_else(|| self.receiver_type_from_expr(&call.func)),
                syn::Expr::MethodCall(method)
                    if is_receiver_type_wrapper_method(&method.method) =>
                {
                    self.receiver_type_from_expr(&method.receiver)
                }
                syn::Expr::MethodCall(method) => {
                    let receiver_type = self.receiver_type_from_expr(&method.receiver)?;
                    let method_key = impl_method_reachability_name(
                        &receiver_type.name,
                        &method.method.to_string(),
                    );
                    self.top_level_return_types
                        .get(&method_key)
                        .cloned()
                        .or_else(|| {
                            external_receiver_method_return_type(
                                &receiver_type,
                                &method.method.to_string(),
                            )
                        })
                }
                syn::Expr::Cast(cast) => self.receiver_type_from_expr(&cast.expr),
                syn::Expr::Field(field) => {
                    let base_type = self.receiver_type_from_expr(&field.base)?;
                    let syn::Member::Named(member) = &field.member else {
                        return None;
                    };
                    self.top_level_field_types
                        .get(&base_type.name)
                        .and_then(|fields| fields.get(&member.to_string()))
                        .cloned()
                }
                syn::Expr::Index(index) => {
                    let base_type = self.receiver_type_from_expr(&index.expr)?;
                    self.top_level_element_types.get(&base_type.name).cloned()
                }
                syn::Expr::Reference(reference) => self.receiver_type_from_expr(&reference.expr),
                syn::Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Deref(_)) => {
                    self.receiver_type_from_expr(&unary.expr)
                }
                _ => None,
            }
        }

        fn receiver_type_from_iife_call(&self, call: &syn::ExprCall) -> Option<ReceiverTypeRef> {
            if !call.args.is_empty() {
                return None;
            }
            let closure = closure_expr_from_call_func(&call.func)?;
            let tail = tail_expr_from_expr(&closure.body)?;
            self.receiver_type_from_expr(tail)
        }

        fn insert_receiver_method_ref(&mut self, receiver_type: ReceiverTypeRef, method: &str) {
            if let Some(module) = receiver_type.module {
                let entry = self.external_refs.entry(module).or_default();
                entry.insert(receiver_type.name.clone());
                entry.insert(impl_method_reachability_name(&receiver_type.name, method));
            } else {
                if is_reachability_name(&receiver_type.name) {
                    self.local_names.insert(receiver_type.name.clone());
                }
                self.local_names
                    .insert(impl_method_reachability_name(&receiver_type.name, method));
            }
        }

        fn fallback_field_receiver_types(&self, expr: &syn::Expr) -> Vec<ReceiverTypeRef> {
            match expr {
                syn::Expr::Group(group) => self.fallback_field_receiver_types(&group.expr),
                syn::Expr::Paren(paren) => self.fallback_field_receiver_types(&paren.expr),
                syn::Expr::Field(field) => {
                    let syn::Member::Named(member) = &field.member else {
                        return Vec::new();
                    };
                    let field_name = member.to_string();
                    self.top_level_field_types
                        .values()
                        .filter_map(|fields| fields.get(&field_name).cloned())
                        .collect()
                }
                _ => Vec::new(),
            }
        }
    }

    impl VisitMut for RefCollector<'_> {
        fn visit_path_mut(&mut self, path: &mut syn::Path) {
            syn::visit_mut::visit_path_mut(self, path);

            let mut segments = path.segments.iter().map(|seg| seg.ident.to_string());
            let first = segments.next();
            let second = segments.next();
            let third = segments.next();
            let fourth = segments.next();

            match (
                first.as_deref(),
                second.as_deref(),
                third.as_deref(),
                fourth.as_deref(),
            ) {
                (Some("crate"), Some(module), Some(symbol), assoc)
                    if self.module_names.contains(module) =>
                {
                    let entry = self.external_refs.entry(module.to_string()).or_default();
                    entry.insert(symbol.to_string());
                    if let Some(assoc) = assoc {
                        entry.insert(assoc.to_string());
                    }
                }
                (Some(module), Some(symbol), assoc, _) if self.module_names.contains(module) => {
                    let entry = self.external_refs.entry(module.to_string()).or_default();
                    entry.insert(symbol.to_string());
                    if let Some(assoc) = assoc {
                        entry.insert(assoc.to_string());
                    }
                }
                (Some(local), Some(symbol), assoc, _) if self.item_names.contains(local) => {
                    self.local_names.insert(local.to_string());
                    self.local_names.insert(symbol.to_string());
                    self.local_names
                        .insert(impl_method_reachability_name(local, symbol));
                    if let Some(assoc) = assoc {
                        self.local_names.insert(assoc.to_string());
                        self.local_names
                            .insert(impl_method_reachability_name(symbol, assoc));
                    }
                }
                (Some("Self"), Some(symbol), _, _) => {
                    for self_name in &self.current_self_reachability_names {
                        if is_reachability_name(self_name) {
                            self.local_names.insert(self_name.clone());
                        }
                        self.local_names
                            .insert(impl_method_reachability_name(self_name, symbol));
                    }
                }
                _ => {}
            }
        }

        fn visit_expr_path_mut(&mut self, expr_path: &mut syn::ExprPath) {
            syn::visit_mut::visit_expr_path_mut(self, expr_path);
            if expr_path.path.leading_colon.is_some() || expr_path.path.segments.len() != 1 {
                return;
            }
            let Some(name) = expr_path
                .path
                .segments
                .first()
                .map(|seg| seg.ident.to_string())
            else {
                return;
            };
            if self.item_names.contains(&name) && !self.bound_names.contains(&name) {
                self.local_names.insert(name);
            }
        }

        fn visit_type_path_mut(&mut self, type_path: &mut syn::TypePath) {
            syn::visit_mut::visit_type_path_mut(self, type_path);
            let Some(last) = type_path.path.segments.last() else {
                return;
            };
            let name = last.ident.to_string();
            if self.top_level_names.contains(&name) {
                self.local_names.insert(name);
            }
        }

        fn visit_expr_struct_mut(&mut self, expr_struct: &mut syn::ExprStruct) {
            let Some(last) = expr_struct.path.segments.last() else {
                syn::visit_mut::visit_expr_struct_mut(self, expr_struct);
                return;
            };
            let name = last.ident.to_string();
            if self.item_names.contains(&name) {
                self.local_names.insert(name);
            }
            syn::visit_mut::visit_expr_struct_mut(self, expr_struct);
        }

        fn visit_item_impl_mut(&mut self, item_impl: &mut syn::ItemImpl) {
            if let Some((_, path, _)) = &item_impl.trait_
                && let Some(last) = path.segments.last()
            {
                let name = last.ident.to_string();
                if self.item_names.contains(&name) {
                    self.local_names.insert(name);
                }
            }
            let previous_self_type = self.current_self_type.clone();
            let previous_self_reachability_names =
                std::mem::take(&mut self.current_self_reachability_names);
            self.current_self_type =
                named_self_type(&item_impl.self_ty).map(|name| ReceiverTypeRef::new(None, name));
            self.current_self_reachability_names = self_type_reachability_names(&item_impl.self_ty);
            syn::visit_mut::visit_item_impl_mut(self, item_impl);
            self.current_self_type = previous_self_type;
            self.current_self_reachability_names = previous_self_reachability_names;
        }

        fn visit_type_param_bound_mut(&mut self, bound: &mut syn::TypeParamBound) {
            if let syn::TypeParamBound::Trait(trait_bound) = bound
                && let Some(last) = trait_bound.path.segments.last()
            {
                let name = last.ident.to_string();
                if self.item_names.contains(&name) {
                    self.local_names.insert(name);
                }
            }
            syn::visit_mut::visit_type_param_bound_mut(self, bound);
        }

        fn visit_item_macro_mut(&mut self, item_macro: &mut syn::ItemMacro) {
            if let Some(name) = item_macro_name(item_macro)
                && self.item_names.contains(&name)
            {
                self.local_names.insert(name);
            }
            self.local_names.extend(macro_token_item_names(
                &item_macro.mac.tokens,
                self.item_names,
            ));
            syn::visit_mut::visit_item_macro_mut(self, item_macro);
        }

        fn visit_macro_mut(&mut self, mac: &mut syn::Macro) {
            self.local_names
                .extend(macro_token_item_names(&mac.tokens, self.top_level_names));
            syn::visit_mut::visit_macro_mut(self, mac);
        }

        fn visit_expr_method_call_mut(&mut self, method: &mut syn::ExprMethodCall) {
            let name = method.method.to_string();
            if let Some(receiver_type) = self.receiver_type_from_expr(&method.receiver) {
                self.insert_receiver_method_ref(receiver_type, &name);
            } else {
                let fallback_receivers = self.fallback_field_receiver_types(&method.receiver);
                if !fallback_receivers.is_empty() {
                    for receiver_type in fallback_receivers {
                        self.insert_receiver_method_ref(receiver_type, &name);
                    }
                } else if let Some(module) =
                    external_module_from_expr(&method.receiver, self.module_names)
                {
                    let entry = self.external_refs.entry(module).or_default();
                    if let Some((_, symbol)) =
                        external_path_symbol_from_expr(&method.receiver, self.module_names)
                    {
                        entry.insert(impl_method_reachability_name(
                            &symbol,
                            &method.method.to_string(),
                        ));
                        entry.insert(symbol);
                    } else {
                        entry.insert(name);
                    }
                } else if !self.top_level_names.contains(&name) {
                    self.local_names.insert(name);
                }
            }
            syn::visit_mut::visit_expr_method_call_mut(self, method);
        }

        fn visit_expr_call_mut(&mut self, call: &mut syn::ExprCall) {
            if let syn::Expr::Path(path) = &*call.func {
                if let Some(qself) = &path.qself
                    && let Some(receiver_type) =
                        receiver_type_from_type(&qself.ty, self.module_names)
                    && let Some(method) = path.path.segments.last()
                {
                    self.insert_receiver_method_ref(receiver_type, &method.ident.to_string());
                } else if let Some(receiver_type) = receiver_type_from_associated_call_path(
                    &path.path,
                    self.module_names,
                    self.item_names,
                ) && let Some(method) = path.path.segments.last()
                {
                    self.insert_receiver_method_ref(receiver_type, &method.ident.to_string());
                }
            }
            syn::visit_mut::visit_expr_call_mut(self, call);
        }
    }

    let mut bound_collector = BoundCollector {
        names: std::collections::HashSet::new(),
        types: std::collections::HashMap::new(),
        module_names: context.module_names,
        item_names: context.item_names,
        top_level_field_types: context.top_level_field_types,
        top_level_element_types: context.top_level_element_types,
        top_level_return_types: context.top_level_return_types,
        top_level_tuple_return_types: context.top_level_tuple_return_types,
        current_self_type: None,
    };
    let mut item_for_bounds = item.clone();
    bound_collector.visit_item_mut(&mut item_for_bounds);

    let mut collector = RefCollector {
        module_names: context.module_names,
        item_names: context.item_names,
        top_level_names: context.top_level_names,
        top_level_types: context.top_level_types,
        top_level_field_types: context.top_level_field_types,
        top_level_element_types: context.top_level_element_types,
        top_level_return_types: context.top_level_return_types,
        bound_names: bound_collector.names,
        bound_types: bound_collector.types,
        current_self_type: None,
        current_self_reachability_names: Vec::new(),
        local_names: std::collections::HashSet::new(),
        external_refs: std::collections::HashMap::new(),
    };
    collector.visit_item_mut(item);
    (collector.local_names, collector.external_refs)
}

fn is_reachability_name(name: &str) -> bool {
    !matches!(
        name,
        "AsMut"
            | "AsRef"
            | "Box"
            | "Clone"
            | "Copy"
            | "Debug"
            | "Default"
            | "Deref"
            | "DerefMut"
            | "Display"
            | "Err"
            | "Error"
            | "From"
            | "Into"
            | "None"
            | "Ok"
            | "Option"
            | "Result"
            | "Self"
            | "Some"
            | "String"
            | "ToString"
            | "Vec"
            | "bool"
            | "char"
            | "clone"
            | "collect"
            | "default"
            | "extend"
            | "false"
            | "is_empty"
            | "iter"
            | "len"
            | "new"
            | "push"
            | "std"
            | "to_string"
            | "true"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "usize"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "isize"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_refs_follows_qself_associated_method_calls() {
        let module_names = ReachabilityNameSet::new();
        let item_names = ReachabilityNameSet::from(["bucket".to_string()]);
        let top_level_names = item_names.clone();
        let top_level_types = ReceiverTypeMap::new();
        let top_level_field_types = ReceiverFieldTypeMap::new();
        let top_level_element_types = ReceiverTypeMap::new();
        let top_level_return_types = ReceiverTypeMap::new();
        let top_level_tuple_return_types = ReceiverTupleReturnMap::new();
        let context = RefCollectionContext {
            module_names: &module_names,
            item_names: &item_names,
            top_level_names: &top_level_names,
            top_level_types: &top_level_types,
            top_level_field_types: &top_level_field_types,
            top_level_element_types: &top_level_element_types,
            top_level_return_types: &top_level_return_types,
            top_level_tuple_return_types: &top_level_tuple_return_types,
        };
        let mut item: syn::Item = syn::parse_quote! {
            fn update(mut receiver: bucket) {
                <bucket>::fill(&mut receiver);
            }
        };

        let (names, external_refs) = collect_refs_from_item(&mut item, &context);

        assert!(external_refs.is_empty(), "{external_refs:?}");
        assert!(names.contains("bucket"), "{names:?}");
        assert!(
            names.contains(&impl_method_reachability_name("bucket", "fill")),
            "{names:?}"
        );
    }
}
