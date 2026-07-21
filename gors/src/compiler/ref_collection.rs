use super::{
    item_reachability::{impl_method_reachability_name, trait_impl_reachability_name},
    receiver_type_facts::{
        ReceiverFieldTypeMap, ReceiverTupleReturnMap, ReceiverTupleTypes, ReceiverTypeMap,
        ReceiverTypeRef, external_receiver_method_return_type,
        receiver_type_from_associated_call_path, receiver_type_from_init_expr,
        receiver_type_from_type, specialize_self_receiver_type,
        transparent_receiver_constructor_arg,
    },
    syn_inspect::{
        is_path_call_expr, is_transparent_receiver_method, item_macro_name, macro_token_item_names,
        named_self_type, pat_ident_name, pat_ident_names, path_is, self_type_reachability_names,
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

    #[derive(Clone, Default)]
    struct BoundScope {
        names: std::collections::HashSet<String>,
        types: std::collections::HashMap<String, ReceiverTypeRef>,
    }

    fn closure_expr_from_call_func(expr: &syn::Expr) -> Option<&syn::ExprClosure> {
        match expr {
            syn::Expr::Closure(closure) => Some(closure),
            syn::Expr::Group(group) => closure_expr_from_call_func(&group.expr),
            syn::Expr::Paren(paren) => closure_expr_from_call_func(&paren.expr),
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
            syn::Expr::MethodCall(method) if is_transparent_receiver_method(&method.method) => {
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
        top_level_tuple_return_types: &'a ReceiverTupleReturnMap,
        bound_scopes: Vec<BoundScope>,
        current_self_type: Option<ReceiverTypeRef>,
        current_self_reachability_names: Vec<String>,
        local_names: std::collections::HashSet<String>,
        external_refs: std::collections::HashMap<String, std::collections::HashSet<String>>,
    }

    impl RefCollector<'_> {
        fn receiver_type_from_expr(&self, expr: &syn::Expr) -> Option<ReceiverTypeRef> {
            let mut scopes = self.bound_scopes.clone();
            self.receiver_type_from_expr_in_scopes(expr, &mut scopes)
        }

        fn receiver_type_from_expr_in_scopes(
            &self,
            expr: &syn::Expr,
            scopes: &mut Vec<BoundScope>,
        ) -> Option<ReceiverTypeRef> {
            match expr {
                syn::Expr::Block(block) => self.receiver_type_from_block(&block.block, scopes),
                syn::Expr::Group(group) => {
                    self.receiver_type_from_expr_in_scopes(&group.expr, scopes)
                }
                syn::Expr::Paren(paren) => {
                    self.receiver_type_from_expr_in_scopes(&paren.expr, scopes)
                }
                syn::Expr::Path(path)
                    if path.path.leading_colon.is_none() && path.path.segments.len() == 1 =>
                {
                    let name = path.path.segments.first()?.ident.to_string();
                    if name == "self" {
                        return self.current_self_type.clone();
                    }
                    if Self::is_bound_in_scopes(scopes, &name) {
                        Self::bound_receiver_type_in_scopes(scopes, &name)
                    } else {
                        self.top_level_types.get(&name).cloned()
                    }
                }
                syn::Expr::Call(call) => self
                    .receiver_type_from_iife_call(call, scopes)
                    .or_else(|| {
                        transparent_receiver_constructor_arg(call)
                            .and_then(|arg| self.receiver_type_from_expr_in_scopes(arg, scopes))
                    })
                    .or_else(|| {
                        receiver_type_from_init_expr(
                            expr,
                            self.module_names,
                            self.item_names,
                            self.top_level_return_types,
                        )
                    })
                    .or_else(|| {
                        if is_path_call_expr(&call.func, &["std", "mem", "take"]) {
                            call.args
                                .first()
                                .and_then(|arg| self.receiver_type_from_expr_in_scopes(arg, scopes))
                        } else {
                            None
                        }
                    })
                    .or_else(|| self.receiver_type_from_expr_in_scopes(&call.func, scopes)),
                syn::Expr::MethodCall(method) if is_transparent_receiver_method(&method.method) => {
                    self.receiver_type_from_expr_in_scopes(&method.receiver, scopes)
                }
                syn::Expr::MethodCall(method) => {
                    let receiver_type =
                        self.receiver_type_from_expr_in_scopes(&method.receiver, scopes)?;
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
                syn::Expr::Cast(cast) => self.receiver_type_from_expr_in_scopes(&cast.expr, scopes),
                syn::Expr::Field(field) => {
                    let base_type = self.receiver_type_from_expr_in_scopes(&field.base, scopes)?;
                    let syn::Member::Named(member) = &field.member else {
                        return None;
                    };
                    self.top_level_field_types
                        .get(&base_type.name)
                        .and_then(|fields| fields.get(&member.to_string()))
                        .cloned()
                }
                syn::Expr::Index(index) => {
                    let base_type = self.receiver_type_from_expr_in_scopes(&index.expr, scopes)?;
                    self.top_level_element_types.get(&base_type.name).cloned()
                }
                syn::Expr::Reference(reference) => {
                    self.receiver_type_from_expr_in_scopes(&reference.expr, scopes)
                }
                syn::Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Deref(_)) => {
                    self.receiver_type_from_expr_in_scopes(&unary.expr, scopes)
                }
                _ => None,
            }
        }

        fn receiver_type_from_block(
            &self,
            block: &syn::Block,
            scopes: &mut Vec<BoundScope>,
        ) -> Option<ReceiverTypeRef> {
            scopes.push(BoundScope::default());
            let mut result = None;
            for (index, stmt) in block.stmts.iter().enumerate() {
                match stmt {
                    syn::Stmt::Local(local) => self.record_local_in_scopes(local, scopes),
                    syn::Stmt::Expr(expr, None) if index + 1 == block.stmts.len() => {
                        result = self.receiver_type_from_expr_in_scopes(expr, scopes);
                    }
                    _ => {}
                }
            }
            scopes.pop();
            result
        }

        fn receiver_type_from_iife_call(
            &self,
            call: &syn::ExprCall,
            scopes: &mut Vec<BoundScope>,
        ) -> Option<ReceiverTypeRef> {
            if !call.args.is_empty() {
                return None;
            }
            let closure = closure_expr_from_call_func(&call.func)?;
            scopes.push(BoundScope::default());
            for input in &closure.inputs {
                self.record_closure_input_in_scopes(input, scopes);
            }
            let result = self.receiver_type_from_expr_in_scopes(&closure.body, scopes);
            scopes.pop();
            result
        }

        fn receiver_tuple_types_from_expr_in_scopes(
            &self,
            expr: &syn::Expr,
            scopes: &mut Vec<BoundScope>,
        ) -> Option<ReceiverTupleTypes> {
            match expr {
                syn::Expr::Block(block) => {
                    self.receiver_tuple_types_from_block(&block.block, scopes)
                }
                syn::Expr::Call(call) => {
                    if let syn::Expr::Path(path) = &*call.func
                        && let Some(first) = path.path.segments.first()
                        && let Some(types) = self
                            .top_level_tuple_return_types
                            .get(&first.ident.to_string())
                    {
                        return Some(types.clone());
                    }
                    self.receiver_tuple_types_from_iife_call(call, scopes)
                        .or_else(|| {
                            self.receiver_tuple_types_from_expr_in_scopes(&call.func, scopes)
                        })
                }
                syn::Expr::Cast(cast) => {
                    self.receiver_tuple_types_from_expr_in_scopes(&cast.expr, scopes)
                }
                syn::Expr::Group(group) => {
                    self.receiver_tuple_types_from_expr_in_scopes(&group.expr, scopes)
                }
                syn::Expr::MethodCall(method) => {
                    let receiver_type =
                        self.receiver_type_from_expr_in_scopes(&method.receiver, scopes)?;
                    let method_key = impl_method_reachability_name(
                        &receiver_type.name,
                        &method.method.to_string(),
                    );
                    self.top_level_tuple_return_types.get(&method_key).cloned()
                }
                syn::Expr::Paren(paren) => {
                    self.receiver_tuple_types_from_expr_in_scopes(&paren.expr, scopes)
                }
                syn::Expr::Reference(reference) => {
                    self.receiver_tuple_types_from_expr_in_scopes(&reference.expr, scopes)
                }
                syn::Expr::Unary(unary) => {
                    self.receiver_tuple_types_from_expr_in_scopes(&unary.expr, scopes)
                }
                _ => None,
            }
        }

        fn receiver_tuple_types_from_block(
            &self,
            block: &syn::Block,
            scopes: &mut Vec<BoundScope>,
        ) -> Option<ReceiverTupleTypes> {
            scopes.push(BoundScope::default());
            let mut result = None;
            for (index, stmt) in block.stmts.iter().enumerate() {
                match stmt {
                    syn::Stmt::Local(local) => self.record_local_in_scopes(local, scopes),
                    syn::Stmt::Expr(expr, None) if index + 1 == block.stmts.len() => {
                        result = self.receiver_tuple_types_from_expr_in_scopes(expr, scopes);
                    }
                    _ => {}
                }
            }
            scopes.pop();
            result
        }

        fn receiver_tuple_types_from_iife_call(
            &self,
            call: &syn::ExprCall,
            scopes: &mut Vec<BoundScope>,
        ) -> Option<ReceiverTupleTypes> {
            if !call.args.is_empty() {
                return None;
            }
            let closure = closure_expr_from_call_func(&call.func)?;
            scopes.push(BoundScope::default());
            for input in &closure.inputs {
                self.record_closure_input_in_scopes(input, scopes);
            }
            let result = self.receiver_tuple_types_from_expr_in_scopes(&closure.body, scopes);
            scopes.pop();
            result
        }

        fn is_bound_in_scopes(scopes: &[BoundScope], name: &str) -> bool {
            scopes.iter().rev().any(|scope| scope.names.contains(name))
        }

        fn bound_receiver_type_in_scopes(
            scopes: &[BoundScope],
            name: &str,
        ) -> Option<ReceiverTypeRef> {
            for scope in scopes.iter().rev() {
                if scope.names.contains(name) {
                    return scope.types.get(name).cloned();
                }
            }
            None
        }

        fn bind_names_in_scopes(
            scopes: &mut [BoundScope],
            names: impl IntoIterator<Item = String>,
            types: std::collections::HashMap<String, ReceiverTypeRef>,
        ) {
            let Some(scope) = scopes.last_mut() else {
                return;
            };
            for name in names {
                scope.names.insert(name.clone());
                scope.types.remove(&name);
                if let Some(receiver_type) = types.get(&name) {
                    scope.types.insert(name, receiver_type.clone());
                }
            }
        }

        fn record_fn_arg_in_current_scope(&mut self, arg: &syn::FnArg) {
            let syn::FnArg::Typed(pat_type) = arg else {
                return;
            };
            let mut types = std::collections::HashMap::new();
            if let Some(name) = pat_ident_name(&pat_type.pat)
                && let Some(receiver_type) =
                    receiver_type_from_type(&pat_type.ty, self.module_names)
            {
                types.insert(
                    name,
                    specialize_self_receiver_type(receiver_type, self.current_self_type.as_ref()),
                );
            }
            Self::bind_names_in_scopes(
                &mut self.bound_scopes,
                pat_ident_names(&pat_type.pat),
                types,
            );
        }

        fn record_closure_input_in_scopes(&self, input: &syn::Pat, scopes: &mut [BoundScope]) {
            let mut types = std::collections::HashMap::new();
            if let syn::Pat::Type(pat_type) = input
                && let Some(name) = pat_ident_name(&pat_type.pat)
                && let Some(receiver_type) =
                    receiver_type_from_type(&pat_type.ty, self.module_names)
            {
                types.insert(
                    name,
                    specialize_self_receiver_type(receiver_type, self.current_self_type.as_ref()),
                );
            }
            Self::bind_names_in_scopes(scopes, pat_ident_names(input), types);
        }

        fn record_closure_input_in_current_scope(&mut self, input: &syn::Pat) {
            let mut scopes = std::mem::take(&mut self.bound_scopes);
            self.record_closure_input_in_scopes(input, &mut scopes);
            self.bound_scopes = scopes;
        }

        fn record_local_in_scopes(&self, local: &syn::Local, scopes: &mut Vec<BoundScope>) {
            let mut types = std::collections::HashMap::new();

            if let Some(init) = &local.init
                && let syn::Pat::Tuple(tuple_pat) = &local.pat
                && let Some(tuple_types) =
                    self.receiver_tuple_types_from_expr_in_scopes(&init.expr, scopes)
            {
                for (pat, receiver_type) in tuple_pat.elems.iter().zip(tuple_types) {
                    if let Some(name) = pat_ident_name(pat)
                        && let Some(receiver_type) = receiver_type
                    {
                        types.insert(name, receiver_type);
                    }
                }
            }

            if let syn::Pat::Type(pat_type) = &local.pat
                && let Some(name) = pat_ident_name(&pat_type.pat)
                && let Some(receiver_type) =
                    receiver_type_from_type(&pat_type.ty, self.module_names)
            {
                types.insert(
                    name,
                    specialize_self_receiver_type(receiver_type, self.current_self_type.as_ref()),
                );
            } else if let Some(init) = &local.init
                && let Some(name) = pat_ident_name(&local.pat)
                && let Some(receiver_type) = self
                    .receiver_type_from_expr_in_scopes(&init.expr, scopes)
                    .or_else(|| {
                        receiver_type_from_init_expr(
                            &init.expr,
                            self.module_names,
                            self.item_names,
                            self.top_level_return_types,
                        )
                    })
            {
                types.insert(
                    name,
                    specialize_self_receiver_type(receiver_type, self.current_self_type.as_ref()),
                );
            }

            Self::bind_names_in_scopes(scopes, pat_ident_names(&local.pat), types);
        }

        fn record_local_in_current_scope(&mut self, local: &syn::Local) {
            let mut scopes = std::mem::take(&mut self.bound_scopes);
            self.record_local_in_scopes(local, &mut scopes);
            self.bound_scopes = scopes;
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
        fn visit_item_fn_mut(&mut self, func: &mut syn::ItemFn) {
            self.bound_scopes.push(BoundScope::default());
            for arg in &func.sig.inputs {
                self.record_fn_arg_in_current_scope(arg);
            }
            syn::visit_mut::visit_item_fn_mut(self, func);
            self.bound_scopes.pop();
        }

        fn visit_impl_item_fn_mut(&mut self, func: &mut syn::ImplItemFn) {
            self.bound_scopes.push(BoundScope::default());
            for arg in &func.sig.inputs {
                self.record_fn_arg_in_current_scope(arg);
            }
            syn::visit_mut::visit_impl_item_fn_mut(self, func);
            self.bound_scopes.pop();
        }

        fn visit_trait_item_fn_mut(&mut self, func: &mut syn::TraitItemFn) {
            self.bound_scopes.push(BoundScope::default());
            for arg in &func.sig.inputs {
                self.record_fn_arg_in_current_scope(arg);
            }
            syn::visit_mut::visit_trait_item_fn_mut(self, func);
            self.bound_scopes.pop();
        }

        fn visit_block_mut(&mut self, block: &mut syn::Block) {
            self.bound_scopes.push(BoundScope::default());
            syn::visit_mut::visit_block_mut(self, block);
            self.bound_scopes.pop();
        }

        fn visit_expr_closure_mut(&mut self, closure: &mut syn::ExprClosure) {
            self.bound_scopes.push(BoundScope::default());
            for input in &closure.inputs {
                self.record_closure_input_in_current_scope(input);
            }
            syn::visit_mut::visit_expr_closure_mut(self, closure);
            self.bound_scopes.pop();
        }

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
            if self.item_names.contains(&name)
                && !Self::is_bound_in_scopes(&self.bound_scopes, &name)
            {
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
            if is_transparent_receiver_method(&method.method) {
                syn::visit_mut::visit_expr_method_call_mut(self, method);
                return;
            }
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
            for attr in &mut local.attrs {
                self.visit_attribute_mut(attr);
            }
            self.visit_pat_mut(&mut local.pat);
            if let Some(init) = &mut local.init {
                self.visit_expr_mut(&mut init.expr);
                if let Some((_, diverge)) = &mut init.diverge {
                    self.visit_expr_mut(diverge);
                }
            }
            self.record_local_in_current_scope(local);
        }
    }

    let mut collector = RefCollector {
        module_names: context.module_names,
        item_names: context.item_names,
        top_level_names: context.top_level_names,
        top_level_types: context.top_level_types,
        top_level_field_types: context.top_level_field_types,
        top_level_element_types: context.top_level_element_types,
        top_level_return_types: context.top_level_return_types,
        top_level_tuple_return_types: context.top_level_tuple_return_types,
        bound_scopes: vec![BoundScope::default()],
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
            | "as_ref"
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
            | "lock"
            | "new"
            | "push"
            | "std"
            | "to_string"
            | "true"
            | "u8"
            | "unwrap"
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
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    fn collect_function_refs(
        file: &syn::File,
        module_names: &ReachabilityNameSet,
        function: &str,
    ) -> (
        std::collections::HashSet<String>,
        std::collections::HashMap<String, std::collections::HashSet<String>>,
    ) {
        let item_names = super::super::reachability_names::item_reachability_names(&file.items);
        let top_level_names = super::super::reachability_names::top_level_item_names(&file.items);
        let top_level_types =
            super::super::receiver_type_facts::top_level_item_types(&file.items, module_names);
        let top_level_field_types = super::super::receiver_type_facts::top_level_item_field_types(
            &file.items,
            module_names,
        );
        let top_level_element_types =
            super::super::receiver_type_facts::top_level_collection_element_types(
                &file.items,
                module_names,
            );
        let top_level_return_types = super::super::receiver_type_facts::top_level_item_return_types(
            &file.items,
            module_names,
        );
        let top_level_tuple_return_types =
            super::super::receiver_type_facts::top_level_item_tuple_return_types(
                &file.items,
                module_names,
            );
        let context = RefCollectionContext {
            module_names,
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
            .find(|item| matches!(item, syn::Item::Fn(func) if func.sig.ident == function))
            .cloned()
            .expect("function item");
        collect_refs_from_item(&mut item, &context)
    }

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
    fn collect_refs_follows_method_calls_inside_saved_method_value_closures() {
        let module_names = ReachabilityNameSet::new();
        let file: syn::File = syn::parse_quote! {
            pub struct Counter;

            impl Counter {
                pub fn Read(&self) -> isize {
                    7
                }
            }

            fn root(counter: Counter) {
                let saved = {
                    let receiver = counter.clone();
                    move || receiver.Read()
                };
                let _ = saved();
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
            .expect("root function item");

        let (names, external_refs) = collect_refs_from_item(&mut item, &context);

        assert!(external_refs.is_empty(), "{external_refs:?}");
        assert!(names.contains("Counter"), "{names:?}");
        assert!(
            names.contains(&impl_method_reachability_name("Counter", "Read")),
            "{names:?}"
        );
    }

    #[test]
    fn collect_refs_follows_external_receiver_methods_through_block_values() {
        let module_names = ReachabilityNameSet::from(["builtin".to_string()]);
        let file: syn::File = syn::parse_quote! {
            struct Holder {
                items: crate::builtin::GorsMap<String, String>,
            }

            struct Other;

            impl Other {
                fn touch(&self) {}
            }

            fn root(holder: Holder, other: Other) {
                let _ = ({
                    let items = holder.items.clone();
                    items
                }).is_nil();
                let items = other;
                items.touch();
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

        let (_names, external_refs) = collect_refs_from_item(&mut item, &context);
        let builtin_refs = external_refs.get("builtin").expect("builtin references");

        assert!(builtin_refs.contains("GorsMap"), "{builtin_refs:?}");
        assert!(
            builtin_refs.contains(&impl_method_reachability_name("GorsMap", "is_nil")),
            "{builtin_refs:?}"
        );
        assert!(
            _names.contains(&impl_method_reachability_name("Other", "touch")),
            "{_names:?}"
        );
    }

    #[test]
    fn collect_refs_keeps_sibling_block_receiver_bindings_separate() {
        let module_names = ReachabilityNameSet::new();
        let file: syn::File = syn::parse_quote! {
            struct Left;
            struct Right;

            impl Left {
                fn read(&self) {}
            }

            impl Right {
                fn write(&self) {}
            }

            fn root(left: Left, right: Right) {
                {
                    let receiver = left;
                    receiver.read();
                }
                {
                    let receiver = right;
                    receiver.write();
                }
            }
        };

        let (names, external_refs) = collect_function_refs(&file, &module_names, "root");

        assert!(external_refs.is_empty(), "{external_refs:?}");
        assert!(
            names.contains(&impl_method_reachability_name("Left", "read")),
            "{names:?}"
        );
        assert!(
            names.contains(&impl_method_reachability_name("Right", "write")),
            "{names:?}"
        );
        assert!(
            !names.contains(&impl_method_reachability_name("Right", "read")),
            "{names:?}"
        );
        assert!(
            !names.contains(&impl_method_reachability_name("Left", "write")),
            "{names:?}"
        );
    }

    #[test]
    fn collect_refs_restores_receiver_binding_after_closure_parameter_shadowing() {
        let module_names = ReachabilityNameSet::new();
        let file: syn::File = syn::parse_quote! {
            struct Outer;
            struct Inner;

            impl Outer {
                fn outside(&self) {}
            }

            impl Inner {
                fn inside(&self) {}
            }

            fn root(receiver: Outer) {
                let callback = |receiver: Inner| receiver.inside();
                receiver.outside();
                let _ = callback;
            }
        };

        let (names, external_refs) = collect_function_refs(&file, &module_names, "root");

        assert!(external_refs.is_empty(), "{external_refs:?}");
        assert!(
            names.contains(&impl_method_reachability_name("Inner", "inside")),
            "{names:?}"
        );
        assert!(
            names.contains(&impl_method_reachability_name("Outer", "outside")),
            "{names:?}"
        );
        assert!(
            !names.contains(&impl_method_reachability_name("Outer", "inside")),
            "{names:?}"
        );
        assert!(
            !names.contains(&impl_method_reachability_name("Inner", "outside")),
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
            .find(|item| {
                matches!(item, syn::Item::Impl(item_impl)
                if matches!(super::named_self_type(&item_impl.self_ty).as_deref(), Some("Writer")))
            })
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
    fn collect_refs_ignores_external_receiver_wrapper_methods() {
        let module_names = ReachabilityNameSet::from(["io".to_string()]);
        let file: syn::File = syn::parse_quote! {
            fn use_reader(mut reader: crate::io::Reader) {
                let _ = reader.clone().Read();
                let _ = reader.lock().unwrap().Read();
                let _ = reader.as_ref().Read();
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
        let mut item = file.items.first().cloned().expect("function item");

        let (_names, external_refs) = collect_refs_from_item(&mut item, &context);

        let io_refs = external_refs.get("io").expect("io refs");
        assert!(io_refs.contains("Reader"), "{external_refs:?}");
        assert!(
            io_refs.contains(&impl_method_reachability_name("Reader", "Read")),
            "{external_refs:?}"
        );
        for wrapper in ["as_ref", "clone", "lock", "unwrap"] {
            assert!(
                !io_refs.contains(&impl_method_reachability_name("Reader", wrapper)),
                "{external_refs:?}"
            );
        }
    }

    #[test]
    fn collect_refs_ignores_external_call_result_to_string_wrapper() {
        let module_names = ReachabilityNameSet::from(["path".to_string()]);
        let file: syn::File = syn::parse_quote! {
            fn use_path() {
                let _ = crate::path::Clean("a").to_string();
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
        let mut item = file.items.first().cloned().expect("function item");

        let (_names, external_refs) = collect_refs_from_item(&mut item, &context);

        let path_refs = external_refs.get("path").expect("path refs");
        assert!(path_refs.contains("Clean"), "{external_refs:?}");
        assert!(
            !path_refs.contains("to_string")
                && !path_refs.contains(&impl_method_reachability_name("Clean", "to_string")),
            "{external_refs:?}"
        );
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
