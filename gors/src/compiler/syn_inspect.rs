pub(super) fn path_starts_with(path: &syn::Path, expected: &[&str]) -> bool {
    if path.segments.len() < expected.len() {
        return false;
    }
    path.segments
        .iter()
        .zip(expected)
        .all(|(segment, expected)| segment.ident == *expected)
}

pub(super) fn path_is(path: &syn::Path, expected: &[&str]) -> bool {
    path.segments.len() == expected.len() && path_starts_with(path, expected)
}

pub(super) fn path_ends_with(path: &syn::Path, expected: &[&str]) -> bool {
    if path.segments.len() < expected.len() {
        return false;
    }
    path.segments
        .iter()
        .rev()
        .zip(expected.iter().rev())
        .all(|(segment, expected)| segment.ident == *expected)
}

pub(super) fn is_path_call_expr(func: &syn::Expr, segments: &[&str]) -> bool {
    let syn::Expr::Path(path) = func else {
        return false;
    };
    path_is(&path.path, segments)
}

pub(super) fn call_expr_path_last_ident(expr: &syn::Expr, name: &str) -> bool {
    let syn::Expr::Call(call) = expr else {
        return false;
    };
    let syn::Expr::Path(path) = call.func.as_ref() else {
        return false;
    };
    path.path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == name)
}

pub(super) fn call_target_key(func: &syn::Expr, current_module: &str) -> Option<String> {
    let syn::Expr::Path(path) = func else {
        return None;
    };
    if path.qself.is_some() {
        return None;
    }
    let segments = path
        .path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>();
    match segments.as_slice() {
        [name] => Some(format!("{current_module}::{name}")),
        [.., module, name] => Some(format!("{module}::{name}")),
        [] => None,
    }
}

pub(super) fn type_path_ident_name(ty: &syn::Type) -> Option<String> {
    let syn::Type::Path(path) = ty else {
        return None;
    };
    if path.qself.is_some() || path.path.segments.len() != 1 {
        return None;
    }
    path.path
        .segments
        .first()
        .map(|segment| segment.ident.to_string())
}

pub(super) fn expr_path_ident(expr: &syn::Expr) -> Option<String> {
    let syn::Expr::Path(path) = expr else {
        return None;
    };
    path.path.get_ident().map(ToString::to_string)
}

pub(super) fn path_ident_name(expr: &syn::Expr) -> Option<String> {
    match expr {
        syn::Expr::Path(path) => {
            if path.qself.is_some() || path.path.segments.len() != 1 {
                return None;
            }
            path.path
                .segments
                .first()
                .map(|segment| segment.ident.to_string())
        }
        syn::Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Deref(_)) => {
            path_ident_name(&unary.expr)
        }
        syn::Expr::Paren(paren) => path_ident_name(&paren.expr),
        syn::Expr::Group(group) => path_ident_name(&group.expr),
        _ => None,
    }
}

pub(super) fn is_self_expr(expr: &syn::Expr) -> bool {
    is_path_ident(expr, "self")
}

pub(super) fn is_self_or_ref_self_expr(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Paren(paren) => is_self_or_ref_self_expr(&paren.expr),
        syn::Expr::Group(group) => is_self_or_ref_self_expr(&group.expr),
        syn::Expr::Reference(reference) => is_self_or_ref_self_expr(&reference.expr),
        _ => is_self_expr(expr),
    }
}

pub(super) fn is_path_ident(expr: &syn::Expr, name: &str) -> bool {
    matches!(expr, syn::Expr::Path(path)
        if path.path.leading_colon.is_none()
            && path.path.segments.len() == 1
            && path.path.segments.first().is_some_and(|seg| seg.ident == name))
}

pub(super) fn expr_is_ident(expr: &syn::Expr, ident: &syn::Ident) -> bool {
    let syn::Expr::Path(path) = expr else {
        return false;
    };
    path.qself.is_none()
        && path.path.leading_colon.is_none()
        && path.path.segments.len() == 1
        && path
            .path
            .segments
            .first()
            .is_some_and(|segment| segment.ident == *ident)
}

pub(super) fn is_slice_range_index_expr(expr: &syn::Expr) -> bool {
    matches!(
        expr,
        syn::Expr::Index(index) if matches!(&*index.index, syn::Expr::Range(_))
    )
}

pub(super) fn strip_paren_or_group(mut expr: &syn::Expr) -> &syn::Expr {
    loop {
        match expr {
            syn::Expr::Paren(paren) => expr = &paren.expr,
            syn::Expr::Group(group) => expr = &group.expr,
            _ => return expr,
        }
    }
}

pub(super) fn zero_arg_method_call_receiver_expr<'a>(
    expr: &'a syn::Expr,
    method_name: &str,
) -> Option<&'a syn::Expr> {
    let syn::Expr::MethodCall(method) = expr else {
        return None;
    };
    if method.method != method_name || !method.args.is_empty() {
        return None;
    }
    Some(&method.receiver)
}

pub(super) fn ident_matches_any(ident: &syn::Ident, names: &[&str]) -> bool {
    names.iter().any(|name| ident == *name)
}

pub(super) fn is_receiver_type_wrapper_method(method: &syn::Ident) -> bool {
    ident_matches_any(method, &["clone", "lock", "to_string", "unwrap"])
}

pub(super) fn is_lock_guard_wrapper_method(method: &syn::Ident) -> bool {
    ident_matches_any(method, &["lock", "unwrap"])
}

pub(super) fn direct_clone_call_receiver_expr(expr: &syn::Expr) -> Option<syn::Expr> {
    zero_arg_method_call_receiver_expr(expr, "clone").cloned()
}

pub(super) fn clone_call_receiver_expr(expr: &syn::Expr) -> Option<syn::Expr> {
    direct_clone_call_receiver_expr(expr).or_else(|| {
        let syn::Expr::Paren(paren) = expr else {
            return None;
        };
        clone_call_receiver_expr(&paren.expr)
    })
}

pub(super) fn stripped_clone_call_receiver_expr(expr: &syn::Expr) -> Option<syn::Expr> {
    direct_clone_call_receiver_expr(strip_paren_or_group(expr))
}

pub(super) fn is_clone_call_expr(expr: &syn::Expr) -> bool {
    stripped_clone_call_receiver_expr(expr).is_some()
}

pub(super) fn expr_path_ident_or_clone(expr: &syn::Expr) -> Option<String> {
    if let Some(receiver) = direct_clone_call_receiver_expr(expr) {
        return expr_path_ident_or_clone(&receiver);
    }
    match expr {
        syn::Expr::Group(group) => expr_path_ident_or_clone(&group.expr),
        syn::Expr::Paren(paren) => expr_path_ident_or_clone(&paren.expr),
        _ => expr_path_ident(expr),
    }
}

pub(super) fn receiver_root_ident_name(expr: &syn::Expr) -> Option<String> {
    match expr {
        syn::Expr::Path(_) => path_ident_name(expr),
        syn::Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Deref(_)) => {
            receiver_root_ident_name(&unary.expr)
        }
        syn::Expr::Paren(paren) => receiver_root_ident_name(&paren.expr),
        syn::Expr::Group(group) => receiver_root_ident_name(&group.expr),
        syn::Expr::Field(field) => receiver_root_ident_name(&field.base),
        syn::Expr::Reference(reference) => receiver_root_ident_name(&reference.expr),
        syn::Expr::MethodCall(method)
            if method.args.is_empty() && is_transparent_receiver_method(&method.method) =>
        {
            receiver_root_ident_name(&method.receiver)
        }
        _ => None,
    }
}

fn is_transparent_receiver_method(method: &syn::Ident) -> bool {
    ident_matches_any(method, &["as_mut", "as_ref"]) || is_receiver_type_wrapper_method(method)
}

pub(super) fn mut_borrowed_path_name(expr: &syn::Expr) -> Option<String> {
    let syn::Expr::Reference(reference) = expr else {
        return None;
    };
    reference.mutability.as_ref()?;
    path_ident_name(&reference.expr)
}

pub(super) fn expr_contains_path_ident(expr: &syn::Expr, name: &str) -> bool {
    struct Finder<'a> {
        name: &'a str,
        found: bool,
    }

    impl syn::visit::Visit<'_> for Finder<'_> {
        fn visit_expr_path(&mut self, path: &syn::ExprPath) {
            if path.path.leading_colon.is_none()
                && path.path.segments.len() == 1
                && path
                    .path
                    .segments
                    .first()
                    .is_some_and(|segment| segment.ident == self.name)
            {
                self.found = true;
            }
            syn::visit::visit_expr_path(self, path);
        }
    }

    let mut finder = Finder { name, found: false };
    syn::visit::Visit::visit_expr(&mut finder, expr);
    finder.found
}

pub(super) fn expr_contains_any_path_ident(
    expr: &syn::Expr,
    names: &std::collections::HashSet<String>,
) -> bool {
    names
        .iter()
        .any(|name| expr_contains_path_ident(expr, name))
}

pub(super) fn expr_contains_method_call(expr: &syn::Expr, method: &str) -> bool {
    struct Finder<'a> {
        method: &'a str,
        found: bool,
    }

    impl syn::visit::Visit<'_> for Finder<'_> {
        fn visit_expr_method_call(&mut self, call: &syn::ExprMethodCall) {
            if call.method == self.method {
                self.found = true;
            }
            syn::visit::visit_expr_method_call(self, call);
        }
    }

    let mut finder = Finder {
        method,
        found: false,
    };
    syn::visit::Visit::visit_expr(&mut finder, expr);
    finder.found
}

pub(super) fn receiver_expr_needs_scoped_temp(expr: &syn::Expr) -> bool {
    struct LockUnwrapFinder {
        found: bool,
    }

    impl syn::visit::Visit<'_> for LockUnwrapFinder {
        fn visit_expr_method_call(&mut self, method_call: &syn::ExprMethodCall) {
            if self.found {
                return;
            }
            if method_call.method == "unwrap"
                && method_call.args.is_empty()
                && let syn::Expr::MethodCall(lock_call) = method_call.receiver.as_ref()
                && lock_call.method == "lock"
                && lock_call.args.is_empty()
            {
                self.found = true;
                return;
            }
            syn::visit::visit_expr_method_call(self, method_call);
        }
    }

    let mut finder = LockUnwrapFinder { found: false };
    syn::visit::Visit::visit_expr(&mut finder, expr);
    finder.found
}

pub(super) fn syn_expr_matches_target(expr: &syn::Expr, target: &syn::Expr) -> bool {
    match (strip_paren_or_group(expr), strip_paren_or_group(target)) {
        (syn::Expr::Path(left), syn::Expr::Path(right)) => {
            left.qself.is_none()
                && right.qself.is_none()
                && syn_path_matches(&left.path, &right.path)
        }
        (syn::Expr::Field(left), syn::Expr::Field(right)) => {
            syn_expr_matches_target(&left.base, &right.base)
                && syn_member_matches(&left.member, &right.member)
        }
        (syn::Expr::Index(left), syn::Expr::Index(right)) => {
            syn_expr_matches_target(&left.expr, &right.expr)
                && syn_expr_matches_target(&left.index, &right.index)
        }
        (syn::Expr::Reference(left), syn::Expr::Reference(right)) => {
            left.mutability.is_some() == right.mutability.is_some()
                && syn_expr_matches_target(&left.expr, &right.expr)
        }
        (syn::Expr::Unary(left), syn::Expr::Unary(right)) => {
            syn_unop_matches(&left.op, &right.op)
                && syn_expr_matches_target(&left.expr, &right.expr)
        }
        (syn::Expr::MethodCall(left), syn::Expr::MethodCall(right)) => {
            left.method == right.method
                && left.turbofish.is_none()
                && right.turbofish.is_none()
                && syn_expr_matches_target(&left.receiver, &right.receiver)
                && syn_expr_args_match(&left.args, &right.args)
        }
        (syn::Expr::Call(left), syn::Expr::Call(right)) => {
            syn_expr_matches_target(&left.func, &right.func)
                && syn_expr_args_match(&left.args, &right.args)
        }
        (syn::Expr::Cast(left), syn::Expr::Cast(right)) => {
            syn_expr_matches_target(&left.expr, &right.expr)
                && syn_type_matches(&left.ty, &right.ty)
        }
        (syn::Expr::Lit(left), syn::Expr::Lit(right)) => syn_lit_matches(&left.lit, &right.lit),
        (syn::Expr::Tuple(left), syn::Expr::Tuple(right)) => {
            left.elems.len() == right.elems.len()
                && left
                    .elems
                    .iter()
                    .zip(right.elems.iter())
                    .all(|(left, right)| syn_expr_matches_target(left, right))
        }
        _ => false,
    }
}

fn syn_expr_args_match(
    left: &syn::punctuated::Punctuated<syn::Expr, syn::token::Comma>,
    right: &syn::punctuated::Punctuated<syn::Expr, syn::token::Comma>,
) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right.iter())
            .all(|(left, right)| syn_expr_matches_target(left, right))
}

pub(super) fn syn_path_matches(left: &syn::Path, right: &syn::Path) -> bool {
    left.leading_colon.is_some() == right.leading_colon.is_some()
        && left.segments.len() == right.segments.len()
        && left
            .segments
            .iter()
            .zip(right.segments.iter())
            .all(|(left, right)| {
                left.ident == right.ident
                    && syn_path_arguments_match(&left.arguments, &right.arguments)
            })
}

fn syn_path_arguments_match(left: &syn::PathArguments, right: &syn::PathArguments) -> bool {
    match (left, right) {
        (syn::PathArguments::None, syn::PathArguments::None) => true,
        (syn::PathArguments::AngleBracketed(left), syn::PathArguments::AngleBracketed(right)) => {
            left.args.len() == right.args.len()
                && left
                    .args
                    .iter()
                    .zip(right.args.iter())
                    .all(|(left, right)| syn_generic_argument_matches(left, right))
        }
        (syn::PathArguments::Parenthesized(left), syn::PathArguments::Parenthesized(right)) => {
            left.inputs.len() == right.inputs.len()
                && left
                    .inputs
                    .iter()
                    .zip(right.inputs.iter())
                    .all(|(left, right)| syn_type_matches(left, right))
                && syn_return_type_matches(&left.output, &right.output)
        }
        _ => false,
    }
}

fn syn_generic_argument_matches(left: &syn::GenericArgument, right: &syn::GenericArgument) -> bool {
    match (left, right) {
        (syn::GenericArgument::Lifetime(left), syn::GenericArgument::Lifetime(right)) => {
            left.ident == right.ident
        }
        (syn::GenericArgument::Type(left), syn::GenericArgument::Type(right)) => {
            syn_type_matches(left, right)
        }
        (syn::GenericArgument::Const(left), syn::GenericArgument::Const(right)) => {
            syn_expr_matches_target(left, right)
        }
        _ => false,
    }
}

fn syn_return_type_matches(left: &syn::ReturnType, right: &syn::ReturnType) -> bool {
    match (left, right) {
        (syn::ReturnType::Default, syn::ReturnType::Default) => true,
        (syn::ReturnType::Type(_, left), syn::ReturnType::Type(_, right)) => {
            syn_type_matches(left, right)
        }
        _ => false,
    }
}

fn syn_member_matches(left: &syn::Member, right: &syn::Member) -> bool {
    match (left, right) {
        (syn::Member::Named(left), syn::Member::Named(right)) => left == right,
        (syn::Member::Unnamed(left), syn::Member::Unnamed(right)) => left.index == right.index,
        _ => false,
    }
}

fn syn_unop_matches(left: &syn::UnOp, right: &syn::UnOp) -> bool {
    matches!(
        (left, right),
        (syn::UnOp::Deref(_), syn::UnOp::Deref(_))
            | (syn::UnOp::Not(_), syn::UnOp::Not(_))
            | (syn::UnOp::Neg(_), syn::UnOp::Neg(_))
    )
}

pub(super) fn syn_type_matches(left: &syn::Type, right: &syn::Type) -> bool {
    match (left, right) {
        (syn::Type::Path(left), syn::Type::Path(right)) => {
            left.qself.is_none()
                && right.qself.is_none()
                && syn_path_matches(&left.path, &right.path)
        }
        (syn::Type::Reference(left), syn::Type::Reference(right)) => {
            left.mutability.is_some() == right.mutability.is_some()
                && syn_type_matches(&left.elem, &right.elem)
        }
        (syn::Type::Tuple(left), syn::Type::Tuple(right)) => {
            left.elems.len() == right.elems.len()
                && left
                    .elems
                    .iter()
                    .zip(right.elems.iter())
                    .all(|(left, right)| syn_type_matches(left, right))
        }
        _ => false,
    }
}

pub(super) fn dedupe_syn_types(types: &mut Vec<syn::Type>) {
    let mut deduped = Vec::new();
    for ty in std::mem::take(types) {
        if !deduped
            .iter()
            .any(|existing| syn_type_matches(existing, &ty))
        {
            deduped.push(ty);
        }
    }
    *types = deduped;
}

pub(super) fn impl_trait_targets_match(left: &syn::Item, right: &syn::Item) -> bool {
    let (syn::Item::Impl(left), syn::Item::Impl(right)) = (left, right) else {
        return false;
    };
    let (Some((left_polarity, left_trait, _)), Some((right_polarity, right_trait, _))) =
        (&left.trait_, &right.trait_)
    else {
        return false;
    };
    left_polarity.is_some() == right_polarity.is_some()
        && syn_path_matches(left_trait, right_trait)
        && syn_type_matches(&left.self_ty, &right.self_ty)
}

pub(super) fn type_param_bound_matches(
    left: &syn::TypeParamBound,
    right: &syn::TypeParamBound,
) -> bool {
    match (left, right) {
        (syn::TypeParamBound::Trait(left), syn::TypeParamBound::Trait(right)) => {
            trait_bound_modifier_matches(&left.modifier, &right.modifier)
                && left.paren_token.is_some() == right.paren_token.is_some()
                && bound_lifetimes_match(left.lifetimes.as_ref(), right.lifetimes.as_ref())
                && syn_path_matches(&left.path, &right.path)
        }
        (syn::TypeParamBound::Lifetime(left), syn::TypeParamBound::Lifetime(right)) => {
            left.ident == right.ident
        }
        _ => false,
    }
}

fn trait_bound_modifier_matches(
    left: &syn::TraitBoundModifier,
    right: &syn::TraitBoundModifier,
) -> bool {
    matches!(
        (left, right),
        (syn::TraitBoundModifier::None, syn::TraitBoundModifier::None)
            | (
                syn::TraitBoundModifier::Maybe(_),
                syn::TraitBoundModifier::Maybe(_)
            )
    )
}

fn bound_lifetimes_match(
    left: Option<&syn::BoundLifetimes>,
    right: Option<&syn::BoundLifetimes>,
) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => {
            left.lifetimes.len() == right.lifetimes.len()
                && left
                    .lifetimes
                    .iter()
                    .zip(right.lifetimes.iter())
                    .all(|(left, right)| generic_param_matches(left, right))
        }
        _ => false,
    }
}

fn generic_param_matches(left: &syn::GenericParam, right: &syn::GenericParam) -> bool {
    match (left, right) {
        (syn::GenericParam::Lifetime(left), syn::GenericParam::Lifetime(right)) => {
            left.lifetime.ident == right.lifetime.ident
                && left.bounds.len() == right.bounds.len()
                && left
                    .bounds
                    .iter()
                    .zip(right.bounds.iter())
                    .all(|(left, right)| left.ident == right.ident)
        }
        (syn::GenericParam::Type(left), syn::GenericParam::Type(right)) => {
            left.ident == right.ident
                && left.bounds.len() == right.bounds.len()
                && left
                    .bounds
                    .iter()
                    .zip(right.bounds.iter())
                    .all(|(left, right)| type_param_bound_matches(left, right))
        }
        (syn::GenericParam::Const(left), syn::GenericParam::Const(right)) => {
            left.ident == right.ident && syn_type_matches(&left.ty, &right.ty)
        }
        _ => false,
    }
}

fn syn_lit_matches(left: &syn::Lit, right: &syn::Lit) -> bool {
    match (left, right) {
        (syn::Lit::Str(left), syn::Lit::Str(right)) => left.value() == right.value(),
        (syn::Lit::ByteStr(left), syn::Lit::ByteStr(right)) => left.value() == right.value(),
        (syn::Lit::Byte(left), syn::Lit::Byte(right)) => left.value() == right.value(),
        (syn::Lit::Char(left), syn::Lit::Char(right)) => left.value() == right.value(),
        (syn::Lit::Int(left), syn::Lit::Int(right)) => {
            left.base10_digits() == right.base10_digits() && left.suffix() == right.suffix()
        }
        (syn::Lit::Float(left), syn::Lit::Float(right)) => {
            left.base10_digits() == right.base10_digits() && left.suffix() == right.suffix()
        }
        (syn::Lit::Bool(left), syn::Lit::Bool(right)) => left.value == right.value,
        _ => false,
    }
}

pub(super) fn is_box_new_call(expr: &syn::Expr) -> bool {
    matches!(expr, syn::Expr::Call(call) if is_path_call_expr(&call.func, &["Box", "new"]))
}

pub(super) fn is_box_leak_expr(expr: &syn::Expr) -> bool {
    matches!(expr, syn::Expr::Call(call) if is_path_call_expr(&call.func, &["Box", "leak"]))
}

pub(super) fn is_box_new_unit_expr(expr: &syn::Expr) -> bool {
    let expr = strip_paren_or_group(expr);
    let Some(arg) = single_call_arg_for_suffix_path(expr, &["Box", "new"]) else {
        return false;
    };
    expr_is_unit(arg)
}

pub(super) fn is_box_dyn_any_expr(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Cast(cast) => {
            is_box_type_with_any_bound(&cast.ty) && is_box_new_unit_expr(&cast.expr)
        }
        syn::Expr::Paren(paren) => is_box_dyn_any_expr(&paren.expr),
        syn::Expr::Group(group) => is_box_dyn_any_expr(&group.expr),
        _ => false,
    }
}

pub(super) fn box_dyn_any_cast_source_expr(expr: &syn::Expr) -> Option<syn::Expr> {
    match expr {
        syn::Expr::Cast(cast) if is_box_type_with_any_bound(&cast.ty) => Some((*cast.expr).clone()),
        syn::Expr::Paren(paren) => box_dyn_any_cast_source_expr(&paren.expr),
        syn::Expr::Group(group) => box_dyn_any_cast_source_expr(&group.expr),
        _ => None,
    }
}

pub(super) fn arc_mutex_new_inner_expr(expr: &syn::Expr) -> Option<syn::Expr> {
    let mutex_call = single_call_arg_for_path(expr, &["std", "sync", "Arc", "new"])?;
    single_call_arg_for_path(mutex_call, &["std", "sync", "Mutex", "new"]).cloned()
}

fn single_call_arg_for_path<'a>(expr: &'a syn::Expr, segments: &[&str]) -> Option<&'a syn::Expr> {
    let syn::Expr::Call(call) = expr else {
        return None;
    };
    if !is_path_call_expr(&call.func, segments) || call.args.len() != 1 {
        return None;
    }
    call.args.first()
}

fn single_call_arg_for_suffix_path<'a>(
    expr: &'a syn::Expr,
    segments: &[&str],
) -> Option<&'a syn::Expr> {
    let syn::Expr::Call(call) = expr else {
        return None;
    };
    let syn::Expr::Path(path) = &*call.func else {
        return None;
    };
    if path.qself.is_some() || !path_ends_with(&path.path, segments) || call.args.len() != 1 {
        return None;
    }
    call.args.first()
}

fn expr_is_unit(expr: &syn::Expr) -> bool {
    match strip_paren_or_group(expr) {
        syn::Expr::Tuple(tuple) => tuple.elems.is_empty(),
        _ => false,
    }
}

pub(super) fn pat_ident_name(pat: &syn::Pat) -> Option<String> {
    match pat {
        syn::Pat::Ident(pat_ident) => Some(pat_ident.ident.to_string()),
        syn::Pat::Type(pat_type) => pat_ident_name(&pat_type.pat),
        _ => None,
    }
}

pub(super) fn pat_ident_names(pat: &syn::Pat) -> Vec<String> {
    match pat {
        syn::Pat::Ident(ident) => vec![ident.ident.to_string()],
        syn::Pat::Reference(reference) => pat_ident_names(&reference.pat),
        syn::Pat::Tuple(tuple) => tuple.elems.iter().flat_map(pat_ident_names).collect(),
        syn::Pat::TupleStruct(tuple) => tuple.elems.iter().flat_map(pat_ident_names).collect(),
        syn::Pat::Type(pat_type) => pat_ident_names(&pat_type.pat),
        _ => Vec::new(),
    }
}

pub(super) fn fn_arg_ident(arg: &syn::FnArg) -> Option<syn::Ident> {
    let syn::FnArg::Typed(pat_type) = arg else {
        return None;
    };
    let syn::Pat::Ident(pat_ident) = &*pat_type.pat else {
        return None;
    };
    Some(pat_ident.ident.clone())
}

pub(super) fn vec_type_inner(ty: &syn::Type) -> Option<syn::Type> {
    let syn::Type::Path(path) = ty else {
        return None;
    };
    if path.qself.is_some() || path.path.segments.len() != 1 {
        return None;
    }
    first_type_arg_if_path_last_ident(ty, "Vec").cloned()
}

pub(super) fn slice_type_inner(ty: &syn::Type) -> Option<syn::Type> {
    let syn::Type::Slice(slice) = ty else {
        return None;
    };
    Some((*slice.elem).clone())
}

pub(super) fn first_type_arg_if_path_last_ident<'a>(
    ty: &'a syn::Type,
    ident: &str,
) -> Option<&'a syn::Type> {
    let syn::Type::Path(path) = ty else {
        return None;
    };
    first_type_arg_for_path_last_ident_any(&path.path, &[ident])
}

pub(super) fn first_type_arg_for_path_last_ident_any<'a>(
    path: &'a syn::Path,
    names: &[&str],
) -> Option<&'a syn::Type> {
    let segment = path.segments.last()?;
    if !ident_matches_any(&segment.ident, names) {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
        return None;
    };
    args.args.iter().find_map(|arg| {
        if let syn::GenericArgument::Type(ty) = arg {
            Some(ty)
        } else {
            None
        }
    })
}

pub(super) fn is_box_dyn_any_type(ty: &syn::Type) -> bool {
    let syn::Type::Path(type_path) = ty else {
        return false;
    };
    if type_path.qself.is_some() || type_path.path.segments.len() != 1 {
        return false;
    }
    let Some(segment) = type_path.path.segments.first() else {
        return false;
    };
    if segment.ident != "Box" {
        return false;
    }
    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
        return false;
    };
    let Some(syn::GenericArgument::Type(syn::Type::TraitObject(trait_object))) = args.args.first()
    else {
        return false;
    };
    trait_object_has_any_bound(trait_object)
}

pub(super) fn is_box_type_with_any_bound(ty: &syn::Type) -> bool {
    let syn::Type::Path(type_path) = ty else {
        return false;
    };
    let Some(first) = type_path.path.segments.first() else {
        return false;
    };
    if first.ident != "Box" {
        return false;
    }
    let syn::PathArguments::AngleBracketed(args) = &first.arguments else {
        return false;
    };
    args.args.iter().any(|arg| {
        matches!(arg, syn::GenericArgument::Type(syn::Type::TraitObject(to)) if trait_object_has_any_bound(to))
    })
}

fn trait_object_has_any_bound(trait_object: &syn::TypeTraitObject) -> bool {
    trait_object.bounds.iter().any(|bound| {
        matches!(bound, syn::TypeParamBound::Trait(trait_bound)
            if path_ends_with(&trait_bound.path, &["Any"]))
    })
}

pub(super) fn named_self_type(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(path) => named_self_type_from_path(&path.path),
        syn::Type::Reference(reference) => named_self_type(&reference.elem),
        _ => None,
    }
}

fn named_self_type_from_path(path: &syn::Path) -> Option<String> {
    let last = path.segments.last()?;
    if let Some(inner) = first_type_arg_for_path_last_ident_any(path, &["Arc", "Mutex", "GorsPtr"])
        && let Some(name) = named_self_type(inner)
    {
        return Some(name);
    }
    Some(last.ident.to_string())
}

fn direct_self_type(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(path) => path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string()),
        syn::Type::Reference(reference) => direct_self_type(&reference.elem),
        _ => None,
    }
}

pub(super) fn self_type_reachability_names(ty: &syn::Type) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(name) = named_self_type(ty) {
        names.push(name);
    }
    if let Some(name) = direct_self_type(ty)
        && !names.contains(&name)
    {
        names.push(name);
    }
    for name in qualified_self_type_names(ty) {
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

fn qualified_self_type_names(ty: &syn::Type) -> Vec<String> {
    match ty {
        syn::Type::Path(path) => qualified_self_type_names_from_path(&path.path),
        syn::Type::Reference(reference) => qualified_self_type_names(&reference.elem),
        _ => Vec::new(),
    }
}

fn qualified_self_type_names_from_path(path: &syn::Path) -> Vec<String> {
    if let Some(inner) = first_type_arg_for_path_last_ident_any(path, &["Arc", "Mutex", "GorsPtr"])
    {
        return qualified_self_type_names(inner);
    }
    let segments = path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>();
    match segments.as_slice() {
        [crate_segment, module, symbol] if crate_segment == "crate" => vec![
            format!("{module}::{symbol}"),
            format!("crate::{module}::{symbol}"),
        ],
        [module, symbol] => vec![format!("{module}::{symbol}")],
        _ => Vec::new(),
    }
}

pub(super) fn item_name(item: &syn::Item) -> Option<String> {
    match item {
        syn::Item::Const(item) => Some(item.ident.to_string()),
        syn::Item::Enum(item) => Some(item.ident.to_string()),
        syn::Item::Fn(item) => Some(item.sig.ident.to_string()),
        syn::Item::Static(item) => Some(item.ident.to_string()),
        syn::Item::Struct(item) => Some(item.ident.to_string()),
        syn::Item::Trait(item) => Some(item.ident.to_string()),
        syn::Item::Type(item) => Some(item.ident.to_string()),
        syn::Item::Union(item) => Some(item.ident.to_string()),
        syn::Item::Macro(item) => item_macro_name(item),
        _ => None,
    }
}

pub(super) fn item_macro_name(item: &syn::ItemMacro) -> Option<String> {
    item.ident
        .as_ref()
        .map(std::string::ToString::to_string)
        .or_else(|| {
            item.mac
                .path
                .segments
                .last()
                .map(|seg| seg.ident.to_string())
        })
}

pub(super) fn macro_token_item_names(
    tokens: &proc_macro2::TokenStream,
    item_names: &std::collections::HashSet<String>,
) -> std::collections::HashSet<String> {
    fn collect(
        tokens: proc_macro2::TokenStream,
        item_names: &std::collections::HashSet<String>,
        names: &mut std::collections::HashSet<String>,
    ) {
        for token in tokens {
            match token {
                proc_macro2::TokenTree::Ident(ident) => {
                    let name = ident.to_string();
                    if item_names.contains(&name) {
                        names.insert(name);
                    }
                }
                proc_macro2::TokenTree::Group(group) => {
                    collect(group.stream(), item_names, names);
                }
                proc_macro2::TokenTree::Literal(_) | proc_macro2::TokenTree::Punct(_) => {}
            }
        }
    }

    let mut names = std::collections::HashSet::new();
    collect(tokens.clone(), item_names, &mut names);
    names
}

pub(super) fn type_mentions_name(
    ty: &syn::Type,
    names: &std::collections::HashSet<String>,
) -> bool {
    match ty {
        syn::Type::Array(array) => type_mentions_name(&array.elem, names),
        syn::Type::Group(group) => type_mentions_name(&group.elem, names),
        syn::Type::Paren(paren) => type_mentions_name(&paren.elem, names),
        syn::Type::Path(path) => path_mentions_name(&path.path, names),
        syn::Type::Reference(reference) => type_mentions_name(&reference.elem, names),
        syn::Type::Ptr(ptr) => type_mentions_name(&ptr.elem, names),
        syn::Type::Slice(slice) => type_mentions_name(&slice.elem, names),
        syn::Type::TraitObject(trait_object) => {
            trait_object.bounds.iter().any(|bound| match bound {
                syn::TypeParamBound::Trait(trait_bound) => {
                    path_mentions_name(&trait_bound.path, names)
                }
                _ => false,
            })
        }
        syn::Type::Tuple(tuple) => tuple.elems.iter().any(|ty| type_mentions_name(ty, names)),
        _ => false,
    }
}

pub(super) fn path_mentions_name(
    path: &syn::Path,
    names: &std::collections::HashSet<String>,
) -> bool {
    path.segments.iter().any(|seg| {
        names.contains(&seg.ident.to_string())
            || match &seg.arguments {
                syn::PathArguments::AngleBracketed(args) => {
                    args.args.iter().any(|arg| match arg {
                        syn::GenericArgument::Type(ty) => type_mentions_name(ty, names),
                        syn::GenericArgument::AssocType(assoc) => {
                            type_mentions_name(&assoc.ty, names)
                        }
                        syn::GenericArgument::Constraint(constraint) => {
                            constraint.bounds.iter().any(|bound| match bound {
                                syn::TypeParamBound::Trait(trait_bound) => {
                                    path_mentions_name(&trait_bound.path, names)
                                }
                                _ => false,
                            })
                        }
                        _ => false,
                    })
                }
                syn::PathArguments::Parenthesized(args) => {
                    args.inputs.iter().any(|ty| type_mentions_name(ty, names))
                        || matches!(&args.output, syn::ReturnType::Type(_, ty) if type_mentions_name(ty, names))
                }
                syn::PathArguments::None => false,
            }
    })
}

pub(super) fn trait_method_fns(
    items: &[syn::Item],
) -> std::collections::BTreeMap<String, Vec<syn::TraitItemFn>> {
    let mut traits = std::collections::BTreeMap::new();
    for item in items {
        let syn::Item::Trait(item_trait) = item else {
            continue;
        };
        let methods = item_trait
            .items
            .iter()
            .filter_map(|trait_item| match trait_item {
                syn::TraitItem::Fn(trait_fn) => Some(trait_fn.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        if !methods.is_empty() {
            traits.insert(item_trait.ident.to_string(), methods);
        }
    }
    traits
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::ToTokens;
    use syn::parse_quote;

    #[test]
    fn is_path_call_expr_matches_exact_path_segments() {
        let path: syn::Path = parse_quote! { crate::std::mem::take };
        assert!(path_starts_with(&path, &["crate", "std"]));
        assert!(path_is(&path, &["crate", "std", "mem", "take"]));
        assert!(!path_is(&path, &["std", "mem", "take"]));
        assert!(path_ends_with(&path, &["std", "mem", "take"]));
        assert!(!path_ends_with(&path, &["other", "take"]));

        let expr: syn::Expr = parse_quote! { std::mem::take };
        assert!(is_path_call_expr(&expr, &["std", "mem", "take"]));
        assert!(!is_path_call_expr(&expr, &["mem", "take"]));
        assert!(!is_path_call_expr(&expr, &["std", "mem"]));

        let call: syn::Expr = parse_quote! { std::mem::take(value) };
        assert!(!is_path_call_expr(&call, &["std", "mem", "take"]));
        assert!(call_expr_path_last_ident(&call, "take"));
        assert!(!call_expr_path_last_ident(&call, "mem"));

        let method_call: syn::Expr = parse_quote! { value.take() };
        assert!(!call_expr_path_last_ident(&method_call, "take"));
    }

    #[test]
    fn call_target_key_normalizes_generated_function_paths() {
        let local: syn::Expr = parse_quote! { sort };
        let module: syn::Expr = parse_quote! { helper::sort };
        let crate_module: syn::Expr = parse_quote! { crate::helper::sort };
        let ufcs_method: syn::Expr = parse_quote! { <helper>::sort };
        let call: syn::Expr = parse_quote! { sort(values) };

        assert_eq!(
            call_target_key(&local, "main").as_deref(),
            Some("main::sort")
        );
        assert_eq!(
            call_target_key(&module, "main").as_deref(),
            Some("helper::sort")
        );
        assert_eq!(
            call_target_key(&crate_module, "main").as_deref(),
            Some("helper::sort")
        );
        assert_eq!(call_target_key(&ufcs_method, "main"), None);
        assert_eq!(call_target_key(&call, "main"), None);
    }

    #[test]
    fn path_ident_helpers_preserve_exact_and_stripped_semantics() {
        let ident_expr: syn::Expr = parse_quote! { value };
        let deref_expr: syn::Expr = parse_quote! { *((value)) };
        let self_expr: syn::Expr = parse_quote! { self };
        let field_expr: syn::Expr = parse_quote! { self.value };
        let qualified_expr: syn::Expr = parse_quote! { crate::value };
        let other_expr: syn::Expr = parse_quote! { other };
        let value_ident = syn::Ident::new("value", proc_macro2::Span::call_site());
        let slice_range: syn::Expr = parse_quote! { values[1..] };
        let slice_index: syn::Expr = parse_quote! { values[1] };
        let vec_ty: syn::Type = parse_quote! { Vec<u8> };

        assert_eq!(expr_path_ident(&ident_expr).as_deref(), Some("value"));
        assert_eq!(expr_path_ident(&deref_expr), None);
        assert_eq!(path_ident_name(&deref_expr).as_deref(), Some("value"));
        assert!(is_self_expr(&self_expr));
        assert!(!is_self_expr(&field_expr));
        assert!(expr_is_ident(&ident_expr, &value_ident));
        assert!(!expr_is_ident(&qualified_expr, &value_ident));
        assert!(!expr_is_ident(&other_expr, &value_ident));
        assert!(!expr_is_ident(&field_expr, &value_ident));
        assert!(is_slice_range_index_expr(&slice_range));
        assert!(!is_slice_range_index_expr(&slice_index));
        assert!(!is_path_ident(&qualified_expr, "value"));
        assert_eq!(type_path_ident_name(&vec_ty).as_deref(), Some("Vec"));
    }

    #[test]
    fn self_shape_helpers_distinguish_exact_and_ref_self() {
        let direct: syn::Expr = parse_quote! { self };
        let referenced: syn::Expr = parse_quote! { &self };
        let parenthesized_ref: syn::Expr = parse_quote! { ((&self)) };
        let field: syn::Expr = parse_quote! { self.0 };
        let other_ident: syn::Expr = parse_quote! { value };

        assert!(is_self_expr(&direct));
        assert!(!is_self_expr(&referenced));
        assert!(is_self_or_ref_self_expr(&referenced));
        assert!(is_self_or_ref_self_expr(&parenthesized_ref));
        assert!(!is_self_or_ref_self_expr(&field));
        assert!(!is_self_or_ref_self_expr(&other_ident));
    }

    #[test]
    fn strip_paren_or_group_keeps_non_grouped_expr() {
        let grouped: syn::Expr = parse_quote! { (((value))) };
        let stripped = strip_paren_or_group(&grouped);
        assert_eq!(expr_path_ident(stripped).as_deref(), Some("value"));

        let field: syn::Expr = parse_quote! { value.field };
        assert!(matches!(strip_paren_or_group(&field), syn::Expr::Field(_)));
    }

    #[test]
    fn clone_wrapper_helpers_preserve_wrapper_semantics() {
        fn expr_tokens(expr: syn::Expr) -> String {
            expr.to_token_stream().to_string()
        }

        let clone_call: syn::Expr = parse_quote! { value.clone() };
        let parened_clone: syn::Expr = parse_quote! { (value.clone()) };
        let grouped_clone = syn::Expr::Group(syn::ExprGroup {
            attrs: Vec::new(),
            group_token: Default::default(),
            expr: Box::new(parse_quote! { value.clone() }),
        });
        let clone_with_arg: syn::Expr = parse_quote! { value.clone(extra) };

        assert_eq!(
            direct_clone_call_receiver_expr(&clone_call).map(expr_tokens),
            Some("value".to_string())
        );
        assert!(direct_clone_call_receiver_expr(&parened_clone).is_none());
        assert_eq!(
            clone_call_receiver_expr(&parened_clone).map(expr_tokens),
            Some("value".to_string())
        );
        assert!(clone_call_receiver_expr(&grouped_clone).is_none());
        assert_eq!(
            stripped_clone_call_receiver_expr(&grouped_clone).map(expr_tokens),
            Some("value".to_string())
        );
        assert!(is_clone_call_expr(&grouped_clone));
        assert!(is_clone_call_expr(&parened_clone));
        assert!(stripped_clone_call_receiver_expr(&clone_with_arg).is_none());
        assert!(!is_clone_call_expr(&clone_with_arg));
    }

    #[test]
    fn zero_arg_method_call_helpers_read_receiver_only_for_exact_zero_arg_calls() {
        let plain: syn::Expr = parse_quote! { value.clone() };
        let with_arg: syn::Expr = parse_quote! { value.clone(extra) };
        let other: syn::Expr = parse_quote! { value.to_vec() };
        let path: syn::Expr = parse_quote! { clone(value) };

        assert_eq!(
            zero_arg_method_call_receiver_expr(&plain, "clone")
                .map(|expr| expr.to_token_stream().to_string())
                .as_deref(),
            Some("value")
        );
        assert!(zero_arg_method_call_receiver_expr(&with_arg, "clone").is_none());
        assert!(zero_arg_method_call_receiver_expr(&other, "clone").is_none());
        assert!(zero_arg_method_call_receiver_expr(&path, "clone").is_none());
    }

    #[test]
    fn method_identifier_helpers_capture_generated_receiver_wrappers() {
        let clone = syn::Ident::new("clone", proc_macro2::Span::mixed_site());
        let lock = syn::Ident::new("lock", proc_macro2::Span::mixed_site());
        let unwrap = syn::Ident::new("unwrap", proc_macro2::Span::mixed_site());
        let as_ref = syn::Ident::new("as_ref", proc_macro2::Span::mixed_site());
        let type_ = syn::Ident::new("Type", proc_macro2::Span::mixed_site());

        assert!(is_receiver_type_wrapper_method(&clone));
        assert!(is_receiver_type_wrapper_method(&lock));
        assert!(is_receiver_type_wrapper_method(&unwrap));
        assert!(!is_receiver_type_wrapper_method(&as_ref));
        assert!(!is_lock_guard_wrapper_method(&clone));
        assert!(is_lock_guard_wrapper_method(&lock));
        assert!(is_lock_guard_wrapper_method(&unwrap));
        assert!(ident_matches_any(&type_, &["IsValid", "Type"]));
    }

    #[test]
    fn expr_path_ident_or_clone_reads_path_through_clone_wrappers() {
        let plain: syn::Expr = parse_quote! { value };
        let cloned: syn::Expr = parse_quote! { ((value.clone())) };
        let grouped_clone = syn::Expr::Group(syn::ExprGroup {
            attrs: Vec::new(),
            group_token: Default::default(),
            expr: Box::new(parse_quote! { value.clone() }),
        });
        let field_clone: syn::Expr = parse_quote! { value.field.clone() };
        let clone_with_arg: syn::Expr = parse_quote! { value.clone(extra) };

        assert_eq!(expr_path_ident_or_clone(&plain).as_deref(), Some("value"));
        assert_eq!(expr_path_ident_or_clone(&cloned).as_deref(), Some("value"));
        assert_eq!(
            expr_path_ident_or_clone(&grouped_clone).as_deref(),
            Some("value")
        );
        assert_eq!(expr_path_ident_or_clone(&field_clone), None);
        assert_eq!(expr_path_ident_or_clone(&clone_with_arg), None);
    }

    #[test]
    fn receiver_root_ident_name_follows_generated_receiver_wrappers() {
        let receiver: syn::Expr = parse_quote! { (&((*value).field.clone().lock().unwrap())) };
        let call_with_args: syn::Expr = parse_quote! { value.wrap(extra) };
        let call_no_root: syn::Expr = parse_quote! { make_value().field };

        assert_eq!(
            receiver_root_ident_name(&receiver).as_deref(),
            Some("value")
        );
        assert_eq!(receiver_root_ident_name(&call_with_args), None);
        assert_eq!(receiver_root_ident_name(&call_no_root), None);
    }

    #[test]
    fn mut_borrowed_path_name_reads_only_simple_mut_path_borrows() {
        let borrow: syn::Expr = parse_quote! { &mut value };
        let parened_borrow: syn::Expr = parse_quote! { &mut ((value)) };
        let field_borrow: syn::Expr = parse_quote! { &mut value.field };
        let shared_borrow: syn::Expr = parse_quote! { &value };

        assert_eq!(mut_borrowed_path_name(&borrow).as_deref(), Some("value"));
        assert_eq!(
            mut_borrowed_path_name(&parened_borrow).as_deref(),
            Some("value")
        );
        assert_eq!(mut_borrowed_path_name(&field_borrow), None);
        assert_eq!(mut_borrowed_path_name(&shared_borrow), None);
    }

    #[test]
    fn expression_contains_helpers_match_local_paths_and_methods() {
        let expr: syn::Expr = parse_quote! { value + other.wrap(crate::value) };
        let names = ["missing".to_string(), "other".to_string()]
            .into_iter()
            .collect::<std::collections::HashSet<_>>();

        assert!(expr_contains_path_ident(&expr, "value"));
        assert!(expr_contains_any_path_ident(&expr, &names));
        assert!(!expr_contains_path_ident(&expr, "crate"));
        assert!(expr_contains_method_call(&expr, "wrap"));
        assert!(!expr_contains_method_call(&expr, "missing"));
    }

    #[test]
    fn receiver_scoped_temp_detection_uses_lock_unwrap_ast_shape() {
        let direct: syn::Expr = parse_quote! { p.lock().unwrap() };
        let nested: syn::Expr = parse_quote! { (p.lock().unwrap()).field };
        let unrelated_name: syn::Expr = parse_quote! { p.locked().unwrap() };
        let different_unwrap: syn::Expr = parse_quote! { p.lock().unwrap_or_else(recover) };

        assert!(receiver_expr_needs_scoped_temp(&direct));
        assert!(receiver_expr_needs_scoped_temp(&nested));
        assert!(!receiver_expr_needs_scoped_temp(&unrelated_name));
        assert!(!receiver_expr_needs_scoped_temp(&different_unwrap));
    }

    #[test]
    fn syn_type_matches_compares_paths_references_and_tuples_structurally() {
        let thing: syn::Type = parse_quote! { crate::pkg::Thing };
        let same_thing: syn::Type = parse_quote! { crate::pkg::Thing };
        let other_thing: syn::Type = parse_quote! { crate::pkg::Other };
        let shared_ref: syn::Type = parse_quote! { &crate::pkg::Thing };
        let mut_ref: syn::Type = parse_quote! { &mut crate::pkg::Thing };
        let tuple: syn::Type = parse_quote! { (crate::pkg::Thing, usize) };
        let same_tuple: syn::Type = parse_quote! { (crate::pkg::Thing, usize) };
        let short_tuple: syn::Type = parse_quote! { (crate::pkg::Thing,) };

        assert!(syn_type_matches(&thing, &same_thing));
        assert!(!syn_type_matches(&thing, &other_thing));
        assert!(!syn_type_matches(&shared_ref, &mut_ref));
        assert!(syn_type_matches(&tuple, &same_tuple));
        assert!(!syn_type_matches(&tuple, &short_tuple));
    }

    #[test]
    fn dedupe_syn_types_matches_parameterized_types_structurally() {
        let mut types: Vec<syn::Type> = vec![
            parse_quote! { crate::pkg::Thing },
            parse_quote! { crate::pkg::Thing },
            parse_quote! { crate::builtin::GorsPtr<crate::pkg::Thing> },
            parse_quote! { crate::builtin::GorsPtr<crate::pkg::Thing> },
            parse_quote! { crate::pkg::Other },
        ];

        dedupe_syn_types(&mut types);

        assert_eq!(types.len(), 3);
        let [thing, pointer, other] = types.as_slice() else {
            return;
        };
        assert!(syn_type_matches(thing, &parse_quote! { crate::pkg::Thing }));
        assert!(syn_type_matches(
            pointer,
            &parse_quote! { crate::builtin::GorsPtr<crate::pkg::Thing> }
        ));
        assert!(syn_type_matches(other, &parse_quote! { crate::pkg::Other }));
    }

    #[test]
    fn syn_expr_matches_target_uses_lvalue_ast_shape() {
        let field_target: syn::Expr = parse_quote! { self.xs };
        let field_read: syn::Expr = parse_quote! { (self.xs) };
        let other_field: syn::Expr = parse_quote! { self.ys };
        let pointer_target: syn::Expr = parse_quote! { *p.lock().unwrap() };
        let pointer_read: syn::Expr = parse_quote! { *(p.lock().unwrap()) };
        let index_target: syn::Expr = parse_quote! { values[(i) as usize] };
        let index_read: syn::Expr = parse_quote! { (values[((i) as usize)]) };

        assert!(syn_expr_matches_target(&field_read, &field_target));
        assert!(!syn_expr_matches_target(&other_field, &field_target));
        assert!(syn_expr_matches_target(&pointer_read, &pointer_target));
        assert!(syn_expr_matches_target(&index_read, &index_target));
    }

    #[test]
    fn impl_trait_targets_match_compares_trait_and_self_targets() {
        let target: syn::Item = parse_quote! { impl Reader for Source {} };
        let same_target: syn::Item = parse_quote! { impl Reader for Source { fn read(&self) {} } };
        let other_trait: syn::Item = parse_quote! { impl Writer for Source {} };
        let other_self: syn::Item = parse_quote! { impl Reader for Sink {} };
        let inherent_impl: syn::Item = parse_quote! { impl Source {} };
        let negative_impl: syn::Item = parse_quote! { impl !Reader for Source {} };

        assert!(impl_trait_targets_match(&target, &same_target));
        assert!(!impl_trait_targets_match(&target, &other_trait));
        assert!(!impl_trait_targets_match(&target, &other_self));
        assert!(!impl_trait_targets_match(&target, &inherent_impl));
        assert!(!impl_trait_targets_match(&target, &negative_impl));
    }

    #[test]
    fn type_param_bound_matches_compares_trait_bounds_structurally() {
        let bound: syn::TypeParamBound = parse_quote! { for<'a> Fn(&'a str) -> usize };
        let same_bound: syn::TypeParamBound = parse_quote! { for<'a> Fn(&'a str) -> usize };
        let other_lifetime: syn::TypeParamBound = parse_quote! { for<'b> Fn(&'b str) -> usize };
        let other_output: syn::TypeParamBound = parse_quote! { for<'a> Fn(&'a str) -> isize };
        let maybe_bound: syn::TypeParamBound = parse_quote! { ?Sized };
        let same_maybe_bound: syn::TypeParamBound = parse_quote! { ?Sized };
        let plain_bound: syn::TypeParamBound = parse_quote! { Sized };

        assert!(type_param_bound_matches(&bound, &same_bound));
        assert!(!type_param_bound_matches(&bound, &other_lifetime));
        assert!(!type_param_bound_matches(&bound, &other_output));
        assert!(type_param_bound_matches(&maybe_bound, &same_maybe_bound));
        assert!(!type_param_bound_matches(&maybe_bound, &plain_bound));
    }

    #[test]
    fn type_param_bound_matches_compares_lifetime_bounds() {
        let lifetime: syn::TypeParamBound = parse_quote! { 'a };
        let same_lifetime: syn::TypeParamBound = parse_quote! { 'a };
        let other_lifetime: syn::TypeParamBound = parse_quote! { 'b };
        let trait_bound: syn::TypeParamBound = parse_quote! { Clone };

        assert!(type_param_bound_matches(&lifetime, &same_lifetime));
        assert!(!type_param_bound_matches(&lifetime, &other_lifetime));
        assert!(!type_param_bound_matches(&lifetime, &trait_bound));
    }

    #[test]
    fn generic_param_matches_compares_type_and_const_params_structurally() {
        let type_param: syn::GenericParam = parse_quote! { T: Clone + Default };
        let same_type_param: syn::GenericParam = parse_quote! { T: Clone + Default };
        let reordered_type_param: syn::GenericParam = parse_quote! { T: Default + Clone };
        let const_param: syn::GenericParam = parse_quote! { const N: usize };
        let same_const_param: syn::GenericParam = parse_quote! { const N: usize };
        let other_const_ty: syn::GenericParam = parse_quote! { const N: isize };

        assert!(generic_param_matches(&type_param, &same_type_param));
        assert!(!generic_param_matches(&type_param, &reordered_type_param));
        assert!(generic_param_matches(&const_param, &same_const_param));
        assert!(!generic_param_matches(&const_param, &other_const_ty));
    }

    #[test]
    fn call_shape_helpers_read_box_and_arc_mutex_wrappers() {
        let boxed: syn::Expr = parse_quote! { Box::new(value) };
        let qualified_boxed: syn::Expr = parse_quote! { crate::Box::new(value) };
        let boxed_unit: syn::Expr = parse_quote! { crate::Box::new((())) };
        let leaked: syn::Expr = parse_quote! { Box::leak(Box::new(value)) };
        let qualified_leak: syn::Expr = parse_quote! { crate::Box::leak(Box::new(value)) };
        let arc_mutex: syn::Expr =
            parse_quote! { std::sync::Arc::new(std::sync::Mutex::new(value)) };
        let arc_other: syn::Expr = parse_quote! { std::sync::Arc::new(value) };

        assert!(is_box_new_call(&boxed));
        assert!(!is_box_new_call(&qualified_boxed));
        assert!(is_box_new_unit_expr(&boxed_unit));
        assert!(!is_box_new_unit_expr(&boxed));
        assert!(is_box_leak_expr(&leaked));
        assert!(!is_box_leak_expr(&qualified_leak));
        assert_eq!(
            arc_mutex_new_inner_expr(&arc_mutex)
                .map(|expr| expr.to_token_stream().to_string())
                .as_deref(),
            Some("value")
        );
        assert!(arc_mutex_new_inner_expr(&arc_other).is_none());
    }

    #[test]
    fn boxed_any_expression_helpers_detect_zero_and_existing_casts() {
        let empty_any: syn::Expr = parse_quote! {
            Box::new(()) as Box<dyn std::any::Any>
        };
        let empty_any_send_sync: syn::Expr = parse_quote! {
            (Box::new(()) as Box<dyn std::any::Any + Send + Sync>)
        };
        let boxed_value: syn::Expr = parse_quote! {
            Box::new(value) as Box<dyn std::any::Any>
        };
        let not_cast: syn::Expr = parse_quote! { Box::new(()) };

        assert!(is_box_dyn_any_expr(&empty_any));
        assert!(is_box_dyn_any_expr(&empty_any_send_sync));
        assert!(!is_box_dyn_any_expr(&boxed_value));
        assert!(box_dyn_any_cast_source_expr(&empty_any).is_some());
        assert!(box_dyn_any_cast_source_expr(&boxed_value).is_some());
        assert_eq!(
            box_dyn_any_cast_source_expr(&boxed_value)
                .map(|expr| expr.to_token_stream().to_string())
                .as_deref(),
            Some("Box :: new (value)")
        );
        assert!(box_dyn_any_cast_source_expr(&not_cast).is_none());
    }

    #[test]
    fn pat_ident_name_reads_ident_and_typed_patterns() {
        let ident: syn::Pat = parse_quote! { value };
        assert_eq!(pat_ident_name(&ident).as_deref(), Some("value"));

        let typed: syn::Pat = syn::Pat::Type(parse_quote! { value: String });
        assert_eq!(pat_ident_name(&typed).as_deref(), Some("value"));

        let tuple: syn::Pat = parse_quote! { (left, right) };
        assert_eq!(pat_ident_name(&tuple), None);
    }

    #[test]
    fn pat_ident_names_reads_nested_bindings() {
        let pat: syn::Pat = parse_quote! { (first, &second, Some(third)) };
        assert_eq!(pat_ident_names(&pat), ["first", "second", "third"]);
    }

    #[test]
    fn fn_arg_ident_reads_typed_identifier_args_only() {
        let value_arg: syn::FnArg = parse_quote! { value: isize };
        let receiver_arg: syn::FnArg = parse_quote! { &self };
        let tuple_arg: syn::FnArg = parse_quote! { (left, right): (isize, isize) };

        assert_eq!(
            fn_arg_ident(&value_arg)
                .map(|ident| ident.to_string())
                .as_deref(),
            Some("value")
        );
        assert!(fn_arg_ident(&receiver_arg).is_none());
        assert!(fn_arg_ident(&tuple_arg).is_none());
    }

    #[test]
    fn type_shape_helpers_read_vec_slice_and_nested_type_args() {
        let vec_ty: syn::Type = parse_quote! { Vec<u8> };
        assert_eq!(
            vec_type_inner(&vec_ty)
                .map(|ty| ty.to_token_stream().to_string())
                .as_deref(),
            Some("u8")
        );

        let slice_ty: syn::Type = parse_quote! { [String] };
        assert_eq!(
            slice_type_inner(&slice_ty)
                .map(|ty| ty.to_token_stream().to_string())
                .as_deref(),
            Some("String")
        );

        let lazy_ty: syn::Type = parse_quote! { std::sync::LazyLock<Arc<Mutex<i32>>> };
        assert_eq!(
            first_type_arg_if_path_last_ident(&lazy_ty, "LazyLock")
                .map(|ty| ty.to_token_stream().to_string())
                .as_deref(),
            Some("Arc < Mutex < i32 > >")
        );

        let wrapper_path: syn::TypePath = parse_quote! { std::sync::Arc<std::sync::Mutex<Item>> };
        assert_eq!(
            first_type_arg_for_path_last_ident_any(
                &wrapper_path.path,
                &["Arc", "Mutex", "GorsPtr"],
            )
            .map(|ty| ty.to_token_stream().to_string())
            .as_deref(),
            Some("std :: sync :: Mutex < Item >")
        );
    }

    #[test]
    fn any_type_helpers_detect_boxed_any_trait_objects() {
        let std_any: syn::Type = parse_quote! { Box<dyn std::any::Any> };
        let any_with_bounds: syn::Type = parse_quote! { Box<dyn Any + Send + Sync> };
        let other_trait: syn::Type = parse_quote! { Box<dyn std::fmt::Display> };
        let qualified_box: syn::Type = parse_quote! { crate::Box<dyn Any> };

        assert!(is_box_dyn_any_type(&std_any));
        assert!(is_box_dyn_any_type(&any_with_bounds));
        assert!(!is_box_dyn_any_type(&other_trait));
        assert!(is_box_type_with_any_bound(&std_any));
        assert!(is_box_type_with_any_bound(&any_with_bounds));
        assert!(!is_box_type_with_any_bound(&other_trait));
        assert!(!is_box_type_with_any_bound(&qualified_box));
    }
}
