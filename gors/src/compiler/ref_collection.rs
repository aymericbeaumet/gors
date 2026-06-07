use super::{
    item_reachability::{impl_method_reachability_name, trait_impl_reachability_name},
    receiver_type_facts::{
        ReceiverFieldTypeMap, ReceiverTupleReturnMap, ReceiverTupleTypes, ReceiverTypeMap,
        ReceiverTypeRef, external_receiver_method_return_type,
        receiver_type_from_associated_call_path, receiver_type_from_init_expr,
        receiver_type_from_type, specialize_self_receiver_type,
    },
    syn_inspect::{
        is_path_call_expr, is_receiver_type_wrapper_method, item_macro_name,
        macro_token_item_names, named_self_type, pat_ident_name, pat_ident_names, path_is,
        self_type_reachability_names,
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
        fn specialize_receiver_type(&self, receiver_type: ReceiverTypeRef) -> ReceiverTypeRef {
            specialize_self_receiver_type(receiver_type, self.current_self_type.as_ref())
        }

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

        fn bound_receiver_tuple_types_from_expr(
            &self,
            expr: &syn::Expr,
        ) -> Option<ReceiverTupleTypes> {
            match expr {
                syn::Expr::Call(call) => {
                    if let syn::Expr::Path(path) = &*call.func
                        && let Some(first) = path.path.segments.first()
                        && let Some(types) = self
                            .top_level_tuple_return_types
                            .get(&first.ident.to_string())
                    {
                        return Some(types.clone());
                    }
                    self.bound_receiver_tuple_types_from_expr(&call.func)
                }
                syn::Expr::Cast(cast) => self.bound_receiver_tuple_types_from_expr(&cast.expr),
                syn::Expr::Group(group) => self.bound_receiver_tuple_types_from_expr(&group.expr),
                syn::Expr::MethodCall(method) => {
                    let receiver_type = self.bound_receiver_type_from_expr(&method.receiver)?;
                    let method_key = impl_method_reachability_name(
                        &receiver_type.name,
                        &method.method.to_string(),
                    );
                    self.top_level_tuple_return_types.get(&method_key).cloned()
                }
                syn::Expr::Paren(paren) => self.bound_receiver_tuple_types_from_expr(&paren.expr),
                syn::Expr::Reference(reference) => {
                    self.bound_receiver_tuple_types_from_expr(&reference.expr)
                }
                syn::Expr::Unary(unary) => self.bound_receiver_tuple_types_from_expr(&unary.expr),
                _ => None,
            }
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
                let ty = self.specialize_receiver_type(ty);
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
                    let ty = self.specialize_receiver_type(ty);
                    self.types.insert(name, ty);
                }
            }
            syn::visit_mut::visit_expr_closure_mut(self, closure);
        }

        fn visit_local_mut(&mut self, local: &mut syn::Local) {
            if let Some(init) = &local.init
                && let syn::Pat::Tuple(tuple_pat) = &local.pat
                && let Some(tuple_types) = self.bound_receiver_tuple_types_from_expr(&init.expr)
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
                let ty = self.specialize_receiver_type(ty);
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
                let ty = self.specialize_receiver_type(ty);
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
            syn::Expr::Path(path) => external_path_symbol_from_path(&path.path, module_names),
            _ => None,
        }
    }

    fn external_path_symbol_from_path(
        path: &syn::Path,
        module_names: &std::collections::HashSet<String>,
    ) -> Option<(String, String)> {
        let mut segments = path.segments.iter().map(|seg| seg.ident.to_string());
        match (
            segments.next().as_deref(),
            segments.next().as_deref(),
            segments.next().as_deref(),
        ) {
            (Some("crate"), Some(module), Some(symbol)) if module_names.contains(module) => {
                Some((module.to_string(), symbol.to_string()))
            }
            (Some(module), Some(symbol), _) if module_names.contains(module) => {
                Some((module.to_string(), symbol.to_string()))
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

        fn insert_trait_impl_ref(&mut self, trait_name: &str, receiver_type: ReceiverTypeRef) {
            if receiver_type.module.is_some() || !is_reachability_name(&receiver_type.name) {
                return;
            }
            self.local_names.insert(receiver_type.name.clone());
            self.local_names.insert(trait_impl_reachability_name(
                trait_name,
                &receiver_type.name,
            ));
        }

        fn receiver_type_from_trait_impl_source(
            &self,
            expr: &syn::Expr,
        ) -> Option<ReceiverTypeRef> {
            receiver_type_from_init_expr(
                expr,
                self.module_names,
                self.item_names,
                self.top_level_return_types,
            )
            .or_else(|| self.receiver_type_from_expr(expr))
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
                        entry.insert(impl_method_reachability_name(symbol, assoc));
                    }
                }
                (Some(module), Some(symbol), assoc, _) if self.module_names.contains(module) => {
                    let entry = self.external_refs.entry(module.to_string()).or_default();
                    entry.insert(symbol.to_string());
                    if let Some(assoc) = assoc {
                        entry.insert(impl_method_reachability_name(symbol, assoc));
                    }
                }
                (Some(local), Some(symbol), assoc, _) if self.item_names.contains(local) => {
                    self.local_names.insert(local.to_string());
                    self.local_names
                        .insert(impl_method_reachability_name(local, symbol));
                    if let Some(assoc) = assoc {
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
            if let Some((module, symbol)) =
                external_path_symbol_from_path(&type_path.path, self.module_names)
            {
                self.local_names.insert(format!("{module}::{symbol}"));
                self.external_refs.entry(module).or_default().insert(symbol);
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
                    self.local_names.insert(name.clone());
                }
                if path_is(path, &["std", "error", "Error"]) {
                    for self_name in self_type_reachability_names(&item_impl.self_ty) {
                        self.local_names
                            .insert(trait_impl_reachability_name("Display", &self_name));
                    }
                }
                for impl_item in &item_impl.items {
                    if let Some(member_name) = impl_item_member_name(impl_item) {
                        self.local_names
                            .insert(impl_method_reachability_name(&name, &member_name));
                    }
                }
                if let Some((module, symbol)) =
                    external_path_symbol_from_path(path, self.module_names)
                {
                    self.external_refs.entry(module).or_default().insert(symbol);
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
                if let Some((module, symbol)) =
                    external_path_symbol_from_path(&trait_bound.path, self.module_names)
                {
                    self.external_refs.entry(module).or_default().insert(symbol);
                }
            }
            syn::visit_mut::visit_type_param_bound_mut(self, bound);
        }

        fn visit_expr_cast_mut(&mut self, cast: &mut syn::ExprCast) {
            if let Some(trait_name) = boxed_trait_object_name(&cast.ty)
                && self.top_level_names.contains(&trait_name)
                && let Some(receiver_type) = self.receiver_type_from_trait_impl_source(&cast.expr)
            {
                self.insert_trait_impl_ref(&trait_name, receiver_type);
            }
            syn::visit_mut::visit_expr_cast_mut(self, cast);
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
                if path.path.leading_colon.is_none()
                    && path.path.segments.len() >= 2
                    && path.qself.is_none()
                    && let Some(trait_segment) = path.path.segments.first()
                    && let Some(receiver_expr) = call.args.first()
                {
                    let trait_name = trait_segment.ident.to_string();
                    if self.top_level_names.contains(&trait_name)
                        && let Some(receiver_type) =
                            self.receiver_type_from_trait_impl_source(receiver_expr)
                        && receiver_type.name != trait_name
                    {
                        self.insert_trait_impl_ref(&trait_name, receiver_type);
                    }
                }

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

        fn visit_local_mut(&mut self, local: &mut syn::Local) {
            if let Some(init) = &mut local.init {
                let removed = pat_ident_names(&local.pat)
                    .into_iter()
                    .filter(|name| self.bound_names.remove(name.as_str()))
                    .collect::<Vec<_>>();
                self.visit_expr_mut(&mut init.expr);
                for name in removed {
                    self.bound_names.insert(name);
                }
            }
            syn::visit_mut::visit_local_mut(self, local);
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

fn impl_item_member_name(item: &syn::ImplItem) -> Option<String> {
    match item {
        syn::ImplItem::Fn(func) => Some(func.sig.ident.to_string()),
        syn::ImplItem::Const(konst) => Some(konst.ident.to_string()),
        syn::ImplItem::Type(ty) => Some(ty.ident.to_string()),
        syn::ImplItem::Macro(item_macro) => item_macro
            .mac
            .path
            .segments
            .last()
            .map(|seg| seg.ident.to_string()),
        _ => None,
    }
}

fn boxed_trait_object_name(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Group(group) => boxed_trait_object_name(&group.elem),
        syn::Type::Paren(paren) => boxed_trait_object_name(&paren.elem),
        syn::Type::Path(type_path) => {
            let segment = type_path.path.segments.last()?;
            let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
                return None;
            };
            args.args.iter().find_map(|arg| match arg {
                syn::GenericArgument::Type(ty) => boxed_trait_object_name(ty),
                _ => None,
            })
        }
        syn::Type::Reference(reference) => boxed_trait_object_name(&reference.elem),
        syn::Type::TraitObject(trait_object) => trait_object.bounds.iter().find_map(|bound| {
            let syn::TypeParamBound::Trait(trait_bound) = bound else {
                return None;
            };
            trait_bound
                .path
                .segments
                .last()
                .map(|segment| segment.ident.to_string())
                .filter(|name| name != "Any")
        }),
        _ => None,
    }
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

    #[test]
    fn collect_refs_uses_qself_method_return_type_for_local_receivers() {
        let module_names = ReachabilityNameSet::new();
        let file: syn::File = syn::parse_quote! {
            pub struct headerGNU;
            pub struct sparseArray;

            impl headerGNU {
                fn sparse(&mut self) -> sparseArray {
                    sparseArray
                }
            }

            impl sparseArray {
                fn maxEntries(&mut self) -> isize {
                    0
                }
            }

            fn root(mut h: headerGNU) {
                let mut s = <headerGNU>::sparse(&mut h);
                let _ = (s).clone().maxEntries();
            }
        };
        let item_names = super::super::reachability_names::item_reachability_names(&file.items);
        let top_level_names = super::super::reachability_names::top_level_item_names(&file.items);
        let top_level_types =
            super::super::receiver_type_facts::top_level_item_types(&file.items, &module_names);
        let top_level_field_types = super::super::receiver_type_facts::top_level_item_field_types(
            &file.items,
            &module_names,
        );
        let top_level_element_types =
            super::super::receiver_type_facts::top_level_collection_element_types(
                &file.items,
                &module_names,
            );
        let top_level_return_types = super::super::receiver_type_facts::top_level_item_return_types(
            &file.items,
            &module_names,
        );
        let top_level_tuple_return_types =
            super::super::receiver_type_facts::top_level_item_tuple_return_types(
                &file.items,
                &module_names,
            );
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
        let mut item = file
            .items
            .iter()
            .find(|item| matches!(item, syn::Item::Fn(func) if func.sig.ident == "root"))
            .cloned()
            .expect("root function");

        let (names, external_refs) = collect_refs_from_item(&mut item, &context);

        assert!(external_refs.is_empty(), "{external_refs:?}");
        assert!(
            names.contains(&impl_method_reachability_name("headerGNU", "sparse")),
            "{names:?}"
        );
        assert!(
            names.contains(&impl_method_reachability_name("sparseArray", "maxEntries")),
            "{names:?}"
        );
        assert!(
            !names.contains(&impl_method_reachability_name("headerGNU", "maxEntries")),
            "{names:?}"
        );
    }

    #[test]
    fn collect_refs_resolves_self_receiver_fields_through_pointer_cells() {
        let module_names = ReachabilityNameSet::from(["time".to_string()]);
        let file: syn::File = syn::parse_quote! {
            pub struct Header {
                ModTime: crate::time::Time,
            }

            impl Header {
                fn allowedFormats(&mut self) {}
            }

            pub struct Writer {
                hdr: Header,
            }

            impl Writer {
                fn WriteHeader(mut tw: crate::builtin::GorsPtr<Self>) {
                    let _ = (std::mem::take(&mut (((tw).lock().unwrap()).hdr).ModTime))
                        .Round(crate::time::Second);
                    let _ = ((((tw).lock().unwrap()).hdr).clone()).allowedFormats();
                }
            }
        };
        let item_names = super::super::reachability_names::item_reachability_names(&file.items);
        let top_level_names = super::super::reachability_names::top_level_item_names(&file.items);
        let top_level_types =
            super::super::receiver_type_facts::top_level_item_types(&file.items, &module_names);
        let top_level_field_types = super::super::receiver_type_facts::top_level_item_field_types(
            &file.items,
            &module_names,
        );
        let top_level_element_types =
            super::super::receiver_type_facts::top_level_collection_element_types(
                &file.items,
                &module_names,
            );
        let top_level_return_types = super::super::receiver_type_facts::top_level_item_return_types(
            &file.items,
            &module_names,
        );
        let top_level_tuple_return_types =
            super::super::receiver_type_facts::top_level_item_tuple_return_types(
                &file.items,
                &module_names,
            );
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
        let mut item = file
            .items
            .iter()
            .find(|item| matches!(item, syn::Item::Impl(item_impl)
                if matches!(super::named_self_type(&item_impl.self_ty).as_deref(), Some("Writer"))))
            .cloned()
            .expect("Writer impl");

        let (names, external_refs) = collect_refs_from_item(&mut item, &context);

        assert!(
            names.contains(&impl_method_reachability_name("Header", "allowedFormats")),
            "{names:?}"
        );
        let time_refs = external_refs.get("time").expect("time refs");
        assert!(time_refs.contains("Time"), "{external_refs:?}");
        assert!(
            time_refs.contains(&impl_method_reachability_name("Time", "Round")),
            "{external_refs:?}"
        );
        assert!(time_refs.contains("Second"), "{external_refs:?}");
    }

    #[test]
    fn collect_refs_qualifies_local_associated_path_members() {
        let module_names = ReachabilityNameSet::new();
        let item_names = ReachabilityNameSet::from(["bucket".to_string(), "Read".to_string()]);
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
                bucket::Read(&mut receiver);
            }
        };

        let (names, external_refs) = collect_refs_from_item(&mut item, &context);

        assert!(external_refs.is_empty(), "{external_refs:?}");
        assert!(names.contains("bucket"), "{names:?}");
        assert!(
            names.contains(&impl_method_reachability_name("bucket", "Read")),
            "{names:?}"
        );
        assert!(!names.contains("Read"), "{names:?}");
    }

    #[test]
    fn collect_refs_qualifies_external_associated_path_members()
    -> Result<(), Box<dyn std::error::Error>> {
        let module_names = ReachabilityNameSet::from(["syscall".to_string()]);
        let item_names = ReachabilityNameSet::new();
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
            fn call_raw(mut value: Box<dyn crate::syscall::RawConn>) {
                crate::syscall::RawConn::Write(&mut *value, || true);
            }
        };

        let (_names, external_refs) = collect_refs_from_item(&mut item, &context);
        let syscall_refs = external_refs
            .get("syscall")
            .ok_or_else(|| std::io::Error::other(format!("syscall refs: {external_refs:?}")))?;

        assert!(syscall_refs.contains("RawConn"), "{external_refs:?}");
        assert!(
            syscall_refs.contains(&impl_method_reachability_name("RawConn", "Write")),
            "{external_refs:?}"
        );
        assert!(!syscall_refs.contains("Write"), "{external_refs:?}");
        Ok(())
    }

    #[test]
    fn collect_refs_follows_external_trait_impl_and_object_bounds() {
        let module_names = ReachabilityNameSet::from(["runtime".to_string()]);
        let item_names = ReachabilityNameSet::from(["Kind".to_string()]);
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
            impl crate::runtime::stringer for Kind {
                fn String(&mut self) -> String {
                    String::new()
                }

                fn __gors_clone_box(&self) -> Box<dyn crate::runtime::stringer> {
                    todo!()
                }
            }
        };

        let (_, external_refs) = collect_refs_from_item(&mut item, &context);

        assert_eq!(
            external_refs
                .get("runtime")
                .and_then(|refs| refs.get("stringer")),
            Some(&"stringer".to_string()),
            "{external_refs:?}"
        );
    }

    #[test]
    fn collect_refs_roots_local_trait_impl_pairs_from_boxed_trait_object_casts() {
        let module_names = ReachabilityNameSet::new();
        let item_names = ReachabilityNameSet::from([
            "Interface".to_string(),
            "Values".to_string(),
            "root".to_string(),
        ]);
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
            fn root() -> Box<dyn Interface> {
                Box::new(Values::default()) as Box<dyn Interface>
            }
        };

        let (names, external_refs) = collect_refs_from_item(&mut item, &context);

        assert!(external_refs.is_empty(), "{external_refs:?}");
        assert!(names.contains("Values"), "{names:?}");
        assert!(
            names.contains(&trait_impl_reachability_name("Interface", "Values")),
            "{names:?}"
        );
    }

    #[test]
    fn collect_refs_roots_local_trait_impl_pairs_from_trait_ufcs_calls() {
        let module_names = ReachabilityNameSet::new();
        let item_names = ReachabilityNameSet::from([
            "Interface".to_string(),
            "Values".to_string(),
            "root".to_string(),
        ]);
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
            fn root(mut value: Values) {
                Interface::Len(&mut value);
            }
        };

        let (names, external_refs) = collect_refs_from_item(&mut item, &context);

        assert!(external_refs.is_empty(), "{external_refs:?}");
        assert!(
            names.contains(&trait_impl_reachability_name("Interface", "Values")),
            "{names:?}"
        );
    }

    #[test]
    fn collect_refs_follows_tuple_return_types_from_impl_methods()
    -> Result<(), Box<dyn std::error::Error>> {
        let module_names = ReachabilityNameSet::new();
        let file: syn::File = syn::parse_quote! {
            pub struct Time;
            pub struct absSeconds;
            pub struct absDays;

            impl Time {
                fn locabs(&self) -> (String, isize, absSeconds) {
                    todo!()
                }

                fn append(&self) {
                    let (_, _, abs) = self.locabs();
                    let days = abs.days();
                    days.date();
                }
            }

            impl absSeconds {
                fn days(&self) -> absDays {
                    todo!()
                }
            }

            impl absDays {
                fn date(&self) {}
            }
        };
        let item_names = ReachabilityNameSet::from([
            "Time".to_string(),
            "Time::locabs".to_string(),
            "Time::append".to_string(),
            "absSeconds".to_string(),
            "absSeconds::days".to_string(),
            "absDays".to_string(),
            "absDays::date".to_string(),
        ]);
        let top_level_names = item_names.clone();
        let top_level_types =
            super::super::receiver_type_facts::top_level_item_types(&file.items, &module_names);
        let top_level_field_types = super::super::receiver_type_facts::top_level_item_field_types(
            &file.items,
            &module_names,
        );
        let top_level_element_types =
            super::super::receiver_type_facts::top_level_collection_element_types(
                &file.items,
                &module_names,
            );
        let top_level_return_types = super::super::receiver_type_facts::top_level_item_return_types(
            &file.items,
            &module_names,
        );
        let top_level_tuple_return_types =
            super::super::receiver_type_facts::top_level_item_tuple_return_types(
                &file.items,
                &module_names,
            );
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
        let mut item = file
            .items
            .iter()
            .find(|item| {
                matches!(item, syn::Item::Impl(item_impl)
                if matches!(super::named_self_type(&item_impl.self_ty).as_deref(), Some("Time")))
            })
            .cloned()
            .ok_or_else(|| std::io::Error::other("Time impl"))?;

        let (names, external_refs) = collect_refs_from_item(&mut item, &context);

        assert!(external_refs.is_empty(), "{external_refs:?}");
        assert!(
            names.contains(&impl_method_reachability_name("Time", "locabs")),
            "{names:?}"
        );
        assert!(
            names.contains(&impl_method_reachability_name("absSeconds", "days")),
            "{names:?}"
        );
        assert!(
            names.contains(&impl_method_reachability_name("absDays", "date")),
            "{names:?}"
        );
        Ok(())
    }

    #[test]
    fn collect_refs_follows_tuple_return_types_from_trait_methods()
    -> Result<(), Box<dyn std::error::Error>> {
        let module_names = ReachabilityNameSet::from(["time".to_string()]);
        let file: syn::File = syn::parse_quote! {
            pub trait Context {
                fn Deadline(&mut self) -> (crate::time::Time, bool);
            }

            fn root(ctx: &mut dyn Context, d: crate::time::Time) {
                let (cur, ok) = ctx.Deadline();
                if ok && cur.Before(d) {}
            }
        };
        let item_names = ReachabilityNameSet::from(["Context".to_string(), "root".to_string()]);
        let top_level_names = item_names.clone();
        let top_level_types =
            super::super::receiver_type_facts::top_level_item_types(&file.items, &module_names);
        let top_level_field_types = super::super::receiver_type_facts::top_level_item_field_types(
            &file.items,
            &module_names,
        );
        let top_level_element_types =
            super::super::receiver_type_facts::top_level_collection_element_types(
                &file.items,
                &module_names,
            );
        let top_level_return_types = super::super::receiver_type_facts::top_level_item_return_types(
            &file.items,
            &module_names,
        );
        let top_level_tuple_return_types =
            super::super::receiver_type_facts::top_level_item_tuple_return_types(
                &file.items,
                &module_names,
            );
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
        let mut item = file
            .items
            .iter()
            .find(|item| matches!(item, syn::Item::Fn(func) if func.sig.ident == "root"))
            .cloned()
            .ok_or_else(|| std::io::Error::other("root"))?;

        let (names, external_refs) = collect_refs_from_item(&mut item, &context);

        assert!(names.contains("Context"), "{names:?}");
        let time_refs = external_refs
            .get("time")
            .ok_or_else(|| std::io::Error::other("time refs"))?;
        assert!(time_refs.contains("Time"), "{time_refs:?}");
        assert!(
            time_refs.contains(&impl_method_reachability_name("Time", "Before")),
            "{time_refs:?}"
        );
        Ok(())
    }

    #[test]
    fn collect_refs_counts_initializer_item_before_same_name_local_binding() {
        let module_names = ReachabilityNameSet::new();
        let item_names = ReachabilityNameSet::from(["helper".to_string(), "entry".to_string()]);
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
            fn entry() {
                let mut helper = helper();
                helper.use_value();
            }
        };

        let (names, external_refs) = collect_refs_from_item(&mut item, &context);

        assert!(external_refs.is_empty(), "{external_refs:?}");
        assert!(names.contains("helper"), "{names:?}");
    }
}
