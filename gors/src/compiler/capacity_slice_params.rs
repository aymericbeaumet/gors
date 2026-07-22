use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::{
    CompiledModule,
    receiver_method_targets::Targets,
    receiver_type_facts::ReceiverTypeRef,
    receiver_type_scopes,
    syn_inspect::{
        call_target_key, is_path_call_expr, named_self_type, owned_slice_storage_type_inner,
        pat_ident_name, slice_type_inner, strip_paren_or_group,
    },
    synthetic_names,
};

type FunctionTargets = BTreeMap<String, BTreeSet<usize>>;
type MethodTargets = Targets<BTreeSet<usize>>;
type MethodKey = (String, String, String);
type MethodRequirements = BTreeMap<MethodKey, BTreeSet<usize>>;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum LengthDependency {
    None,
    VisibleLength,
    MayExceedVisibleLength,
}

pub(super) fn preserve_capacity_for_resliced_slice_params(
    modules: &mut BTreeMap<String, CompiledModule>,
) {
    let (mut functions, mut method_requirements) = direct_capacity_requirements(modules);
    propagate_capacity_requirements(modules, &mut functions, &mut method_requirements);
    let methods = method_targets(modules, &method_requirements);

    for module in modules.values_mut() {
        for item in &mut module.file.items {
            match item {
                syn::Item::Fn(function) => {
                    let key = format!("{}::{}", module.mod_name, function.sig.ident);
                    if let Some(params) = functions.get(&key) {
                        rewrite_capacity_slice_params(
                            &mut function.sig,
                            &mut function.block,
                            params,
                        );
                    }
                }
                syn::Item::Impl(item_impl) if item_impl.trait_.is_none() => {
                    let Some(self_name) = named_self_type(&item_impl.self_ty) else {
                        continue;
                    };
                    for impl_item in &mut item_impl.items {
                        let syn::ImplItem::Fn(method) = impl_item else {
                            continue;
                        };
                        let key = (
                            module.mod_name.clone(),
                            self_name.clone(),
                            method.sig.ident.to_string(),
                        );
                        if let Some(params) = method_requirements.get(&key) {
                            let signature_params = params
                                .iter()
                                .map(|index| index + 1)
                                .collect::<BTreeSet<_>>();
                            rewrite_capacity_slice_params(
                                &mut method.sig,
                                &mut method.block,
                                &signature_params,
                            );
                        }
                    }
                }
                _ => {}
            }
        }
    }

    if functions.is_empty() && methods.is_empty() {
        return;
    }
    let receiver_facts = receiver_type_scopes::ProgramFacts::collect(modules);
    for module in modules.values_mut() {
        syn::visit_mut::VisitMut::visit_file_mut(
            &mut CapacityCallVisitor {
                receiver_types: receiver_facts.tracker(module.mod_name.clone()),
                functions: &functions,
                methods: &methods,
                capacity_params: Vec::new(),
                borrowed_slice_params: Vec::new(),
            },
            &mut module.file,
        );
    }
}

fn direct_capacity_requirements(
    modules: &BTreeMap<String, CompiledModule>,
) -> (FunctionTargets, MethodRequirements) {
    let mut functions = FunctionTargets::new();
    let mut methods = MethodRequirements::new();

    for module in modules.values() {
        for item in &module.file.items {
            match item {
                syn::Item::Fn(function) => {
                    let params = direct_capacity_slice_params(&function.sig, &function.block);
                    if !params.is_empty() {
                        functions.insert(
                            format!("{}::{}", module.mod_name, function.sig.ident),
                            params,
                        );
                    }
                }
                syn::Item::Impl(item_impl) if item_impl.trait_.is_none() => {
                    let Some(self_name) = named_self_type(&item_impl.self_ty) else {
                        continue;
                    };
                    for impl_item in &item_impl.items {
                        let syn::ImplItem::Fn(method) = impl_item else {
                            continue;
                        };
                        let params = direct_capacity_slice_params(&method.sig, &method.block)
                            .into_iter()
                            .filter_map(|index| index.checked_sub(1))
                            .collect::<BTreeSet<_>>();
                        if !params.is_empty() {
                            methods.insert(
                                (
                                    module.mod_name.clone(),
                                    self_name.clone(),
                                    method.sig.ident.to_string(),
                                ),
                                params,
                            );
                        }
                    }
                }
                _ => {}
            }
        }
    }

    (functions, methods)
}

fn direct_capacity_slice_params(signature: &syn::Signature, body: &syn::Block) -> BTreeSet<usize> {
    signature
        .inputs
        .iter()
        .enumerate()
        .filter_map(|(index, input)| {
            let syn::FnArg::Typed(param) = input else {
                return None;
            };
            let name = pat_ident_name(&param.pat)?;
            if mutable_slice_element(&param.ty).is_some() {
                return capacity_dependent_self_reslice(body, &name).then_some(index);
            }
            let ident = syn::Ident::new(&name, proc_macro2::Span::mixed_site());
            owned_slice_storage_type_inner(&param.ty)?;
            (super::call_arg_rewrites::body_reassigns_param(body, &ident)
                && super::call_arg_rewrites::body_mutates_vec_param(body, &ident)
                && body_has_owned_self_reslice(body, &name))
            .then_some(index)
        })
        .collect()
}

fn method_targets(
    modules: &BTreeMap<String, CompiledModule>,
    requirements: &MethodRequirements,
) -> MethodTargets {
    let mut targets = MethodTargets::default();
    for module in modules.values() {
        for item in &module.file.items {
            let syn::Item::Impl(item_impl) = item else {
                continue;
            };
            if item_impl.trait_.is_some() {
                continue;
            }
            targets.record_methods_seen(&module.mod_name, item_impl);
            let Some(self_name) = named_self_type(&item_impl.self_ty) else {
                continue;
            };
            for impl_item in &item_impl.items {
                let syn::ImplItem::Fn(method) = impl_item else {
                    continue;
                };
                let key = (
                    module.mod_name.clone(),
                    self_name.clone(),
                    method.sig.ident.to_string(),
                );
                if let Some(params) = requirements.get(&key) {
                    targets.insert_receiver(
                        &module.mod_name,
                        &self_name,
                        &method.sig.ident.to_string(),
                        params.clone(),
                    );
                }
            }
        }
    }
    targets.finalize_unambiguous_names();
    targets
}

fn propagate_capacity_requirements(
    modules: &BTreeMap<String, CompiledModule>,
    functions: &mut FunctionTargets,
    method_requirements: &mut MethodRequirements,
) {
    let receiver_facts = receiver_type_scopes::ProgramFacts::collect(modules);
    loop {
        let methods = method_targets(modules, method_requirements);
        let mut function_additions = FunctionTargets::new();
        let mut method_additions = MethodRequirements::new();

        for module in modules.values() {
            for item in &module.file.items {
                match item {
                    syn::Item::Fn(function) => {
                        let params = forwarded_capacity_slice_params(
                            &module.mod_name,
                            &function.sig,
                            &function.block,
                            None,
                            &receiver_facts,
                            functions,
                            &methods,
                        );
                        if !params.is_empty() {
                            function_additions.insert(
                                format!("{}::{}", module.mod_name, function.sig.ident),
                                params,
                            );
                        }
                    }
                    syn::Item::Impl(item_impl) if item_impl.trait_.is_none() => {
                        let Some(self_name) = named_self_type(&item_impl.self_ty) else {
                            continue;
                        };
                        for impl_item in &item_impl.items {
                            let syn::ImplItem::Fn(method) = impl_item else {
                                continue;
                            };
                            let params = forwarded_capacity_slice_params(
                                &module.mod_name,
                                &method.sig,
                                &method.block,
                                Some(item_impl),
                                &receiver_facts,
                                functions,
                                &methods,
                            )
                            .into_iter()
                            .filter_map(|index| index.checked_sub(1))
                            .collect::<BTreeSet<_>>();
                            if !params.is_empty() {
                                method_additions.insert(
                                    (
                                        module.mod_name.clone(),
                                        self_name.clone(),
                                        method.sig.ident.to_string(),
                                    ),
                                    params,
                                );
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        let mut changed = merge_requirements(functions, function_additions);
        changed |= merge_requirements(method_requirements, method_additions);
        if !changed {
            break;
        }
    }
}

fn merge_requirements<K: Ord>(
    requirements: &mut BTreeMap<K, BTreeSet<usize>>,
    additions: BTreeMap<K, BTreeSet<usize>>,
) -> bool {
    let mut changed = false;
    for (key, params) in additions {
        let entry = requirements.entry(key).or_default();
        let previous_len = entry.len();
        entry.extend(params);
        changed |= entry.len() != previous_len;
    }
    changed
}

fn forwarded_capacity_slice_params(
    module_name: &str,
    signature: &syn::Signature,
    body: &syn::Block,
    item_impl: Option<&syn::ItemImpl>,
    receiver_facts: &receiver_type_scopes::ProgramFacts,
    functions: &FunctionTargets,
    methods: &MethodTargets,
) -> BTreeSet<usize> {
    let params = signature
        .inputs
        .iter()
        .enumerate()
        .filter_map(|(index, input)| {
            let syn::FnArg::Typed(param) = input else {
                return None;
            };
            transactional_slice_element(&param.ty)?;
            Some((pat_ident_name(&param.pat)?, index))
        })
        .collect::<HashMap<_, _>>();
    if params.is_empty() {
        return BTreeSet::new();
    }

    let mut receiver_types = receiver_facts.tracker(module_name.to_string());
    if let Some(item_impl) = item_impl {
        receiver_types.enter_impl(item_impl);
    }
    for input in &signature.inputs {
        receiver_types.record_fn_arg(input);
    }
    let mut visitor = ForwardedCapacityVisitor {
        receiver_types,
        functions,
        methods,
        params,
        shadowed_params: Vec::new(),
        found: BTreeSet::new(),
    };
    syn::visit::Visit::visit_block(&mut visitor, body);
    visitor.found
}

struct ForwardedCapacityVisitor<'a> {
    receiver_types: receiver_type_scopes::Tracker<'a>,
    functions: &'a FunctionTargets,
    methods: &'a MethodTargets,
    params: HashMap<String, usize>,
    shadowed_params: Vec<BTreeSet<String>>,
    found: BTreeSet<usize>,
}

impl ForwardedCapacityVisitor<'_> {
    fn record_forwarded_args<'a>(
        &mut self,
        args: impl Iterator<Item = (usize, &'a syn::Expr)>,
        required: &BTreeSet<usize>,
    ) {
        for (index, arg) in args {
            if !required.contains(&index) {
                continue;
            }
            let Some(name) = forwarded_param_name(arg) else {
                continue;
            };
            if self
                .shadowed_params
                .iter()
                .rev()
                .any(|names| names.contains(&name))
            {
                continue;
            }
            if let Some(index) = self.params.get(&name) {
                self.found.insert(*index);
            }
        }
    }
}

impl syn::visit::Visit<'_> for ForwardedCapacityVisitor<'_> {
    fn visit_block(&mut self, block: &syn::Block) {
        self.receiver_types.push_scope();
        self.shadowed_params.push(BTreeSet::new());
        syn::visit::visit_block(self, block);
        self.shadowed_params.pop();
        self.receiver_types.pop_scope();
    }

    fn visit_local(&mut self, local: &syn::Local) {
        syn::visit::visit_local(self, local);
        self.receiver_types.record_local(local);
        if let Some(name) = pat_ident_name(&local.pat)
            && self.params.contains_key(&name)
            && let Some(scope) = self.shadowed_params.last_mut()
        {
            scope.insert(name);
        }
    }

    fn visit_expr_call(&mut self, call: &syn::ExprCall) {
        if let Some((receiver, method)) = qself_receiver_method_call(&self.receiver_types, call)
            && let Some(required) = self
                .methods
                .target_for_call(self.receiver_types.module_name(), &method, Some(&receiver))
                .cloned()
        {
            self.record_forwarded_args(
                call.args
                    .iter()
                    .enumerate()
                    .skip(1)
                    .map(|(index, arg)| (index - 1, arg)),
                &required,
            );
        } else if let Some(key) = call_target_key(&call.func, self.receiver_types.module_name())
            && let Some(required) = self.functions.get(&key).cloned()
        {
            self.record_forwarded_args(call.args.iter().enumerate(), &required);
        }
        syn::visit::visit_expr_call(self, call);
    }

    fn visit_expr_method_call(&mut self, call: &syn::ExprMethodCall) {
        let Some(receiver) = self.receiver_types.receiver_type_for_expr(&call.receiver) else {
            syn::visit::visit_expr_method_call(self, call);
            return;
        };
        if let Some(required) = self
            .methods
            .target_for_call(
                self.receiver_types.module_name(),
                &call.method.to_string(),
                Some(&receiver),
            )
            .cloned()
        {
            self.record_forwarded_args(call.args.iter().enumerate(), &required);
        }
        syn::visit::visit_expr_method_call(self, call);
    }
}

fn forwarded_param_name(expr: &syn::Expr) -> Option<String> {
    match strip_paren_or_group(expr) {
        syn::Expr::Path(path) => path.path.get_ident().map(ToString::to_string),
        syn::Expr::Reference(reference) => forwarded_param_name(&reference.expr),
        syn::Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Deref(_)) => {
            forwarded_param_name(&unary.expr)
        }
        _ => None,
    }
}

fn rewrite_capacity_slice_params(
    signature: &mut syn::Signature,
    body: &mut syn::Block,
    required: &BTreeSet<usize>,
) {
    let candidates = signature
        .inputs
        .iter()
        .enumerate()
        .filter_map(|(index, input)| {
            if !required.contains(&index) {
                return None;
            }
            let syn::FnArg::Typed(param) = input else {
                return None;
            };
            let name = pat_ident_name(&param.pat)?;
            let elem = transactional_slice_element(&param.ty)?;
            Some((index, name, elem))
        })
        .collect::<Vec<_>>();

    if candidates.is_empty() {
        return;
    }

    let names = candidates
        .iter()
        .map(|(_, name, _)| name.clone())
        .collect::<BTreeSet<_>>();
    syn::visit_mut::VisitMut::visit_block_mut(&mut SelfResliceRewriter { names }, body);

    for (index, _, elem) in candidates {
        let Some(syn::FnArg::Typed(param)) = signature.inputs.iter_mut().nth(index) else {
            continue;
        };
        *param.ty = syn::parse_quote! {
            &mut crate::builtin::GorsSliceParam<'_, #elem>
        };
    }
}

fn mutable_slice_element(ty: &syn::Type) -> Option<syn::Type> {
    let syn::Type::Reference(reference) = ty else {
        return None;
    };
    reference.mutability.as_ref()?;
    slice_type_inner(&reference.elem)
}

fn transactional_slice_element(ty: &syn::Type) -> Option<syn::Type> {
    mutable_slice_element(ty).or_else(|| owned_slice_storage_type_inner(ty))
}

fn capacity_dependent_self_reslice(body: &syn::Block, name: &str) -> bool {
    let mut dependencies = HashMap::new();
    collect_length_dependencies(body, name, &mut dependencies);

    struct Finder<'a> {
        name: &'a str,
        dependencies: &'a HashMap<String, LengthDependency>,
        found: bool,
    }

    impl syn::visit::Visit<'_> for Finder<'_> {
        fn visit_expr_assign(&mut self, assign: &syn::ExprAssign) {
            if self.found {
                return;
            }
            if let Some(parts) = self_reslice_parts(assign, self.name) {
                let high = expression_dependency(&parts.end, self.name, self.dependencies);
                let max = parts.max.as_ref().map_or(LengthDependency::None, |max| {
                    expression_dependency(max, self.name, self.dependencies)
                });
                if high == LengthDependency::MayExceedVisibleLength
                    || max == LengthDependency::MayExceedVisibleLength
                {
                    self.found = true;
                    return;
                }
            }
            syn::visit::visit_expr_assign(self, assign);
        }
    }

    let mut finder = Finder {
        name,
        dependencies: &dependencies,
        found: false,
    };
    syn::visit::Visit::visit_block(&mut finder, body);
    finder.found
}

fn collect_length_dependencies(
    body: &syn::Block,
    param: &str,
    dependencies: &mut HashMap<String, LengthDependency>,
) {
    struct Collector<'a> {
        param: &'a str,
        dependencies: &'a mut HashMap<String, LengthDependency>,
    }

    impl syn::visit::Visit<'_> for Collector<'_> {
        fn visit_local(&mut self, local: &syn::Local) {
            if let Some(name) = pat_ident_name(&local.pat)
                && let Some(init) = &local.init
            {
                let dependency = expression_dependency(&init.expr, self.param, self.dependencies);
                if dependency != LengthDependency::None {
                    self.dependencies.insert(name, dependency);
                }
            }
            syn::visit::visit_local(self, local);
        }
    }

    syn::visit::Visit::visit_block(
        &mut Collector {
            param,
            dependencies,
        },
        body,
    );
}

fn expression_dependency(
    expr: &syn::Expr,
    param: &str,
    dependencies: &HashMap<String, LengthDependency>,
) -> LengthDependency {
    let expr = strip_paren_or_group(expr);
    if let syn::Expr::Call(call) = expr
        && (is_path_call_expr(&call.func, &["crate", "builtin", "len"])
            || is_path_call_expr(&call.func, &["crate", "builtin", "cap"]))
        && call
            .args
            .first()
            .is_some_and(|arg| expr_mentions_ident(arg, param))
    {
        return if is_path_call_expr(&call.func, &["crate", "builtin", "cap"]) {
            LengthDependency::MayExceedVisibleLength
        } else {
            LengthDependency::VisibleLength
        };
    }
    match expr {
        syn::Expr::Path(path) => path
            .path
            .get_ident()
            .and_then(|ident| dependencies.get(&ident.to_string()).copied())
            .unwrap_or(LengthDependency::None),
        syn::Expr::Binary(binary) => {
            let left = expression_dependency(&binary.left, param, dependencies);
            let right = expression_dependency(&binary.right, param, dependencies);
            if matches!(binary.op, syn::BinOp::Add(_))
                && ((left != LengthDependency::None && positive_integer_expr(&binary.right))
                    || (right != LengthDependency::None && positive_integer_expr(&binary.left)))
            {
                LengthDependency::MayExceedVisibleLength
            } else {
                left.max(right)
            }
        }
        syn::Expr::Cast(cast) => expression_dependency(&cast.expr, param, dependencies),
        syn::Expr::Reference(reference) => {
            expression_dependency(&reference.expr, param, dependencies)
        }
        syn::Expr::Unary(unary) => expression_dependency(&unary.expr, param, dependencies),
        _ => LengthDependency::None,
    }
}

fn positive_integer_expr(expr: &syn::Expr) -> bool {
    match strip_paren_or_group(expr) {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Int(value),
            ..
        }) => value.base10_parse::<u128>().is_ok_and(|value| value > 0),
        syn::Expr::Cast(cast) => positive_integer_expr(&cast.expr),
        _ => false,
    }
}

fn expr_mentions_ident(expr: &syn::Expr, name: &str) -> bool {
    struct Finder<'a> {
        name: &'a str,
        found: bool,
    }
    impl syn::visit::Visit<'_> for Finder<'_> {
        fn visit_expr_path(&mut self, path: &syn::ExprPath) {
            if path.path.is_ident(self.name) {
                self.found = true;
                return;
            }
            syn::visit::visit_expr_path(self, path);
        }
    }
    let mut finder = Finder { name, found: false };
    syn::visit::Visit::visit_expr(&mut finder, expr);
    finder.found
}

#[derive(Clone)]
struct SelfResliceParts {
    prefix: Vec<syn::Stmt>,
    start: syn::Expr,
    end: syn::Expr,
    max: Option<syn::Expr>,
}

fn self_reslice_parts(assign: &syn::ExprAssign, name: &str) -> Option<SelfResliceParts> {
    borrowed_self_reslice_parts(assign, name).or_else(|| owned_self_reslice_parts(assign, name))
}

fn borrowed_self_reslice_parts(assign: &syn::ExprAssign, name: &str) -> Option<SelfResliceParts> {
    let syn::Expr::Path(left) = strip_paren_or_group(&assign.left) else {
        return None;
    };
    if !left.path.is_ident(name) {
        return None;
    }
    let syn::Expr::Block(block) = strip_paren_or_group(&assign.right) else {
        return None;
    };
    let (last, prefix) = block.block.stmts.split_last()?;
    let syn::Stmt::Expr(syn::Expr::Reference(reference), None) = last else {
        return None;
    };
    reference.mutability.as_ref()?;
    let syn::Expr::Index(index) = strip_paren_or_group(&reference.expr) else {
        return None;
    };
    let syn::Expr::Path(base) = strip_paren_or_group(&index.expr) else {
        return None;
    };
    if !base.path.is_ident(name) {
        return None;
    }
    let syn::Expr::Range(range) = strip_paren_or_group(&index.index) else {
        return None;
    };
    let start = range
        .start
        .as_ref()
        .map(|expr| (**expr).clone())
        .unwrap_or_else(|| syn::parse_quote! { 0usize });
    let end = range
        .end
        .as_ref()
        .map(|expr| (**expr).clone())
        .unwrap_or_else(|| syn::parse_quote! { crate::builtin::len(&*#left) as usize });
    let max = prefix.iter().find_map(|stmt| {
        let syn::Stmt::Local(local) = stmt else {
            return None;
        };
        let ident = pat_ident_name(&local.pat)?;
        (ident == "__gors_slice_max").then(|| {
            let ident = syn::Ident::new(&ident, proc_macro2::Span::mixed_site());
            syn::parse_quote! { #ident }
        })
    });
    Some(SelfResliceParts {
        prefix: prefix.to_vec(),
        start,
        end,
        max,
    })
}

fn owned_self_reslice_parts(assign: &syn::ExprAssign, name: &str) -> Option<SelfResliceParts> {
    let syn::Expr::Path(left) = strip_paren_or_group(&assign.left) else {
        return None;
    };
    if !left.path.is_ident(name) {
        return None;
    }
    let syn::Expr::Block(block) = strip_paren_or_group(&assign.right) else {
        return None;
    };
    let stmts = block.block.stmts.as_slice();
    let source_index = stmts.iter().position(|stmt| {
        matches!(stmt, syn::Stmt::Local(local) if pat_ident_name(&local.pat).as_deref() == Some("__gors_slice_source"))
    })?;
    let result_index = stmts.iter().position(|stmt| {
        matches!(stmt, syn::Stmt::Local(local) if pat_ident_name(&local.pat).as_deref() == Some("__gors_slice_result"))
    })?;
    if source_index >= result_index || stmts.len() != result_index + 3 {
        return None;
    }

    let syn::Stmt::Local(source) = &stmts[source_index] else {
        return None;
    };
    let source_init = source.init.as_ref()?;
    let syn::Expr::Call(take) = strip_paren_or_group(&source_init.expr) else {
        return None;
    };
    if !is_path_call_expr(&take.func, &["std", "mem", "take"]) || take.args.len() != 1 {
        return None;
    }
    let syn::Expr::Reference(reference) = take.args.first().map(strip_paren_or_group)? else {
        return None;
    };
    reference.mutability.as_ref()?;
    if !matches!(strip_paren_or_group(&reference.expr), syn::Expr::Path(path) if path.path.is_ident(name))
    {
        return None;
    }

    let syn::Stmt::Local(result) = &stmts[result_index] else {
        return None;
    };
    let result_init = result.init.as_ref()?;
    if !matches!(strip_paren_or_group(&result_init.expr), syn::Expr::Path(path) if path.path.is_ident("__gors_slice_source"))
    {
        return None;
    }
    let syn::Stmt::Expr(syn::Expr::MethodCall(reslice), Some(_)) = &stmts[result_index + 1] else {
        return None;
    };
    if reslice.method != "reslice"
        || reslice.args.len() != 3
        || !matches!(strip_paren_or_group(&reslice.receiver), syn::Expr::Path(path) if path.path.is_ident("__gors_slice_result"))
        || !matches!(&stmts[result_index + 2], syn::Stmt::Expr(expr, None) if matches!(strip_paren_or_group(expr), syn::Expr::Path(path) if path.path.is_ident("__gors_slice_result")))
    {
        return None;
    }

    let param = syn::Ident::new(name, proc_macro2::Span::mixed_site());
    let source_ident = syn::Ident::new("__gors_slice_source", proc_macro2::Span::mixed_site());
    let mut prefix = stmts[..result_index].to_vec();
    prefix[source_index] = syn::parse_quote! {
        let #source_ident = &*#param;
    };
    Some(SelfResliceParts {
        prefix,
        start: reslice.args.first()?.clone(),
        end: reslice.args.iter().nth(1)?.clone(),
        max: Some(reslice.args.iter().nth(2)?.clone()),
    })
}

fn body_has_owned_self_reslice(body: &syn::Block, name: &str) -> bool {
    struct Finder<'a> {
        name: &'a str,
        found: bool,
    }

    impl syn::visit::Visit<'_> for Finder<'_> {
        fn visit_expr_assign(&mut self, assign: &syn::ExprAssign) {
            if owned_self_reslice_parts(assign, self.name).is_some() {
                self.found = true;
                return;
            }
            syn::visit::visit_expr_assign(self, assign);
        }
    }

    let mut finder = Finder { name, found: false };
    syn::visit::Visit::visit_block(&mut finder, body);
    finder.found
}

struct SelfResliceRewriter {
    names: BTreeSet<String>,
}

impl syn::visit_mut::VisitMut for SelfResliceRewriter {
    fn visit_expr_mut(&mut self, expr: &mut syn::Expr) {
        syn::visit_mut::visit_expr_mut(self, expr);
        let syn::Expr::Assign(assign) = expr else {
            return;
        };
        let Some((name, parts)) = self
            .names
            .iter()
            .find_map(|name| self_reslice_parts(assign, name).map(|parts| (name.clone(), parts)))
        else {
            return;
        };
        let param = syn::Ident::new(&name, proc_macro2::Span::mixed_site());
        let prefix = parts.prefix;
        let start = parts.start;
        let end = parts.end;
        let limit = parts.max.unwrap_or_else(|| {
            syn::parse_quote! { crate::builtin::cap(&*#param) as usize }
        });
        let limit_ident = synthetic_names::slice_reslice_limit_ident();
        *expr = syn::parse_quote! {{
            #(#prefix)*
            let #limit_ident = #limit;
            (#param).reslice(#start, #end, #limit_ident);
        }};
    }
}

struct CapacityCallVisitor<'a> {
    receiver_types: receiver_type_scopes::Tracker<'a>,
    functions: &'a FunctionTargets,
    methods: &'a MethodTargets,
    capacity_params: Vec<BTreeSet<String>>,
    borrowed_slice_params: Vec<BTreeSet<String>>,
}

impl syn::visit_mut::VisitMut for CapacityCallVisitor<'_> {
    fn visit_item_fn_mut(&mut self, function: &mut syn::ItemFn) {
        self.capacity_params
            .push(capacity_slice_param_names(&function.sig));
        self.borrowed_slice_params
            .push(borrowed_slice_param_names(&function.sig));
        self.receiver_types.push_scope();
        syn::visit_mut::visit_item_fn_mut(self, function);
        self.receiver_types.pop_scope();
        self.borrowed_slice_params.pop();
        self.capacity_params.pop();
    }

    fn visit_item_impl_mut(&mut self, item_impl: &mut syn::ItemImpl) {
        let previous = self.receiver_types.enter_impl(item_impl);
        syn::visit_mut::visit_item_impl_mut(self, item_impl);
        self.receiver_types.restore_impl(previous);
    }

    fn visit_impl_item_fn_mut(&mut self, function: &mut syn::ImplItemFn) {
        self.capacity_params
            .push(capacity_slice_param_names(&function.sig));
        self.borrowed_slice_params
            .push(borrowed_slice_param_names(&function.sig));
        self.receiver_types.push_scope();
        syn::visit_mut::visit_impl_item_fn_mut(self, function);
        self.receiver_types.pop_scope();
        self.borrowed_slice_params.pop();
        self.capacity_params.pop();
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
        if let Some((receiver, method)) = qself_receiver_method_call(&self.receiver_types, call)
            && let Some(indices) = self.methods.target_for_call(
                self.receiver_types.module_name(),
                &method,
                Some(&receiver),
            )
        {
            let capacity_params = self.capacity_params.last().cloned().unwrap_or_default();
            let borrowed_slice_params = self
                .borrowed_slice_params
                .last()
                .cloned()
                .unwrap_or_default();
            for (index, arg) in call.args.iter_mut().enumerate().skip(1) {
                if indices.contains(&(index - 1)) {
                    wrap_capacity_slice_arg(arg, &capacity_params, &borrowed_slice_params);
                }
            }
            return;
        }
        let Some(key) = call_target_key(&call.func, self.receiver_types.module_name()) else {
            return;
        };
        let Some(indices) = self.functions.get(&key) else {
            return;
        };
        let capacity_params = self.capacity_params.last().cloned().unwrap_or_default();
        let borrowed_slice_params = self
            .borrowed_slice_params
            .last()
            .cloned()
            .unwrap_or_default();
        for (index, arg) in call.args.iter_mut().enumerate() {
            if indices.contains(&index) {
                wrap_capacity_slice_arg(arg, &capacity_params, &borrowed_slice_params);
            }
        }
    }

    fn visit_expr_method_call_mut(&mut self, call: &mut syn::ExprMethodCall) {
        syn::visit_mut::visit_expr_method_call_mut(self, call);
        let Some(receiver) = self.receiver_types.receiver_type_for_expr(&call.receiver) else {
            return;
        };
        let Some(indices) = self.methods.target_for_call(
            self.receiver_types.module_name(),
            &call.method.to_string(),
            Some(&receiver),
        ) else {
            return;
        };
        let capacity_params = self.capacity_params.last().cloned().unwrap_or_default();
        let borrowed_slice_params = self
            .borrowed_slice_params
            .last()
            .cloned()
            .unwrap_or_default();
        for (index, arg) in call.args.iter_mut().enumerate() {
            if indices.contains(&index) {
                wrap_capacity_slice_arg(arg, &capacity_params, &borrowed_slice_params);
            }
        }
    }
}

fn qself_receiver_method_call(
    receiver_types: &receiver_type_scopes::Tracker<'_>,
    call: &syn::ExprCall,
) -> Option<(ReceiverTypeRef, String)> {
    let syn::Expr::Path(path) = call.func.as_ref() else {
        return None;
    };
    let method = path.path.segments.last()?.ident.to_string();
    let receiver = if let Some(qself) = &path.qself {
        receiver_types.receiver_type_for_type(&qself.ty)?
    } else {
        if path.path.segments.len() < 2 {
            return None;
        }
        let mut receiver_path = path.path.clone();
        receiver_path.segments.pop();
        let receiver_type = syn::Type::Path(syn::TypePath {
            qself: None,
            path: receiver_path,
        });
        receiver_types.receiver_type_for_type(&receiver_type)?
    };
    Some((receiver, method))
}

fn capacity_slice_param_names(signature: &syn::Signature) -> BTreeSet<String> {
    signature
        .inputs
        .iter()
        .filter_map(|input| {
            let syn::FnArg::Typed(param) = input else {
                return None;
            };
            let syn::Type::Reference(reference) = param.ty.as_ref() else {
                return None;
            };
            let syn::Type::Path(path) = reference.elem.as_ref() else {
                return None;
            };
            path.path
                .segments
                .last()
                .is_some_and(|segment| segment.ident == "GorsSliceParam")
                .then(|| pat_ident_name(&param.pat))
                .flatten()
        })
        .collect()
}

fn borrowed_slice_param_names(signature: &syn::Signature) -> BTreeSet<String> {
    signature
        .inputs
        .iter()
        .filter_map(|input| {
            let syn::FnArg::Typed(param) = input else {
                return None;
            };
            mutable_slice_element(&param.ty)?;
            pat_ident_name(&param.pat)
        })
        .collect()
}

fn wrap_capacity_slice_arg(
    arg: &mut syn::Expr,
    capacity_params: &BTreeSet<String>,
    borrowed_slice_params: &BTreeSet<String>,
) {
    let bridge_source = super::forwarding_adapters::owned_slice_bridge_source(arg);
    let source_is_visible = bridge_source.is_some();
    let source = bridge_source.unwrap_or_else(|| match &*arg {
        syn::Expr::Reference(reference) if reference.mutability.is_some() => {
            match strip_paren_or_group(&reference.expr) {
                syn::Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Deref(_)) => {
                    (*unary.expr).clone()
                }
                other => other.clone(),
            }
        }
        other => other.clone(),
    });
    let source = super::call_arg_rewrites::cloned_lvalue_source(&source)
        .or_else(|| super::call_arg_rewrites::cloned_lvalue_block_source(&source))
        .unwrap_or(source);
    let forwarded_name = forwarded_param_name(&source);
    if forwarded_name
        .as_ref()
        .is_some_and(|name| capacity_params.contains(name))
    {
        *arg = syn::parse_quote! {
            &mut crate::builtin::GorsSliceParam::from_param(#source)
        };
    } else if source_is_visible
        || forwarded_name.is_some_and(|name| borrowed_slice_params.contains(&name))
    {
        *arg = syn::parse_quote! {
            &mut crate::builtin::GorsSliceParam::from_storage(&mut *#source)
        };
    } else {
        *arg = syn::parse_quote! {
            &mut crate::builtin::GorsSliceParam::from_owned_storage(&mut #source)
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_only_slice_params_that_extend_from_visible_length() {
        let mut signature: syn::Signature = syn::parse_quote! {
            fn extend(mut values: &mut [i32], mut shrink: &mut [i32])
        };
        let mut body: syn::Block = syn::parse_quote! {{
            let mut n = crate::builtin::len(&values) as isize;
            values = {
                let __gors_slice_high = n + 1;
                &mut (values)[..__gors_slice_high as usize]
            };
            shrink = {
                let __gors_slice_high = 1usize;
                &mut (shrink)[..__gors_slice_high]
            };
        }};

        let params = direct_capacity_slice_params(&signature, &body);
        rewrite_capacity_slice_params(&mut signature, &mut body, &params);
        let output = quote::quote! { #signature #body }.to_string();
        let compact = output.split_whitespace().collect::<String>();
        assert_eq!(params, BTreeSet::from([0]));
        assert!(output.contains("GorsSliceParam"), "{output}");
        assert!(compact.contains("(values).reslice"), "{output}");
        assert!(output.contains("shrink ="), "{output}");
    }

    #[test]
    fn rewrites_owned_slice_params_that_rebind_headers_and_mutate_backing() {
        let mut modules = BTreeMap::from([(
            "pkg".to_string(),
            CompiledModule {
                mod_name: "pkg".to_string(),
                import_path: "pkg".to_string(),
                file: syn::parse_quote! {
                    fn pread(bytes: &mut [u8]) {
                        bytes[0] = 7;
                    }

                    fn read_at(mut bytes: crate::builtin::GorsSliceStorage<u8>) {
                        pread(&mut (bytes));
                        bytes = {
                            let __gors_slice_low = 1usize;
                            let __gors_slice_source = std::mem::take(&mut bytes);
                            let __gors_slice_end = crate::builtin::len(&__gors_slice_source)
                                as usize;
                            let __gors_slice_capacity = crate::builtin::cap(
                                &__gors_slice_source,
                            ) as usize;
                            let mut __gors_slice_result = __gors_slice_source;
                            __gors_slice_result.reslice(
                                __gors_slice_low,
                                __gors_slice_end,
                                __gors_slice_capacity,
                            );
                            __gors_slice_result
                        };
                    }

                    fn caller() {
                        let mut bytes = crate::builtin::GorsSliceStorage::from_initialized_backing(
                            vec![0u8, 0u8],
                            2usize,
                        );
                        read_at((bytes).clone());
                    }

                    trait ReaderAt {
                        fn ReadAt(&mut self, bytes: &mut [u8]);
                    }

                    struct Adapter;

                    impl ReaderAt for Adapter {
                        fn ReadAt(&mut self, bytes: &mut [u8]) {
                            read_at({
                                let __gors_owned_slice_backing = (bytes).to_vec();
                                let __gors_owned_slice_len = __gors_owned_slice_backing.len();
                                crate::builtin::GorsSliceStorage::from_initialized_backing(
                                    __gors_owned_slice_backing,
                                    __gors_owned_slice_len,
                                )
                            });
                        }
                    }
                },
                filename: "pkg.rs".to_string(),
                content_hash: "stale".to_string(),
                is_main: false,
                is_stdlib: false,
            },
        )]);

        preserve_capacity_for_resliced_slice_params(&mut modules);

        let module = modules.get("pkg").unwrap();
        let output = prettyplease::unparse(&module.file);
        let compact = output.split_whitespace().collect::<String>();
        assert!(
            compact.contains("fnread_at(mutbytes:&mutcrate::builtin::GorsSliceParam<'_,u8>)"),
            "{output}"
        );
        assert!(compact.contains("(bytes).reslice("), "{output}");
        assert!(
            compact.contains(
                "read_at(&mutcrate::builtin::GorsSliceParam::from_owned_storage(&mut(bytes)))"
            ),
            "{output}"
        );
        assert!(
            !compact.contains("from_owned_storage(&mut(bytes).clone())"),
            "{output}"
        );
        assert!(
            compact.contains("GorsSliceParam::from_storage(&mut*(bytes))"),
            "{output}"
        );
        assert!(!compact.contains("(bytes).to_vec()"), "{output}");
        assert!(module.content_hash.is_empty() || module.content_hash == "stale");
    }
}
