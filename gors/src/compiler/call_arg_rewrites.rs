use std::collections::{BTreeMap, BTreeSet};

use proc_macro2::Span;

use super::{
    CompiledModule, receiver_method_targets,
    receiver_type_facts::receiver_type_from_path,
    receiver_type_scopes,
    syn_inspect::{
        call_target_key, clone_call_receiver_expr, expr_path_ident, expr_path_ident_or_clone,
        is_box_dyn_any_type, is_box_leak_expr, is_lock_guard_wrapper_method, is_path_call_expr,
        is_slice_range_index_expr, named_self_type, pat_ident_name, slice_type_inner,
        vec_type_inner, zero_arg_method_call_receiver_expr,
    },
    synthetic_names,
};

#[derive(Clone, Copy)]
enum CloneValueParamKind {
    Clone,
    Take,
    Vec,
}

#[derive(Clone, PartialEq, Eq)]
enum MutRefParamKind {
    Plain,
    TraitObject { trait_name: String },
}

type ReceiverMethodArgTargets = receiver_method_targets::Targets<std::collections::HashSet<usize>>;
type MutRefParamTargets = BTreeMap<String, BTreeMap<usize, MutRefParamKind>>;
type ReceiverMethodMutRefTargets =
    receiver_method_targets::Targets<BTreeMap<usize, MutRefParamKind>>;
type TraitImplSet = BTreeSet<(String, String, String)>;
type CloneValueParamTargets = BTreeMap<String, BTreeMap<usize, CloneValueParamKind>>;
type ReceiverTraitSupertraits = receiver_method_targets::Supertraits;
type ReceiverMethodCloneValueTargets =
    receiver_method_targets::Targets<BTreeMap<usize, CloneValueParamKind>>;

trait ReceiverScopedCallArgRewrite {
    fn rewrite_expr_call(
        &mut self,
        _receiver_types: &receiver_type_scopes::Tracker<'_>,
        _call: &mut syn::ExprCall,
    ) {
    }

    fn rewrite_expr_method_call(
        &mut self,
        _receiver_types: &receiver_type_scopes::Tracker<'_>,
        _call: &mut syn::ExprMethodCall,
    ) {
    }
}

struct ReceiverScopedCallArgVisitor<'a, R> {
    receiver_types: receiver_type_scopes::Tracker<'a>,
    rewrite: R,
}

impl<R> syn::visit_mut::VisitMut for ReceiverScopedCallArgVisitor<'_, R>
where
    R: ReceiverScopedCallArgRewrite,
{
    fn visit_item_fn_mut(&mut self, func: &mut syn::ItemFn) {
        self.receiver_types.push_scope();
        syn::visit_mut::visit_item_fn_mut(self, func);
        self.receiver_types.pop_scope();
    }

    fn visit_item_impl_mut(&mut self, item_impl: &mut syn::ItemImpl) {
        let previous_self_type = self.receiver_types.enter_impl(item_impl);
        syn::visit_mut::visit_item_impl_mut(self, item_impl);
        self.receiver_types.restore_impl(previous_self_type);
    }

    fn visit_impl_item_fn_mut(&mut self, func: &mut syn::ImplItemFn) {
        self.receiver_types.push_scope();
        syn::visit_mut::visit_impl_item_fn_mut(self, func);
        self.receiver_types.pop_scope();
    }

    fn visit_block_mut(&mut self, block: &mut syn::Block) {
        self.receiver_types.push_scope();
        syn::visit_mut::visit_block_mut(self, block);
        self.receiver_types.pop_scope();
    }

    fn visit_fn_arg_mut(&mut self, arg: &mut syn::FnArg) {
        self.receiver_types.record_fn_arg(arg);
        syn::visit_mut::visit_fn_arg_mut(self, arg);
    }

    fn visit_local_mut(&mut self, local: &mut syn::Local) {
        syn::visit_mut::visit_local_mut(self, local);
        self.receiver_types.record_local(local);
    }

    fn visit_expr_call_mut(&mut self, call: &mut syn::ExprCall) {
        syn::visit_mut::visit_expr_call_mut(self, call);
        self.rewrite.rewrite_expr_call(&self.receiver_types, call);
    }

    fn visit_expr_method_call_mut(&mut self, call: &mut syn::ExprMethodCall) {
        syn::visit_mut::visit_expr_method_call_mut(self, call);
        self.rewrite
            .rewrite_expr_method_call(&self.receiver_types, call);
    }
}

pub(super) fn borrow_mutated_vec_params(modules: &mut BTreeMap<String, CompiledModule>) {
    let receiver_facts = receiver_type_scopes::ProgramFacts::collect(modules);
    let mut targets = collect_mut_ref_vec_targets(modules);
    let mut method_targets = collect_mut_ref_vec_method_targets(modules);

    loop {
        if !targets.is_empty() || !method_targets.is_empty() {
            for module in modules.values_mut() {
                syn::visit_mut::VisitMut::visit_file_mut(
                    &mut ReceiverScopedCallArgVisitor {
                        receiver_types: receiver_facts.tracker(module.mod_name.clone()),
                        rewrite: BorrowMutatedVecCallArgs {
                            targets: &targets,
                            method_targets: &method_targets,
                        },
                    },
                    &mut module.file,
                );
            }
        }

        let mut changed = false;
        for module in modules.values_mut() {
            let module_name = module.mod_name.clone();
            for item in &mut module.file.items {
                let syn::Item::Fn(item_fn) = item else {
                    continue;
                };
                if return_type_is_vec(&item_fn.sig.output) {
                    continue;
                }
                let params = mutated_vec_param_indices(&item_fn.sig, &item_fn.block);
                if params.is_empty() {
                    continue;
                }
                let key = format!("{}::{}", module_name, item_fn.sig.ident);
                let indices = params.iter().map(|(index, _, _)| *index).collect();
                rewrite_vec_params_as_mut_refs(&mut item_fn.sig, &params);
                reborrow_mutated_vec_params(&mut item_fn.block, &params);
                if targets.insert(key, indices).is_none() {
                    changed = true;
                }
            }
            for item in &mut module.file.items {
                let syn::Item::Impl(item_impl) = item else {
                    continue;
                };
                if item_impl.trait_.is_some() {
                    continue;
                }
                for impl_item in &mut item_impl.items {
                    let syn::ImplItem::Fn(method) = impl_item else {
                        continue;
                    };
                    if return_type_is_vec(&method.sig.output) {
                        continue;
                    }
                    let params = mutated_vec_param_indices(&method.sig, &method.block);
                    if params.is_empty() {
                        continue;
                    }
                    let indices = params
                        .iter()
                        .filter_map(|(index, _, _)| index.checked_sub(1))
                        .collect();
                    rewrite_vec_params_as_mut_refs(&mut method.sig, &params);
                    reborrow_mutated_vec_params(&mut method.block, &params);
                    let Some(self_name) = named_self_type(&item_impl.self_ty) else {
                        continue;
                    };
                    if method_targets
                        .insert_receiver(
                            &module_name,
                            &self_name,
                            &method.sig.ident.to_string(),
                            indices,
                        )
                        .is_none()
                    {
                        changed = true;
                    }
                }
            }
        }

        if changed {
            method_targets.finalize_unambiguous_names();
        }

        if !changed {
            break;
        }
    }
}

fn collect_mut_ref_vec_targets(
    modules: &BTreeMap<String, CompiledModule>,
) -> BTreeMap<String, std::collections::HashSet<usize>> {
    let mut targets = BTreeMap::new();
    for module in modules.values() {
        for item in &module.file.items {
            let syn::Item::Fn(item_fn) = item else {
                continue;
            };
            let indices = mut_ref_vec_param_indices(&item_fn.sig);
            if indices.is_empty() {
                continue;
            }
            targets.insert(
                format!("{}::{}", module.mod_name, item_fn.sig.ident),
                indices,
            );
        }
    }
    targets
}

fn collect_mut_ref_vec_method_targets(
    modules: &BTreeMap<String, CompiledModule>,
) -> ReceiverMethodArgTargets {
    let mut targets = ReceiverMethodArgTargets::default();
    for module in modules.values() {
        for item in &module.file.items {
            let syn::Item::Impl(item_impl) = item else {
                continue;
            };
            targets.record_methods_seen(&module.mod_name, item_impl);
            if item_impl.trait_.is_some() {
                continue;
            }
            let Some(self_name) = named_self_type(&item_impl.self_ty) else {
                continue;
            };
            for impl_item in &item_impl.items {
                let syn::ImplItem::Fn(method) = impl_item else {
                    continue;
                };
                let indices = mut_ref_vec_param_indices(&method.sig)
                    .into_iter()
                    .filter_map(|index| index.checked_sub(1))
                    .collect::<std::collections::HashSet<_>>();
                if indices.is_empty() {
                    continue;
                }
                targets.insert_receiver(
                    &module.mod_name,
                    &self_name,
                    &method.sig.ident.to_string(),
                    indices,
                );
            }
        }
    }
    targets.finalize_unambiguous_names();
    targets
}

fn mut_ref_vec_param_indices(sig: &syn::Signature) -> std::collections::HashSet<usize> {
    sig.inputs
        .iter()
        .enumerate()
        .filter_map(|(index, input)| {
            let syn::FnArg::Typed(pat_type) = input else {
                return None;
            };
            mut_ref_vec_inner(&pat_type.ty).map(|_| index)
        })
        .collect()
}

fn mut_ref_vec_inner(ty: &syn::Type) -> Option<syn::Type> {
    let syn::Type::Reference(reference) = ty else {
        return None;
    };
    reference.mutability.as_ref()?;
    vec_type_inner(&reference.elem).or_else(|| slice_type_inner(&reference.elem))
}

fn return_type_is_vec(output: &syn::ReturnType) -> bool {
    let syn::ReturnType::Type(_, ty) = output else {
        return false;
    };
    vec_type_inner(ty).is_some()
}

fn mutated_vec_param_indices(
    sig: &syn::Signature,
    block: &syn::Block,
) -> Vec<(usize, syn::Ident, syn::Type)> {
    sig.inputs
        .iter()
        .enumerate()
        .filter_map(|(index, input)| {
            let syn::FnArg::Typed(pat_type) = input else {
                return None;
            };
            let syn::Pat::Ident(pat_ident) = &*pat_type.pat else {
                return None;
            };
            let inner = vec_type_inner(&pat_type.ty).or_else(|| mut_ref_vec_inner(&pat_type.ty))?;
            (body_mutates_vec_param(block, &pat_ident.ident)
                && !body_reassigns_param(block, &pat_ident.ident))
            .then(|| (index, pat_ident.ident.clone(), inner))
        })
        .collect()
}

fn body_reassigns_param(block: &syn::Block, ident: &syn::Ident) -> bool {
    struct Finder<'a> {
        ident: &'a syn::Ident,
        found: bool,
    }

    impl syn::visit::Visit<'_> for Finder<'_> {
        fn visit_expr_assign(&mut self, assign: &syn::ExprAssign) {
            if matches!(&*assign.left, syn::Expr::Path(path) if path.path.is_ident(self.ident)) {
                self.found = true;
                return;
            }
            syn::visit::visit_expr_assign(self, assign);
        }
    }

    let mut finder = Finder {
        ident,
        found: false,
    };
    syn::visit::Visit::visit_block(&mut finder, block);
    finder.found
}

fn body_mutates_vec_param(block: &syn::Block, ident: &syn::Ident) -> bool {
    struct Finder<'a> {
        ident: &'a syn::Ident,
        found: bool,
    }

    fn lhs_mutates_vec_param(expr: &syn::Expr, ident: &syn::Ident) -> bool {
        match expr {
            syn::Expr::Field(field) => lhs_mutates_vec_param(&field.base, ident),
            syn::Expr::Index(index) => {
                expr_base_is_ident(&index.expr, ident) || lhs_mutates_vec_param(&index.expr, ident)
            }
            syn::Expr::Paren(paren) => lhs_mutates_vec_param(&paren.expr, ident),
            syn::Expr::Tuple(tuple) => tuple
                .elems
                .iter()
                .any(|elem| lhs_mutates_vec_param(elem, ident)),
            _ => false,
        }
    }

    fn expr_base_is_ident(expr: &syn::Expr, ident: &syn::Ident) -> bool {
        match expr {
            syn::Expr::Path(path) => path.path.is_ident(ident),
            syn::Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Deref(_)) => {
                expr_base_is_ident(&unary.expr, ident)
            }
            syn::Expr::Field(field) => expr_base_is_ident(&field.base, ident),
            syn::Expr::Index(index) => expr_base_is_ident(&index.expr, ident),
            syn::Expr::Paren(paren) => expr_base_is_ident(&paren.expr, ident),
            _ => false,
        }
    }

    fn is_assign_binop(op: &syn::BinOp) -> bool {
        matches!(
            op,
            syn::BinOp::AddAssign(_)
                | syn::BinOp::SubAssign(_)
                | syn::BinOp::MulAssign(_)
                | syn::BinOp::DivAssign(_)
                | syn::BinOp::RemAssign(_)
                | syn::BinOp::BitXorAssign(_)
                | syn::BinOp::BitAndAssign(_)
                | syn::BinOp::BitOrAssign(_)
                | syn::BinOp::ShlAssign(_)
                | syn::BinOp::ShrAssign(_)
        )
    }

    impl syn::visit::Visit<'_> for Finder<'_> {
        fn visit_expr_call(&mut self, call: &syn::ExprCall) {
            if is_path_call_expr(&call.func, &["std", "mem", "take"]) {
                return;
            }
            syn::visit::visit_expr_call(self, call);
        }

        fn visit_expr_assign(&mut self, assign: &syn::ExprAssign) {
            if lhs_mutates_vec_param(&assign.left, self.ident) {
                self.found = true;
                return;
            }
            syn::visit::visit_expr_assign(self, assign);
        }

        fn visit_expr_binary(&mut self, binary: &syn::ExprBinary) {
            if is_assign_binop(&binary.op) && lhs_mutates_vec_param(&binary.left, self.ident) {
                self.found = true;
                return;
            }
            syn::visit::visit_expr_binary(self, binary);
        }

        fn visit_expr_reference(&mut self, reference: &syn::ExprReference) {
            if reference.mutability.is_some() && expr_base_is_ident(&reference.expr, self.ident) {
                self.found = true;
                return;
            }
            syn::visit::visit_expr_reference(self, reference);
        }
    }

    let mut finder = Finder {
        ident,
        found: false,
    };
    syn::visit::Visit::visit_block(&mut finder, block);
    finder.found
}

fn rewrite_vec_params_as_mut_refs(
    sig: &mut syn::Signature,
    params: &[(usize, syn::Ident, syn::Type)],
) {
    for (index, _, inner) in params {
        let Some(syn::FnArg::Typed(pat_type)) = sig.inputs.iter_mut().nth(*index) else {
            continue;
        };
        *pat_type.ty = syn::parse_quote! { &mut [#inner] };
    }
}

fn reborrow_mutated_vec_params(block: &mut syn::Block, params: &[(usize, syn::Ident, syn::Type)]) {
    struct Reborrow {
        names: std::collections::HashSet<String>,
    }

    impl syn::visit_mut::VisitMut for Reborrow {
        fn visit_expr_reference_mut(&mut self, reference: &mut syn::ExprReference) {
            syn::visit_mut::visit_expr_reference_mut(self, reference);
            if reference.mutability.is_none() {
                return;
            }
            let syn::Expr::Path(path) = &*reference.expr else {
                return;
            };
            let Some(ident) = path.path.get_ident() else {
                return;
            };
            if !self.names.contains(&ident.to_string()) {
                return;
            }
            *reference.expr = syn::parse_quote! { *#ident };
        }
    }

    let names = params
        .iter()
        .map(|(_, ident, _)| ident.to_string())
        .collect();
    syn::visit_mut::VisitMut::visit_block_mut(&mut Reborrow { names }, block);
}

struct BorrowMutatedVecCallArgs<'a> {
    targets: &'a BTreeMap<String, std::collections::HashSet<usize>>,
    method_targets: &'a ReceiverMethodArgTargets,
}

impl ReceiverScopedCallArgRewrite for BorrowMutatedVecCallArgs<'_> {
    fn rewrite_expr_call(
        &mut self,
        receiver_types: &receiver_type_scopes::Tracker<'_>,
        call: &mut syn::ExprCall,
    ) {
        let Some(key) = call_target_key(&call.func, receiver_types.module_name()) else {
            return;
        };
        let Some(indices) = self.targets.get(&key) else {
            return;
        };
        for (index, arg) in call.args.iter_mut().enumerate() {
            if indices.contains(&index) {
                borrow_mut_slice_call_arg(arg);
            }
        }
    }

    fn rewrite_expr_method_call(
        &mut self,
        receiver_types: &receiver_type_scopes::Tracker<'_>,
        call: &mut syn::ExprMethodCall,
    ) {
        let receiver_type = receiver_types.receiver_type_for_expr(&call.receiver);
        let Some(indices) = self.method_targets.target_for_call(
            receiver_types.module_name(),
            &call.method.to_string(),
            receiver_type.as_ref(),
        ) else {
            return;
        };
        for (index, arg) in call.args.iter_mut().enumerate() {
            if indices.contains(&index) {
                borrow_mut_slice_call_arg(arg);
            }
        }
    }
}

fn borrow_mut_vec_call_arg(arg: &mut syn::Expr) {
    if matches!(arg, syn::Expr::Reference(_)) {
        return;
    }
    if expr_yields_mut_ref(arg) {
        return;
    }
    if let Some(source) = cloned_lvalue_source(arg) {
        *arg = syn::parse_quote! { &mut #source };
        return;
    }
    if let Some(source) = cloned_lvalue_block_source(arg) {
        *arg = syn::parse_quote! { &mut #source };
        return;
    }
    if let Some(name) = expr_path_ident(arg) {
        if name == "self" {
            return;
        }
        let ident = syn::Ident::new(&name, Span::mixed_site());
        *arg = syn::parse_quote! { &mut #ident };
        return;
    }
    let inner = arg.clone();
    *arg = syn::parse_quote! { &mut #inner };
}

fn expr_yields_mut_ref(expr: &syn::Expr) -> bool {
    if is_box_leak_expr(expr) {
        return true;
    }
    let syn::Expr::Block(block) = expr else {
        return false;
    };
    block
        .block
        .stmts
        .last()
        .is_some_and(|stmt| matches!(stmt, syn::Stmt::Expr(expr, None) if is_box_leak_expr(expr)))
}

fn borrow_mut_slice_call_arg(arg: &mut syn::Expr) {
    if let syn::Expr::Reference(reference) = arg {
        if reference.mutability.is_some() {
            *reference.expr = strip_cloned_lvalue_slice_source((*reference.expr).clone());
        }
        return;
    }
    if is_box_leak_expr(arg) {
        return;
    }
    if let Some(receiver) = to_vec_receiver_expr(arg) {
        *arg = syn::parse_quote! { &mut #receiver };
        return;
    }
    if let Some(source) = cloned_lvalue_source(arg) {
        *arg = syn::parse_quote! { &mut #source };
        return;
    }
    if let Some(source) = cloned_lvalue_block_source(arg) {
        *arg = syn::parse_quote! { &mut #source };
        return;
    }
    if let Some(name) = expr_path_ident(arg) {
        if name == "self" {
            return;
        }
        let ident = syn::Ident::new(&name, Span::mixed_site());
        *arg = syn::parse_quote! { &mut *#ident };
        return;
    }
    if matches!(arg, syn::Expr::MethodCall(_) | syn::Expr::Call(_)) {
        let inner = arg.clone();
        *arg = syn::parse_quote! { &mut *#inner };
        return;
    }
    let inner = arg.clone();
    *arg = syn::parse_quote! { &mut #inner };
}

fn cloned_lvalue_source(expr: &syn::Expr) -> Option<syn::Expr> {
    let source = clone_call_receiver_expr(expr)?;
    expr_can_be_mutably_borrowed(&source).then_some(source)
}

fn cloned_lvalue_block_source(expr: &syn::Expr) -> Option<syn::Expr> {
    let block = match expr {
        syn::Expr::Block(block) => block,
        syn::Expr::Group(group) => return cloned_lvalue_block_source(&group.expr),
        syn::Expr::Paren(paren) => return cloned_lvalue_block_source(&paren.expr),
        _ => return None,
    };
    let [local_stmt, result_stmt] = block.block.stmts.as_slice() else {
        return None;
    };
    let syn::Stmt::Local(local) = local_stmt else {
        return None;
    };
    let ident = pat_ident_name(&local.pat)?;
    let init = local.init.as_ref()?;
    let source = clone_call_receiver_expr(&init.expr)?;
    let syn::Stmt::Expr(result, None) = result_stmt else {
        return None;
    };
    if expr_path_ident(result).as_deref() != Some(ident.as_str()) {
        return None;
    }
    expr_can_be_mutably_borrowed(&source).then_some(source)
}

fn expr_can_be_mutably_borrowed(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Field(field) => expr_can_be_mutably_borrowed(&field.base),
        syn::Expr::Group(group) => expr_can_be_mutably_borrowed(&group.expr),
        syn::Expr::Index(index) => expr_can_be_mutably_borrowed(&index.expr),
        syn::Expr::MethodCall(method) if is_lock_guard_wrapper_method(&method.method) => {
            expr_can_be_mutably_borrowed(&method.receiver)
        }
        syn::Expr::Paren(paren) => expr_can_be_mutably_borrowed(&paren.expr),
        syn::Expr::Path(path) => path.path.leading_colon.is_none() && path.path.segments.len() == 1,
        syn::Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Deref(_)) => true,
        _ => false,
    }
}

fn to_vec_receiver_expr(expr: &syn::Expr) -> Option<syn::Expr> {
    let receiver = zero_arg_method_call_receiver_expr(expr, "to_vec")?;
    Some(strip_cloned_lvalue_slice_source(receiver.clone()))
}

fn strip_cloned_lvalue_slice_source(expr: syn::Expr) -> syn::Expr {
    match expr {
        syn::Expr::Index(mut index) => {
            if let Some(source) = clone_call_receiver_expr(&index.expr)
                && expr_can_be_mutably_borrowed(&source)
            {
                index.expr = Box::new(source);
            } else if let Some(source) = cloned_lvalue_block_source(&index.expr) {
                index.expr = Box::new(source);
            } else {
                index.expr = Box::new(strip_cloned_lvalue_slice_source(*index.expr));
            }
            syn::Expr::Index(index)
        }
        syn::Expr::Paren(mut paren) => {
            paren.expr = Box::new(strip_cloned_lvalue_slice_source(*paren.expr));
            syn::Expr::Paren(paren)
        }
        syn::Expr::Group(mut group) => {
            group.expr = Box::new(strip_cloned_lvalue_slice_source(*group.expr));
            syn::Expr::Group(group)
        }
        other => other,
    }
}

pub(super) fn clone_vec_value_call_args(modules: &mut BTreeMap<String, CompiledModule>) {
    let receiver_facts = receiver_type_scopes::ProgramFacts::collect(modules);
    let targets = collect_vec_value_param_targets(modules);
    let method_targets = collect_vec_value_method_targets(modules, receiver_facts.module_names());
    let vec_newtypes = collect_vec_newtypes(modules);
    if targets.is_empty() && method_targets.is_empty() && vec_newtypes.is_empty() {
        return;
    }

    for module in modules.values_mut() {
        syn::visit_mut::VisitMut::visit_file_mut(
            &mut ReceiverScopedCallArgVisitor {
                receiver_types: receiver_facts.tracker(module.mod_name.clone()),
                rewrite: CloneVecValueCallArgs {
                    targets: &targets,
                    method_targets: &method_targets,
                    vec_newtypes: &vec_newtypes,
                },
            },
            &mut module.file,
        );
    }
}

fn collect_vec_value_param_targets(
    modules: &BTreeMap<String, CompiledModule>,
) -> CloneValueParamTargets {
    let mut targets = BTreeMap::new();
    for module in modules.values() {
        for item in &module.file.items {
            let syn::Item::Fn(item_fn) = item else {
                continue;
            };
            let kinds = clone_value_param_kinds(&item_fn.sig);
            if kinds.is_empty() {
                continue;
            }
            targets.insert(format!("{}::{}", module.mod_name, item_fn.sig.ident), kinds);
        }
    }
    targets
}

fn collect_vec_value_method_targets(
    modules: &BTreeMap<String, CompiledModule>,
    module_names: &std::collections::HashSet<String>,
) -> ReceiverMethodCloneValueTargets {
    let mut targets = ReceiverMethodCloneValueTargets::default();
    let mut supertraits: ReceiverTraitSupertraits = BTreeMap::new();
    for module in modules.values() {
        for item in &module.file.items {
            match item {
                syn::Item::Impl(item_impl) => {
                    let Some(seen_self_name) = named_self_type(&item_impl.self_ty) else {
                        continue;
                    };
                    for impl_item in &item_impl.items {
                        let syn::ImplItem::Fn(method) = impl_item else {
                            continue;
                        };
                        targets.record_method_seen(
                            &module.mod_name,
                            &seen_self_name,
                            &method.sig.ident.to_string(),
                        );
                    }
                    if item_impl.trait_.is_some() {
                        continue;
                    }
                    for impl_item in &item_impl.items {
                        let syn::ImplItem::Fn(method) = impl_item else {
                            continue;
                        };
                        let kinds = method_arg_targets(clone_value_param_kinds(&method.sig));
                        if kinds.is_empty() {
                            continue;
                        }
                        targets.insert_receiver(
                            &module.mod_name,
                            &seen_self_name,
                            &method.sig.ident.to_string(),
                            kinds,
                        );
                    }
                }
                syn::Item::Trait(item_trait) => {
                    let self_name = item_trait.ident.to_string();
                    record_direct_supertraits(
                        &mut supertraits,
                        &module.mod_name,
                        &self_name,
                        item_trait,
                        module_names,
                    );
                    for trait_item in &item_trait.items {
                        let syn::TraitItem::Fn(method) = trait_item else {
                            continue;
                        };
                        targets.record_method_seen(
                            &module.mod_name,
                            &self_name,
                            &method.sig.ident.to_string(),
                        );
                        let kinds = method_arg_targets(clone_value_param_kinds(&method.sig));
                        if kinds.is_empty() {
                            continue;
                        }
                        targets.insert_receiver(
                            &module.mod_name,
                            &self_name,
                            &method.sig.ident.to_string(),
                            kinds,
                        );
                    }
                }
                _ => {}
            }
        }
    }
    targets.inherit_supertrait_methods(&supertraits);
    targets.finalize_unambiguous_names();
    targets
}

fn record_direct_supertraits(
    supertraits: &mut ReceiverTraitSupertraits,
    module_name: &str,
    self_name: &str,
    item_trait: &syn::ItemTrait,
    module_names: &std::collections::HashSet<String>,
) {
    let direct_supertraits = item_trait
        .supertraits
        .iter()
        .filter_map(|bound| {
            let syn::TypeParamBound::Trait(trait_bound) = bound else {
                return None;
            };
            let mut receiver_type = receiver_type_from_path(&trait_bound.path, module_names)?;
            if receiver_type.module.is_none() {
                receiver_type.module = Some(module_name.to_string());
            }
            Some(receiver_type)
        })
        .collect::<Vec<_>>();
    if !direct_supertraits.is_empty() {
        supertraits.insert(
            (module_name.to_string(), self_name.to_string()),
            direct_supertraits,
        );
    }
}

fn method_arg_targets<T>(param_targets: BTreeMap<usize, T>) -> BTreeMap<usize, T> {
    param_targets
        .into_iter()
        .filter_map(|(index, target)| index.checked_sub(1).map(|arg_index| (arg_index, target)))
        .collect()
}

fn clone_value_param_kinds(sig: &syn::Signature) -> BTreeMap<usize, CloneValueParamKind> {
    let clone_type_params: std::collections::HashSet<String> = sig
        .generics
        .params
        .iter()
        .filter_map(|param| {
            let syn::GenericParam::Type(type_param) = param else {
                return None;
            };
            type_param
                .bounds
                .iter()
                .any(|bound| {
                    matches!(bound, syn::TypeParamBound::Trait(trait_bound) if trait_bound.path.is_ident("Clone"))
                })
                .then(|| type_param.ident.to_string())
        })
        .collect();
    sig.inputs
        .iter()
        .enumerate()
        .filter_map(|(index, input)| {
            let syn::FnArg::Typed(pat_type) = input else {
                return None;
            };
            cloneable_value_param_kind(&pat_type.ty, &clone_type_params).map(|kind| (index, kind))
        })
        .collect()
}

fn cloneable_value_param_kind(
    ty: &syn::Type,
    clone_type_params: &std::collections::HashSet<String>,
) -> Option<CloneValueParamKind> {
    if matches!(ty, syn::Type::Reference(_)) {
        return None;
    }
    if let Some(inner) = vec_type_inner(ty) {
        return Some(if is_box_dyn_any_type(&inner) {
            CloneValueParamKind::Take
        } else {
            CloneValueParamKind::Vec
        });
    }
    let syn::Type::Path(type_path) = ty else {
        return None;
    };
    if type_path.qself.is_some() {
        return None;
    }
    let segment = type_path.path.segments.last()?;
    (matches!(segment.ident.to_string().as_str(), "String")
        || clone_type_params.contains(&segment.ident.to_string()))
    .then_some(CloneValueParamKind::Clone)
}

struct CloneVecValueCallArgs<'a> {
    targets: &'a CloneValueParamTargets,
    method_targets: &'a ReceiverMethodCloneValueTargets,
    vec_newtypes: &'a std::collections::HashSet<String>,
}

impl ReceiverScopedCallArgRewrite for CloneVecValueCallArgs<'_> {
    fn rewrite_expr_call(
        &mut self,
        receiver_types: &receiver_type_scopes::Tracker<'_>,
        call: &mut syn::ExprCall,
    ) {
        if let Some(type_key) = from_call_vec_newtype_key(&call.func, receiver_types.module_name())
            && self.vec_newtypes.contains(&type_key)
            && call.args.len() == 1
            && let Some(arg) = call.args.first_mut()
        {
            clone_value_arg(arg);
        }

        let Some(key) = call_target_key(&call.func, receiver_types.module_name()) else {
            return;
        };
        let Some(kinds) = self.targets.get(&key) else {
            return;
        };
        for (index, arg) in call.args.iter_mut().enumerate() {
            if let Some(kind) = kinds.get(&index) {
                normalize_vec_value_arg(arg, *kind);
            }
        }
    }

    fn rewrite_expr_method_call(
        &mut self,
        receiver_types: &receiver_type_scopes::Tracker<'_>,
        call: &mut syn::ExprMethodCall,
    ) {
        let receiver_type = receiver_types.receiver_type_for_expr(&call.receiver);
        let Some(kinds) = self.method_targets.target_for_call(
            receiver_types.module_name(),
            &call.method.to_string(),
            receiver_type.as_ref(),
        ) else {
            return;
        };
        for (index, arg) in call.args.iter_mut().enumerate() {
            if let Some(kind) = kinds.get(&index) {
                normalize_vec_value_arg(arg, *kind);
            }
        }
    }
}

fn normalize_vec_value_arg(arg: &mut syn::Expr, kind: CloneValueParamKind) {
    if let syn::Expr::Reference(reference) = arg
        && reference.mutability.is_none()
    {
        *arg = (*reference.expr).clone();
    }
    if matches!(kind, CloneValueParamKind::Take) {
        take_value_arg(arg);
        return;
    }
    if matches!(kind, CloneValueParamKind::Vec)
        && (is_slice_range_index_expr(arg) || matches!(arg, syn::Expr::Block(_)))
    {
        let inner = arg.clone();
        *arg = syn::parse_quote! { (#inner).to_vec() };
        return;
    }
    if matches!(kind, CloneValueParamKind::Vec) && take_deref_value_arg(arg) {
        return;
    }
    clone_value_arg(arg);
}

fn take_deref_value_arg(arg: &mut syn::Expr) -> bool {
    let Some(inner) = deref_lvalue_inner_expr(arg) else {
        return false;
    };
    *arg = syn::parse_quote! { std::mem::take(&mut *#inner) };
    true
}

fn deref_lvalue_inner_expr(expr: &syn::Expr) -> Option<syn::Expr> {
    match expr {
        syn::Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Deref(_)) => {
            takeable_deref_base(&unary.expr).then(|| (*unary.expr).clone())
        }
        syn::Expr::Paren(paren) => deref_lvalue_inner_expr(&paren.expr),
        syn::Expr::Group(group) => deref_lvalue_inner_expr(&group.expr),
        _ => None,
    }
}

fn takeable_deref_base(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Path(_) | syn::Expr::Field(_) | syn::Expr::Index(_) => true,
        syn::Expr::Paren(paren) => takeable_deref_base(&paren.expr),
        syn::Expr::Group(group) => takeable_deref_base(&group.expr),
        _ => false,
    }
}

fn take_value_arg(arg: &mut syn::Expr) {
    if !matches!(
        arg,
        syn::Expr::Path(_) | syn::Expr::Field(_) | syn::Expr::Index(_)
    ) {
        return;
    }
    if matches!(expr_path_ident(arg).as_deref(), Some("self")) {
        return;
    }
    let inner = arg.clone();
    *arg = syn::parse_quote! { std::mem::take(&mut #inner) };
}

fn clone_value_arg(arg: &mut syn::Expr) {
    if !matches!(
        arg,
        syn::Expr::Path(_) | syn::Expr::Field(_) | syn::Expr::Index(_)
    ) {
        return;
    }
    if matches!(expr_path_ident(arg).as_deref(), Some("self")) {
        return;
    }
    let inner = arg.clone();
    *arg = syn::parse_quote! { (#inner).clone() };
}

pub(super) fn borrow_mut_ref_call_args(modules: &mut BTreeMap<String, CompiledModule>) {
    let receiver_facts = receiver_type_scopes::ProgramFacts::collect(modules);
    let (targets, method_targets) = collect_mut_ref_targets(modules, receiver_facts.module_names());
    let (direct_trait_impls, mut_ref_trait_impls) = collect_trait_impl_self_kinds(modules);
    if targets.is_empty() && method_targets.is_empty() {
        return;
    }
    for module in modules.values_mut() {
        syn::visit_mut::VisitMut::visit_file_mut(
            &mut ReceiverScopedCallArgVisitor {
                receiver_types: receiver_facts.tracker(module.mod_name.clone()),
                rewrite: BorrowMutRefCallArgs {
                    targets: &targets,
                    method_targets: &method_targets,
                    direct_trait_impls: &direct_trait_impls,
                    mut_ref_trait_impls: &mut_ref_trait_impls,
                },
            },
            &mut module.file,
        );
    }
}

fn collect_mut_ref_targets(
    modules: &BTreeMap<String, CompiledModule>,
    module_names: &std::collections::HashSet<String>,
) -> (MutRefParamTargets, ReceiverMethodMutRefTargets) {
    let mut targets = BTreeMap::new();
    let mut method_targets = ReceiverMethodMutRefTargets::default();
    let mut supertraits: ReceiverTraitSupertraits = BTreeMap::new();
    for module in modules.values() {
        for item in &module.file.items {
            match item {
                syn::Item::Fn(item_fn) => {
                    let param_kinds = mut_ref_param_kinds(&item_fn.sig);
                    if param_kinds.is_empty() {
                        continue;
                    }
                    targets.insert(
                        format!("{}::{}", module.mod_name, item_fn.sig.ident),
                        param_kinds,
                    );
                }
                syn::Item::Impl(item_impl) => {
                    method_targets.record_methods_seen(&module.mod_name, item_impl);
                    if item_impl.trait_.is_some() {
                        continue;
                    }
                    let Some(self_name) = named_self_type(&item_impl.self_ty) else {
                        continue;
                    };
                    for item in &item_impl.items {
                        let syn::ImplItem::Fn(item_fn) = item else {
                            continue;
                        };
                        let param_kinds = method_arg_targets(mut_ref_param_kinds(&item_fn.sig));
                        if param_kinds.is_empty() {
                            continue;
                        }
                        method_targets.insert_receiver(
                            &module.mod_name,
                            &self_name,
                            &item_fn.sig.ident.to_string(),
                            param_kinds,
                        );
                    }
                }
                syn::Item::Trait(item_trait) => {
                    let self_name = item_trait.ident.to_string();
                    record_direct_supertraits(
                        &mut supertraits,
                        &module.mod_name,
                        &self_name,
                        item_trait,
                        module_names,
                    );
                    for item in &item_trait.items {
                        let syn::TraitItem::Fn(item_fn) = item else {
                            continue;
                        };
                        method_targets.record_method_seen(
                            &module.mod_name,
                            &self_name,
                            &item_fn.sig.ident.to_string(),
                        );
                        let param_kinds = method_arg_targets(mut_ref_param_kinds(&item_fn.sig));
                        if param_kinds.is_empty() {
                            continue;
                        }
                        method_targets.insert_receiver(
                            &module.mod_name,
                            &self_name,
                            &item_fn.sig.ident.to_string(),
                            param_kinds,
                        );
                    }
                }
                _ => {}
            }
        }
    }
    method_targets.inherit_supertrait_methods(&supertraits);
    method_targets.finalize_unambiguous_names();
    (targets, method_targets)
}

fn mut_ref_param_kinds(sig: &syn::Signature) -> BTreeMap<usize, MutRefParamKind> {
    sig.inputs
        .iter()
        .enumerate()
        .filter_map(|(index, input)| {
            let syn::FnArg::Typed(pat_type) = input else {
                return None;
            };
            let syn::Type::Reference(reference) = &*pat_type.ty else {
                return None;
            };
            reference.mutability.as_ref()?;
            Some((
                index,
                trait_object_name(&reference.elem)
                    .map(|trait_name| MutRefParamKind::TraitObject { trait_name })
                    .unwrap_or(MutRefParamKind::Plain),
            ))
        })
        .collect()
}

fn trait_object_name(ty: &syn::Type) -> Option<String> {
    let syn::Type::TraitObject(trait_object) = ty else {
        return None;
    };
    trait_object.bounds.iter().find_map(|bound| {
        let syn::TypeParamBound::Trait(trait_bound) = bound else {
            return None;
        };
        trait_bound
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string())
    })
}

fn collect_trait_impl_self_kinds(
    modules: &BTreeMap<String, CompiledModule>,
) -> (TraitImplSet, TraitImplSet) {
    let mut direct_impls = TraitImplSet::new();
    let mut mut_ref_impls = TraitImplSet::new();

    for module in modules.values() {
        for item in &module.file.items {
            let syn::Item::Impl(item_impl) = item else {
                continue;
            };
            let Some((_, trait_path, _)) = &item_impl.trait_ else {
                continue;
            };
            let Some(trait_name) = trait_path
                .segments
                .last()
                .map(|segment| segment.ident.to_string())
            else {
                continue;
            };

            if let Some(self_name) = direct_impl_self_name(&item_impl.self_ty) {
                direct_impls.insert((module.mod_name.clone(), self_name, trait_name.clone()));
            }
            if let Some(self_name) = mut_ref_impl_self_name(&item_impl.self_ty) {
                mut_ref_impls.insert((module.mod_name.clone(), self_name, trait_name));
            }
        }
    }

    (direct_impls, mut_ref_impls)
}

fn direct_impl_self_name(ty: &syn::Type) -> Option<String> {
    let syn::Type::Path(path) = ty else {
        return None;
    };
    path.path
        .segments
        .last()
        .map(|segment| segment.ident.to_string())
}

fn mut_ref_impl_self_name(ty: &syn::Type) -> Option<String> {
    let syn::Type::Reference(reference) = ty else {
        return None;
    };
    reference.mutability.as_ref()?;
    direct_impl_self_name(&reference.elem)
}

fn borrow_mut_ref_call_arg(
    arg: &mut syn::Expr,
    kind: &MutRefParamKind,
    receiver_types: &receiver_type_scopes::Tracker<'_>,
    direct_trait_impls: &TraitImplSet,
    mut_ref_trait_impls: &TraitImplSet,
) {
    if should_reborrow_self_for_mut_trait_object(
        arg,
        kind,
        receiver_types,
        direct_trait_impls,
        mut_ref_trait_impls,
    ) {
        *arg = syn::parse_quote! { &mut { let __gors_self = &mut *self; __gors_self } };
        return;
    }

    borrow_mut_vec_call_arg(arg);
}

fn should_reborrow_self_for_mut_trait_object(
    arg: &syn::Expr,
    kind: &MutRefParamKind,
    receiver_types: &receiver_type_scopes::Tracker<'_>,
    direct_trait_impls: &TraitImplSet,
    mut_ref_trait_impls: &TraitImplSet,
) -> bool {
    let MutRefParamKind::TraitObject { trait_name } = kind else {
        return false;
    };
    if expr_path_ident(arg).as_deref() != Some("self") {
        return false;
    }
    let Some(self_type) = receiver_types.current_self_type() else {
        return false;
    };
    let self_module = self_type
        .module
        .as_deref()
        .unwrap_or_else(|| receiver_types.module_name());
    let key = (
        self_module.to_string(),
        self_type.name.clone(),
        trait_name.clone(),
    );
    mut_ref_trait_impls.contains(&key) && !direct_trait_impls.contains(&key)
}

struct BorrowMutRefCallArgs<'a> {
    targets: &'a MutRefParamTargets,
    method_targets: &'a ReceiverMethodMutRefTargets,
    direct_trait_impls: &'a TraitImplSet,
    mut_ref_trait_impls: &'a TraitImplSet,
}

impl ReceiverScopedCallArgRewrite for BorrowMutRefCallArgs<'_> {
    fn rewrite_expr_call(
        &mut self,
        receiver_types: &receiver_type_scopes::Tracker<'_>,
        call: &mut syn::ExprCall,
    ) {
        let Some(key) = call_target_key(&call.func, receiver_types.module_name()) else {
            return;
        };
        let Some(param_kinds) = self.targets.get(&key) else {
            return;
        };
        for (index, arg) in call.args.iter_mut().enumerate() {
            if let Some(kind) = param_kinds.get(&index) {
                borrow_mut_ref_call_arg(
                    arg,
                    kind,
                    receiver_types,
                    self.direct_trait_impls,
                    self.mut_ref_trait_impls,
                );
            }
        }
    }

    fn rewrite_expr_method_call(
        &mut self,
        receiver_types: &receiver_type_scopes::Tracker<'_>,
        call: &mut syn::ExprMethodCall,
    ) {
        let receiver_type = receiver_types.receiver_type_for_expr(&call.receiver);
        let Some(param_kinds) = self.method_targets.target_for_call(
            receiver_types.module_name(),
            &call.method.to_string(),
            receiver_type.as_ref(),
        ) else {
            return;
        };
        for (index, arg) in call.args.iter_mut().enumerate() {
            if let Some(kind) = param_kinds.get(&index) {
                borrow_mut_ref_call_arg(
                    arg,
                    kind,
                    receiver_types,
                    self.direct_trait_impls,
                    self.mut_ref_trait_impls,
                );
            }
        }
    }
}

pub(super) fn restore_vec_newtype_method_receivers(modules: &mut BTreeMap<String, CompiledModule>) {
    let vec_newtypes = collect_vec_newtypes(modules);
    if vec_newtypes.is_empty() {
        return;
    }
    for module in modules.values_mut() {
        syn::visit_mut::VisitMut::visit_file_mut(
            &mut RestoreVecNewtypeMethodReceivers {
                module_name: module.mod_name.clone(),
                vec_newtypes: &vec_newtypes,
                counter: 0,
            },
            &mut module.file,
        );
    }
}

fn collect_vec_newtypes(
    modules: &BTreeMap<String, CompiledModule>,
) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    for module in modules.values() {
        for item in &module.file.items {
            let syn::Item::Struct(item_struct) = item else {
                continue;
            };
            let syn::Fields::Unnamed(fields) = &item_struct.fields else {
                continue;
            };
            let Some(field) = fields.unnamed.first() else {
                continue;
            };
            if vec_type_inner(&field.ty).is_some() {
                out.insert(format!("{}::{}", module.mod_name, item_struct.ident));
            }
        }
    }
    out
}

struct RestoreVecNewtypeMethodReceivers<'a> {
    module_name: String,
    vec_newtypes: &'a std::collections::HashSet<String>,
    counter: usize,
}

impl syn::visit_mut::VisitMut for RestoreVecNewtypeMethodReceivers<'_> {
    fn visit_block_mut(&mut self, block: &mut syn::Block) {
        syn::visit_mut::visit_block_mut(self, block);
        for stmt in &mut block.stmts {
            if let Some(rewritten) = self.rewrite_stmt(stmt) {
                *stmt = rewritten;
            } else if let Some(rewritten) = self.rewrite_borrowed_newtype_calls(stmt) {
                *stmt = rewritten;
            }
        }
    }
}

impl RestoreVecNewtypeMethodReceivers<'_> {
    fn rewrite_stmt(&mut self, stmt: &syn::Stmt) -> Option<syn::Stmt> {
        let syn::Stmt::Expr(syn::Expr::MethodCall(method_call), semi) = stmt else {
            return None;
        };
        let receiver =
            vec_newtype_receiver_call(&method_call.receiver, &self.module_name, self.vec_newtypes)?;

        let temp = synthetic_names::vec_newtype_receiver_temp_ident(self.counter);
        self.counter += 1;
        let source = receiver.source;
        let from_func = receiver.from_func;
        let method = method_call.method.clone();
        let args = method_call.args.iter().cloned().collect::<Vec<_>>();
        let expr: syn::Expr = syn::parse_quote! {{
            let mut #temp = #from_func(std::mem::take(&mut #source));
            #temp.#method(#(#args),*);
            #source = Vec::from(#temp);
        }};
        Some(syn::Stmt::Expr(expr, *semi))
    }

    fn rewrite_borrowed_newtype_calls(&mut self, stmt: &syn::Stmt) -> Option<syn::Stmt> {
        let syn::Stmt::Expr(_, semi) = stmt else {
            return None;
        };
        let mut stmt = stmt.clone();
        let mut hoister = VecNewtypeBorrowHoister {
            module_name: self.module_name.clone(),
            vec_newtypes: self.vec_newtypes,
            counter: &mut self.counter,
            bindings: vec![],
        };
        syn::visit_mut::VisitMut::visit_stmt_mut(&mut hoister, &mut stmt);
        if hoister.bindings.is_empty() {
            return None;
        }
        let prelude = hoister
            .bindings
            .iter()
            .map(|binding| {
                let temp = &binding.temp;
                let source = &binding.source;
                let from_func = &binding.from_func;
                syn::parse_quote! {
                    let mut #temp = #from_func(std::mem::take(&mut #source));
                }
            })
            .collect::<Vec<syn::Stmt>>();
        let epilogue = hoister
            .bindings
            .iter()
            .rev()
            .map(|binding| {
                let temp = &binding.temp;
                let source = &binding.source;
                syn::parse_quote! {
                    #source = Vec::from(#temp);
                }
            })
            .collect::<Vec<syn::Stmt>>();
        let expr: syn::Expr = syn::parse_quote! {{
            #(#prelude)*
            #stmt
            #(#epilogue)*
        }};
        Some(syn::Stmt::Expr(expr, *semi))
    }
}

struct VecNewtypeBorrowBinding {
    temp: syn::Ident,
    source: syn::Ident,
    from_func: syn::Expr,
}

struct VecNewtypeBorrowHoister<'a> {
    module_name: String,
    vec_newtypes: &'a std::collections::HashSet<String>,
    counter: &'a mut usize,
    bindings: Vec<VecNewtypeBorrowBinding>,
}

impl syn::visit_mut::VisitMut for VecNewtypeBorrowHoister<'_> {
    fn visit_expr_reference_mut(&mut self, reference: &mut syn::ExprReference) {
        syn::visit_mut::visit_expr_reference_mut(self, reference);
        if reference.mutability.is_none() {
            return;
        }
        let Some(receiver) =
            vec_newtype_receiver_call(&reference.expr, &self.module_name, self.vec_newtypes)
        else {
            return;
        };
        let temp = synthetic_names::vec_newtype_arg_temp_ident(*self.counter);
        *self.counter += 1;
        self.bindings.push(VecNewtypeBorrowBinding {
            temp: temp.clone(),
            source: receiver.source,
            from_func: receiver.from_func,
        });
        *reference.expr = syn::parse_quote! { #temp };
    }
}

struct VecNewtypeReceiverCall {
    from_func: syn::Expr,
    source: syn::Ident,
}

fn vec_newtype_receiver_call(
    expr: &syn::Expr,
    current_module: &str,
    vec_newtypes: &std::collections::HashSet<String>,
) -> Option<VecNewtypeReceiverCall> {
    if let Some(receiver) = zero_arg_method_call_receiver_expr(expr, "clone") {
        return vec_newtype_receiver_call(receiver, current_module, vec_newtypes);
    }

    match expr {
        syn::Expr::Call(call) => {
            let type_key = from_call_vec_newtype_key(&call.func, current_module)?;
            if !vec_newtypes.contains(&type_key) || call.args.len() != 1 {
                return None;
            }
            let source_name = call.args.first().and_then(expr_path_ident_or_clone)?;
            Some(VecNewtypeReceiverCall {
                from_func: (*call.func).clone(),
                source: syn::Ident::new(&source_name, Span::mixed_site()),
            })
        }
        syn::Expr::Group(group) => {
            vec_newtype_receiver_call(&group.expr, current_module, vec_newtypes)
        }
        syn::Expr::Paren(paren) => {
            vec_newtype_receiver_call(&paren.expr, current_module, vec_newtypes)
        }
        _ => None,
    }
}

fn from_call_vec_newtype_key(func: &syn::Expr, current_module: &str) -> Option<String> {
    let syn::Expr::Path(path) = func else {
        return None;
    };
    let segments: Vec<_> = path
        .path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect();
    match segments.as_slice() {
        [ty, from] if from == "from" => Some(format!("{current_module}::{ty}")),
        [.., module, ty, from] if from == "from" => Some(format!("{module}::{ty}")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use quote::quote;
    use syn::parse_quote as rust;

    fn compiled_main_file(
        modules: &std::collections::BTreeMap<String, super::CompiledModule>,
    ) -> &syn::File {
        assert!(modules.contains_key("__main__"), "missing main module");
        &modules["__main__"].file
    }

    #[test]
    fn clone_vec_value_call_args_clones_field_and_index_value_args() {
        let helper_file: syn::File = rust! {
            pub fn take(mut first: String, mut second: String, mut bytes: Vec<u8>) {}
        };
        let main_file: syn::File = rust! {
            pub struct Item {
                pub name: String,
                pub bytes: Vec<u8>,
            }

            pub fn call(mut item: Item, mut values: Vec<String>) {
                crate::helper::take(item.name, values[0], item.bytes);
            }
        };
        let mut modules = std::collections::BTreeMap::from([
            (
                "helper".to_string(),
                super::CompiledModule {
                    mod_name: "helper".to_string(),
                    import_path: "helper".to_string(),
                    file: helper_file,
                    filename: "helper.rs".to_string(),
                    content_hash: String::new(),
                    is_main: false,
                    is_stdlib: false,
                },
            ),
            (
                "__main__".to_string(),
                super::CompiledModule {
                    mod_name: "main".to_string(),
                    import_path: String::new(),
                    file: main_file,
                    filename: "main.rs".to_string(),
                    content_hash: String::new(),
                    is_main: true,
                    is_stdlib: false,
                },
            ),
        ]);

        super::clone_vec_value_call_args(&mut modules);

        let main_file = compiled_main_file(&modules);
        let output = quote! { #main_file }.to_string();
        assert!(
            output.contains(
                "crate :: helper :: take ((item . name) . clone () , (values [0]) . clone () , (item . bytes) . clone ())"
            ),
            "expected value-copy coercions to follow callee cloneable value parameters: {output}"
        );
    }

    #[test]
    fn clone_vec_value_call_args_clones_same_module_value_args() {
        let main_file: syn::File = rust! {
            pub struct Item {
                pub name: String,
            }

            pub fn take(mut value: String) {}

            pub fn call(mut item: Item) {
                take(item.name);
            }
        };
        let mut modules = std::collections::BTreeMap::from([(
            "__main__".to_string(),
            super::CompiledModule {
                mod_name: "main".to_string(),
                import_path: String::new(),
                file: main_file,
                filename: "main.rs".to_string(),
                content_hash: String::new(),
                is_main: true,
                is_stdlib: false,
            },
        )]);

        super::clone_vec_value_call_args(&mut modules);

        let main_file = compiled_main_file(&modules);
        let output = quote! { #main_file }.to_string();
        assert!(
            output.contains("take ((item . name) . clone ())"),
            "expected same-module value-copy coercion to follow callee parameter type: {output}"
        );
    }

    #[test]
    fn clone_vec_value_call_args_does_not_confuse_ufcs_methods_with_functions() {
        let main_file: syn::File = rust! {
            pub struct dataIO;

            pub fn read(mut fd: usize, mut buf: Vec<u8>) {}

            impl dataIO {
                pub fn read(
                    mut d: crate::builtin::GorsPtr<Self>,
                    mut n: isize,
                ) -> Vec<u8> {
                    Vec::new()
                }
            }

            pub fn call(mut d: crate::builtin::GorsPtr<dataIO>, mut n: Vec<isize>) {
                let _ = <dataIO>::read(
                    d,
                    {
                        n[0]
                    },
                );
            }
        };
        let mut modules = std::collections::BTreeMap::from([(
            "__main__".to_string(),
            super::CompiledModule {
                mod_name: "main".to_string(),
                import_path: String::new(),
                file: main_file,
                filename: "main.rs".to_string(),
                content_hash: String::new(),
                is_main: true,
                is_stdlib: false,
            },
        )]);

        super::clone_vec_value_call_args(&mut modules);

        let main_file = compiled_main_file(&modules);
        let output = quote! { #main_file }.to_string();
        assert!(
            output.contains("< dataIO > :: read (d , { n [0] }"),
            "expected UFCS method argument not to inherit top-level read Vec parameter: {output}"
        );
        assert!(
            !output.contains("< dataIO > :: read (d , ({ n [0] }) . to_vec ())"),
            "expected UFCS method scalar argument not to be materialized as Vec: {output}"
        );
    }

    #[test]
    fn clone_vec_value_call_args_takes_nonclone_any_vec_args() {
        let helper_file: syn::File = rust! {
            pub fn inspect(mut values: Vec<Box<dyn std::any::Any>>, mut index: isize) {}
            pub fn bytes(mut values: Vec<u8>) {}
        };
        let main_file: syn::File = rust! {
            pub fn make_values() -> Vec<Box<dyn std::any::Any>> {
                Vec::<Box<dyn std::any::Any>>::new()
            }

            pub fn call(mut values: Vec<Box<dyn std::any::Any>>, mut data: Vec<u8>) {
                crate::helper::inspect(values, 0);
                crate::helper::inspect(make_values(), 1);
                crate::helper::bytes(data);
            }
        };
        let mut modules = std::collections::BTreeMap::from([
            (
                "helper".to_string(),
                super::CompiledModule {
                    mod_name: "helper".to_string(),
                    import_path: "helper".to_string(),
                    file: helper_file,
                    filename: "helper.rs".to_string(),
                    content_hash: String::new(),
                    is_main: false,
                    is_stdlib: false,
                },
            ),
            (
                "__main__".to_string(),
                super::CompiledModule {
                    mod_name: "main".to_string(),
                    import_path: String::new(),
                    file: main_file,
                    filename: "main.rs".to_string(),
                    content_hash: String::new(),
                    is_main: true,
                    is_stdlib: false,
                },
            ),
        ]);

        super::clone_vec_value_call_args(&mut modules);

        let main_file = compiled_main_file(&modules);
        let output = quote! { #main_file }.to_string();
        assert!(
            output.contains("crate :: helper :: inspect (std :: mem :: take (& mut values) , 0)"),
            "expected non-clone any Vec lvalue argument to be taken from the caller: {output}"
        );
        assert!(
            output.contains("crate :: helper :: inspect (make_values () , 1)"),
            "expected temporary non-clone any Vec argument to remain unchanged: {output}"
        );
        assert!(
            output.contains("crate :: helper :: bytes ((data) . clone ())"),
            "expected ordinary Vec argument to keep cloneable-value behavior: {output}"
        );
    }

    #[test]
    fn clone_vec_value_call_args_takes_deref_self_vec_args() {
        let helper_file: syn::File = rust! {
            pub fn append_rune(mut values: Vec<u8>, mut r: i32) -> Vec<u8> {
                values
            }
        };
        let main_file: syn::File = rust! {
            pub struct buffer(pub Vec<u8>);

            pub fn write_pointer(mut values: &mut Vec<u8>, mut r: i32) {
                crate::helper::append_rune(*values, r);
            }

            impl buffer {
                pub fn write_rune(&mut self, mut r: i32) {
                    *self = buffer(crate::helper::append_rune(*self, r));
                }
            }
        };
        let mut modules = std::collections::BTreeMap::from([
            (
                "helper".to_string(),
                super::CompiledModule {
                    mod_name: "helper".to_string(),
                    import_path: "helper".to_string(),
                    file: helper_file,
                    filename: "helper.rs".to_string(),
                    content_hash: String::new(),
                    is_main: false,
                    is_stdlib: false,
                },
            ),
            (
                "__main__".to_string(),
                super::CompiledModule {
                    mod_name: "main".to_string(),
                    import_path: String::new(),
                    file: main_file,
                    filename: "main.rs".to_string(),
                    content_hash: String::new(),
                    is_main: true,
                    is_stdlib: false,
                },
            ),
        ]);

        super::clone_vec_value_call_args(&mut modules);

        let main_file = compiled_main_file(&modules);
        let output = quote! { #main_file }.to_string();
        assert!(
            output
                .contains("crate :: helper :: append_rune (std :: mem :: take (& mut * self) , r)"),
            "expected by-value Vec helper arg to take dereferenced receiver lvalue: {output}"
        );
        assert!(
            output.contains(
                "crate :: helper :: append_rune (std :: mem :: take (& mut * values) , r)"
            ),
            "expected by-value Vec helper arg to take ordinary dereferenced lvalue: {output}"
        );
    }

    #[test]
    fn clone_vec_value_call_args_keys_method_value_args_by_receiver_type() {
        let main_file: syn::File = rust! {
            pub struct NeedsClone;
            impl NeedsClone {
                pub fn put(&self, tag: usize, mut value: String) {}
            }

            pub struct TakesCopy;
            impl TakesCopy {
                pub fn put(&self, tag: usize, value: usize) {}
            }

            pub struct Holder {
                pub needs: NeedsClone,
                pub plain: TakesCopy,
            }

            pub struct Item {
                pub name: String,
                pub count: usize,
            }

            pub fn call(mut holder: Holder, mut item: Item) {
                holder.needs.put(item.count, item.name);
                holder.plain.put(item.count, item.count);
            }
        };
        let mut modules = std::collections::BTreeMap::from([(
            "__main__".to_string(),
            super::CompiledModule {
                mod_name: "main".to_string(),
                import_path: String::new(),
                file: main_file,
                filename: "main.rs".to_string(),
                content_hash: String::new(),
                is_main: true,
                is_stdlib: false,
            },
        )]);

        super::clone_vec_value_call_args(&mut modules);

        let main_file = compiled_main_file(&modules);
        let output = quote! { #main_file }.to_string();
        assert!(
            output.contains("holder . needs . put (item . count , (item . name) . clone ())"),
            "expected clone to follow NeedsClone::put value signature: {output}"
        );
        assert!(
            output.contains("holder . plain . put (item . count , item . count)"),
            "expected TakesCopy::put argument to remain unchanged: {output}"
        );
        assert!(
            !output.contains("holder . plain . put (item . count , (item . count) . clone ())"),
            "expected same-named method not to inherit NeedsClone::put coercion: {output}"
        );
    }

    #[test]
    fn clone_vec_value_call_args_converts_slice_method_args_to_vec() {
        let main_file: syn::File = rust! {
            pub struct Sink;
            impl Sink {
                pub fn Write(&self, data: Vec<u8>) {}
            }

            pub struct TakesIndex;
            impl TakesIndex {
                pub fn Write(&self, data: u8) {}
            }

            pub struct Holder {
                pub sink: Sink,
                pub other: TakesIndex,
            }

            pub fn call(mut holder: Holder, mut data: Vec<u8>) {
                holder.sink.Write(data[1..]);
                holder.other.Write(data[0]);
            }
        };
        let mut modules = std::collections::BTreeMap::from([(
            "__main__".to_string(),
            super::CompiledModule {
                mod_name: "main".to_string(),
                import_path: String::new(),
                file: main_file,
                filename: "main.rs".to_string(),
                content_hash: String::new(),
                is_main: true,
                is_stdlib: false,
            },
        )]);

        super::clone_vec_value_call_args(&mut modules);

        let main_file = compiled_main_file(&modules);
        let output = quote! { #main_file }.to_string();
        assert!(
            output.contains("holder . sink . Write ((data [1 ..]) . to_vec ())"),
            "expected range-index slice argument to materialize Vec for Sink::Write: {output}"
        );
        assert!(
            output.contains("holder . other . Write (data [0])"),
            "expected same-named non-Vec method argument to remain unchanged: {output}"
        );
    }

    #[test]
    fn clone_vec_value_call_args_converts_trait_method_block_args_to_vec() {
        let main_file: syn::File = rust! {
            pub struct buffer(pub Vec<u8>);
            impl buffer {
                pub fn to_vec(&self) -> Vec<u8> {
                    self.0.clone()
                }
            }

            pub trait Writer {
                fn Write(&mut self, data: Vec<u8>);
            }

            pub fn call(mut w: &mut dyn Writer, mut data: buffer) {
                w.Write({
                    let temp = data.clone();
                    temp
                });
            }
        };
        let mut modules = std::collections::BTreeMap::from([(
            "__main__".to_string(),
            super::CompiledModule {
                mod_name: "main".to_string(),
                import_path: String::new(),
                file: main_file,
                filename: "main.rs".to_string(),
                content_hash: String::new(),
                is_main: true,
                is_stdlib: false,
            },
        )]);

        super::clone_vec_value_call_args(&mut modules);

        let main_file = compiled_main_file(&modules);
        let output = quote! { #main_file }.to_string();
        assert!(
            output.contains("w . Write (({ let temp = data . clone () ; temp }) . to_vec ())"),
            "expected trait method Vec argument block to materialize Vec: {output}"
        );
    }

    #[test]
    fn clone_vec_value_call_args_uses_supertrait_method_value_args() {
        let io_file: syn::File = rust! {
            pub trait Writer {
                fn Write(&mut self, data: Vec<u8>);
            }
        };
        let hash_file: syn::File = rust! {
            pub trait Hash: crate::io::Writer {
                fn Sum(&mut self, data: Vec<u8>) -> Vec<u8>;
            }

            pub trait Hash32: Hash {
                fn Sum32(&mut self) -> u32;
            }
        };
        let fnv_file: syn::File = rust! {
            pub fn New32() -> Box<dyn crate::hash::Hash32> {
                panic!()
            }
        };
        let main_file: syn::File = rust! {
            pub fn call() {
                let mut data = Vec::<u8>::new();
                let mut h = crate::hash__fnv::New32();
                h.Write(data);
                h.Write(data);
            }
        };
        let mut modules = std::collections::BTreeMap::from([
            (
                "io".to_string(),
                super::CompiledModule {
                    mod_name: "io".to_string(),
                    import_path: "io".to_string(),
                    file: io_file,
                    filename: "io.rs".to_string(),
                    content_hash: String::new(),
                    is_main: false,
                    is_stdlib: false,
                },
            ),
            (
                "hash".to_string(),
                super::CompiledModule {
                    mod_name: "hash".to_string(),
                    import_path: "hash".to_string(),
                    file: hash_file,
                    filename: "hash.rs".to_string(),
                    content_hash: String::new(),
                    is_main: false,
                    is_stdlib: false,
                },
            ),
            (
                "hash__fnv".to_string(),
                super::CompiledModule {
                    mod_name: "hash__fnv".to_string(),
                    import_path: "hash/fnv".to_string(),
                    file: fnv_file,
                    filename: "hash__fnv.rs".to_string(),
                    content_hash: String::new(),
                    is_main: false,
                    is_stdlib: false,
                },
            ),
            (
                "__main__".to_string(),
                super::CompiledModule {
                    mod_name: "main".to_string(),
                    import_path: String::new(),
                    file: main_file,
                    filename: "main.rs".to_string(),
                    content_hash: String::new(),
                    is_main: true,
                    is_stdlib: false,
                },
            ),
        ]);

        super::clone_vec_value_call_args(&mut modules);

        let main_file = compiled_main_file(&modules);
        let output = quote! { #main_file }.to_string();
        let clone_count = output.matches("h . Write ((data) . clone ())").count();
        assert_eq!(
            clone_count, 2,
            "expected Vec argument clones to follow inherited Writer::Write signature: {output}"
        );
    }

    #[test]
    fn borrow_mut_ref_call_args_borrows_cross_module_args_from_callee_signatures() {
        let helper_file: syn::File = rust! {
            pub fn sort(mut values: &mut [String]) {}
        };
        let main_file: syn::File = rust! {
            pub fn call(mut values: Vec<String>) {
                crate::helper::sort(values);
            }
        };
        let mut modules = std::collections::BTreeMap::from([
            (
                "helper".to_string(),
                super::CompiledModule {
                    mod_name: "helper".to_string(),
                    import_path: "helper".to_string(),
                    file: helper_file,
                    filename: "helper.rs".to_string(),
                    content_hash: String::new(),
                    is_main: false,
                    is_stdlib: false,
                },
            ),
            (
                "__main__".to_string(),
                super::CompiledModule {
                    mod_name: "main".to_string(),
                    import_path: String::new(),
                    file: main_file,
                    filename: "main.rs".to_string(),
                    content_hash: String::new(),
                    is_main: true,
                    is_stdlib: false,
                },
            ),
        ]);

        super::borrow_mut_ref_call_args(&mut modules);

        let main_file = compiled_main_file(&modules);
        let output = quote! { #main_file }.to_string();
        assert!(
            output.contains("crate :: helper :: sort (& mut values)"),
            "expected mutable borrow to follow callee signature: {output}"
        );
    }

    #[test]
    fn borrow_mut_ref_call_args_keys_method_args_by_receiver_type() {
        let main_file: syn::File = rust! {
            pub struct NeedsMut;
            impl NeedsMut {
                pub fn fill(&self, mut values: &mut [String]) {}
            }

            pub struct TakesValue;
            impl TakesValue {
                pub fn fill(&self, mut values: Vec<String>) {}
            }

            pub fn call(mut needs: NeedsMut, plain: TakesValue, mut values: Vec<String>, other: Vec<String>) {
                needs.fill(values);
                plain.fill(other);
            }
        };
        let mut modules = std::collections::BTreeMap::from([(
            "__main__".to_string(),
            super::CompiledModule {
                mod_name: "main".to_string(),
                import_path: String::new(),
                file: main_file,
                filename: "main.rs".to_string(),
                content_hash: String::new(),
                is_main: true,
                is_stdlib: false,
            },
        )]);

        super::borrow_mut_ref_call_args(&mut modules);

        let main_file = compiled_main_file(&modules);
        let output = quote! { #main_file }.to_string();
        assert!(
            output.contains("needs . fill (& mut values)"),
            "expected receiver-specific mutable borrow for NeedsMut::fill: {output}"
        );
        assert!(
            output.contains("plain . fill (other)"),
            "expected TakesValue::fill argument to remain by value: {output}"
        );
        assert!(
            !output.contains("plain . fill (& mut other)"),
            "expected same-named value method not to inherit NeedsMut::fill coercion: {output}"
        );
    }

    #[test]
    fn borrow_mut_ref_call_args_keeps_receiver_facts_block_scoped() {
        let main_file: syn::File = rust! {
            pub struct NeedsMut;
            impl NeedsMut {
                pub fn fill(&self, mut values: &mut [String]) {}
            }

            pub struct TakesValue;
            impl TakesValue {
                pub fn fill(&self, mut values: Vec<String>) {}
            }

            pub fn call(mut values: Vec<String>, mut other: Vec<String>) {
                let target: TakesValue = TakesValue;
                {
                    let target: NeedsMut = NeedsMut;
                    target.fill(values);
                }
                target.fill(other);
            }
        };
        let mut modules = std::collections::BTreeMap::from([(
            "__main__".to_string(),
            super::CompiledModule {
                mod_name: "main".to_string(),
                import_path: String::new(),
                file: main_file,
                filename: "main.rs".to_string(),
                content_hash: String::new(),
                is_main: true,
                is_stdlib: false,
            },
        )]);

        super::borrow_mut_ref_call_args(&mut modules);

        let main_file = compiled_main_file(&modules);
        let output = quote! { #main_file }.to_string();
        assert!(
            output.contains("target . fill (& mut values)"),
            "expected inner receiver fact to borrow the matching argument: {output}"
        );
        assert!(
            output.contains("target . fill (other)"),
            "expected outer receiver fact to be restored after the nested block: {output}"
        );
        assert!(
            !output.contains("target . fill (& mut other)"),
            "expected nested receiver fact not to leak after the block: {output}"
        );
    }

    #[test]
    fn borrow_mut_ref_call_args_reborrows_self_for_mut_trait_object_impls() {
        let main_file: syn::File = rust! {
            pub trait State {
                fn write(&mut self);
            }

            pub trait Formatter {
                fn format(&mut self, state: &mut dyn State);
            }

            pub struct Printer;
            pub struct FormatterImpl;

            impl<'a> State for &'a mut Printer {
                fn write(&mut self) {}
            }

            impl Formatter for FormatterImpl {
                fn format(&mut self, state: &mut dyn State) {}
            }

            impl Printer {
                pub fn call(&mut self) {
                    let (mut formatter, mut ok) = (
                        Box::new(FormatterImpl::default()) as Box<dyn Formatter>,
                        true,
                    );
                    if ok {
                        formatter.format(self);
                    }
                }
            }
        };
        let mut modules = std::collections::BTreeMap::from([(
            "__main__".to_string(),
            super::CompiledModule {
                mod_name: "main".to_string(),
                import_path: String::new(),
                file: main_file,
                filename: "main.rs".to_string(),
                content_hash: String::new(),
                is_main: true,
                is_stdlib: false,
            },
        )]);

        super::borrow_mut_ref_call_args(&mut modules);

        let main_file = compiled_main_file(&modules);
        let output = quote! { #main_file }.to_string();
        assert!(
            output.contains("formatter . format (& mut {")
                && output.contains("let __gors_self = & mut * self")
                && output.contains("__gors_self"),
            "expected self to be reborrowed when &mut Self implements the trait object target: {output}"
        );
    }

    #[test]
    fn borrow_mut_ref_call_args_keeps_self_for_direct_trait_object_impls() {
        let main_file: syn::File = rust! {
            pub trait State {
                fn write(&mut self);
            }

            pub trait Formatter {
                fn format(&mut self, state: &mut dyn State);
            }

            pub struct Printer;
            pub struct FormatterImpl;

            impl State for Printer {
                fn write(&mut self) {}
            }

            impl Formatter for FormatterImpl {
                fn format(&mut self, state: &mut dyn State) {}
            }

            impl Printer {
                pub fn call(&mut self) {
                    let (mut formatter, mut ok) = (
                        Box::new(FormatterImpl::default()) as Box<dyn Formatter>,
                        true,
                    );
                    if ok {
                        formatter.format(self);
                    }
                }
            }
        };
        let mut modules = std::collections::BTreeMap::from([(
            "__main__".to_string(),
            super::CompiledModule {
                mod_name: "main".to_string(),
                import_path: String::new(),
                file: main_file,
                filename: "main.rs".to_string(),
                content_hash: String::new(),
                is_main: true,
                is_stdlib: false,
            },
        )]);

        super::borrow_mut_ref_call_args(&mut modules);

        let main_file = compiled_main_file(&modules);
        let output = quote! { #main_file }.to_string();
        assert!(
            output.contains("formatter . format (self)"),
            "expected self to stay unchanged when Self directly implements the trait object target: {output}"
        );
        assert!(
            !output.contains("__gors_self"),
            "expected direct trait impl not to receive the &mut Self reborrow: {output}"
        );
    }

    #[test]
    fn borrow_mut_ref_call_args_uses_supertrait_method_args() {
        let main_file: syn::File = rust! {
            pub trait Writer {
                fn write(&mut self, values: &mut [String]);
            }

            pub trait ReadWriter: Writer {}

            pub struct Sink;

            impl Writer for Sink {
                fn write(&mut self, values: &mut [String]) {}
            }

            impl ReadWriter for Sink {}

            pub fn call(mut rw: Box<dyn ReadWriter>, mut values: Vec<String>) {
                rw.write(values);
            }
        };
        let mut modules = std::collections::BTreeMap::from([(
            "__main__".to_string(),
            super::CompiledModule {
                mod_name: "main".to_string(),
                import_path: String::new(),
                file: main_file,
                filename: "main.rs".to_string(),
                content_hash: String::new(),
                is_main: true,
                is_stdlib: false,
            },
        )]);

        super::borrow_mut_ref_call_args(&mut modules);

        let main_file = compiled_main_file(&modules);
        let output = quote! { #main_file }.to_string();
        assert!(
            output.contains("rw . write (& mut values)"),
            "expected mutable borrow to follow inherited Writer::write signature: {output}"
        );
    }

    #[test]
    fn borrow_mut_ref_call_args_avoids_ambiguous_trait_method_name_fallback() {
        let main_file: syn::File = rust! {
            pub trait NeedsMut {
                fn fill(&mut self, values: &mut [String]);
            }

            pub trait TakesValue {
                fn fill(&mut self, values: Vec<String>);
            }

            pub fn call(mut values: Vec<String>) {
                source().fill(values);
            }
        };
        let mut modules = std::collections::BTreeMap::from([(
            "__main__".to_string(),
            super::CompiledModule {
                mod_name: "main".to_string(),
                import_path: String::new(),
                file: main_file,
                filename: "main.rs".to_string(),
                content_hash: String::new(),
                is_main: true,
                is_stdlib: false,
            },
        )]);

        super::borrow_mut_ref_call_args(&mut modules);

        let main_file = compiled_main_file(&modules);
        let output = quote! { #main_file }.to_string();
        assert!(
            output.contains("source () . fill (values)"),
            "expected ambiguous trait method name not to borrow unknown receiver args: {output}"
        );
        assert!(
            !output.contains("source () . fill (& mut values)"),
            "expected non-mut-ref same-named trait method to suppress untyped fallback: {output}"
        );
    }

    #[test]
    fn borrow_mutated_vec_params_keys_method_args_by_receiver_type() {
        let main_file: syn::File = rust! {
            pub struct Mutates;
            impl Mutates {
                pub fn fill(&self, mut values: Vec<String>) {
                    values[0] = String::new();
                }
            }

            pub struct TakesValue;
            impl TakesValue {
                pub fn fill(&self, mut values: Vec<String>) {
                    let _ = values;
                }
            }

            pub fn call(mut mutates: Mutates, plain: TakesValue, mut values: Vec<String>, other: Vec<String>) {
                mutates.fill(values);
                plain.fill(other);
            }
        };
        let mut modules = std::collections::BTreeMap::from([(
            "__main__".to_string(),
            super::CompiledModule {
                mod_name: "main".to_string(),
                import_path: String::new(),
                file: main_file,
                filename: "main.rs".to_string(),
                content_hash: String::new(),
                is_main: true,
                is_stdlib: false,
            },
        )]);

        super::borrow_mutated_vec_params(&mut modules);

        let main_file = compiled_main_file(&modules);
        let output = quote! { #main_file }.to_string();
        assert!(
            output.contains("mutates . fill (& mut * values)"),
            "expected receiver-specific mutable slice borrow for Mutates::fill: {output}"
        );
        assert!(
            output.contains("plain . fill (other)"),
            "expected TakesValue::fill argument to remain by value: {output}"
        );
        assert!(
            !output.contains("plain . fill (& mut * other)"),
            "expected same-named value method not to inherit Mutates::fill coercion: {output}"
        );
    }

    #[test]
    fn borrow_mutated_vec_params_recovers_cloned_field_lvalue_args() {
        let main_file: syn::File = rust! {
            pub struct Holder {
                pub data: Vec<u8>,
            }

            pub fn fill(mut values: Vec<u8>) {
                values[0] = 1;
            }

            pub fn call(mut h: crate::builtin::GorsPtr<Holder>) {
                fill({
                    let __gors_pointer_field = (h.lock().unwrap().data).clone();
                    __gors_pointer_field
                });
            }
        };
        let mut modules = std::collections::BTreeMap::from([(
            "__main__".to_string(),
            super::CompiledModule {
                mod_name: "main".to_string(),
                import_path: String::new(),
                file: main_file,
                filename: "main.rs".to_string(),
                content_hash: String::new(),
                is_main: true,
                is_stdlib: false,
            },
        )]);

        super::borrow_mutated_vec_params(&mut modules);

        let main_file = compiled_main_file(&modules);
        let output = quote! { #main_file }.to_string();
        assert!(
            output.contains("fill (& mut (h . lock () . unwrap () . data))")
                || output.contains("fill (& mut h . lock () . unwrap () . data)"),
            "expected cloned pointer-field read to recover the mutable field lvalue: {output}"
        );
        assert!(
            !output.contains("& mut { let __gors_pointer_field"),
            "expected pass not to borrow a cloned temporary block: {output}"
        );
    }
}
