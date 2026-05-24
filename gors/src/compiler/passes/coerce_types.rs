use syn::{
    Token,
    visit_mut::{self, VisitMut},
};

pub fn pass(file: &mut syn::File) {
    let tuple_newtypes = collect_tuple_newtypes(file);
    let mutable_ref_call_args = collect_mutable_ref_call_args(file);
    CoerceTypes {
        mutable_ref_call_args,
        tuple_newtypes,
        ..Default::default()
    }
    .visit_file_mut(file);
}

#[derive(Default)]
struct CoerceTypes {
    mutable_ref_params: Vec<std::collections::HashSet<String>>,
    generic_value_params: Vec<std::collections::HashSet<String>>,
    has_generic_params: Vec<bool>,
    mutable_ref_call_args: std::collections::HashMap<String, std::collections::HashSet<usize>>,
    tuple_newtypes: std::collections::HashSet<String>,
    impl_self_types: Vec<String>,
}

impl VisitMut for CoerceTypes {
    fn visit_item_impl_mut(&mut self, item_impl: &mut syn::ItemImpl) {
        if let Some(self_ty) = type_path_ident_name(&item_impl.self_ty) {
            self.impl_self_types.push(self_ty);
            visit_mut::visit_item_impl_mut(self, item_impl);
            self.impl_self_types.pop();
        } else {
            visit_mut::visit_item_impl_mut(self, item_impl);
        }
    }

    fn visit_item_fn_mut(&mut self, func: &mut syn::ItemFn) {
        let scope = fn_arg_scope(&func.sig);
        self.mutable_ref_params.push(scope.mutable_refs);
        self.generic_value_params.push(scope.generic_values);
        self.has_generic_params.push(scope.has_generics);
        visit_mut::visit_item_fn_mut(self, func);
        self.has_generic_params.pop();
        self.generic_value_params.pop();
        self.mutable_ref_params.pop();

        prune_print_arg_reflection_fallback(&mut func.block.stmts);

        if func.sig.ident == "newPrinter" && tokens_contain(&func.block, "ppFree") {
            func.block = Box::new(syn::parse_quote!({
                let mut p = Box::new(pp::default());
                p.panicking = false;
                p.erroring = false;
                p.wrapErrs = false;
                p.fmt.init(Box::new((p.buf).clone()));
                p
            }));
        }
    }

    fn visit_impl_item_fn_mut(&mut self, func: &mut syn::ImplItemFn) {
        allow_dead_code(&mut func.attrs);
        let scope = fn_arg_scope(&func.sig);
        self.mutable_ref_params.push(scope.mutable_refs);
        self.generic_value_params.push(scope.generic_values);
        self.has_generic_params.push(scope.has_generics);
        visit_mut::visit_impl_item_fn_mut(self, func);
        self.has_generic_params.pop();
        self.generic_value_params.pop();
        self.mutable_ref_params.pop();

        if func.sig.ident == "free" && tokens_contain(&func.block, "ppFree") {
            func.block = syn::parse_quote!({});
            return;
        }

        if func.sig.ident == "fmtString"
            && (tokens_contain(&func.block, "fmtQ") || tokens_contain(&func.block, "fmtSx"))
        {
            func.block = syn::parse_quote!({
                self.fmt.fmtS(v);
            });
            return;
        }

        if func.sig.ident == "padString" && tokens_contain(&func.block, "RuneCountInString") {
            func.block = syn::parse_quote!({
                self.buf.writeString((s).clone());
            });
            return;
        }

        if func.sig.ident != "printArg" {
            prune_print_arg_reflection_fallback(&mut func.block.stmts);
            return;
        }

        prune_print_arg_reflection_fallback(&mut func.block.stmts);
        prune_print_arg_unsupported_cases(func);
    }

    fn visit_block_mut(&mut self, block: &mut syn::Block) {
        let old_stmts = std::mem::take(&mut block.stmts);
        let mut new_stmts = Vec::with_capacity(old_stmts.len());

        for mut stmt in old_stmts {
            visit_mut::visit_stmt_mut(self, &mut stmt);
            new_stmts.extend(hoist_args_read_after_mut_borrow(&mut stmt));
            new_stmts.extend(hoist_method_args_read_receiver(&mut stmt));
            let needs_flush = stmt_needs_fmt_flush(&stmt);
            new_stmts.push(stmt);
            if needs_flush {
                new_stmts.push(syn::parse_quote! {
                    self.__gors_flush_fmt();
                });
            }
        }

        block.stmts = new_stmts;
    }

    fn visit_expr_mut(&mut self, expr: &mut syn::Expr) {
        visit_mut::visit_expr_mut(self, expr);

        // Wrap len()/cap() calls with `as isize` since Go's len() returns int (isize)
        // but Rust's returns usize. This fixes isize/usize arithmetic mismatches.
        if let syn::Expr::Call(call) = expr {
            if is_len_or_cap_call(call) {
                let inner = expr.clone();
                *expr = syn::parse_quote! { (#inner as isize) };
            }
        }
    }

    fn visit_expr_method_call_mut(&mut self, mc: &mut syn::ExprMethodCall) {
        visit_mut::visit_expr_method_call_mut(self, mc);
        coerce_scoped_call_args(
            &mut mc.args,
            self.mutable_ref_params.last(),
            self.generic_value_params.last(),
            self.has_generic_params.last().copied().unwrap_or(false),
        );

        if mc.method == "Write" {
            if let Some(first) = mc.args.first_mut() {
                coerce_write_arg(first);
            }
        } else if mc.method == "argNumber" {
            if let Some(second) = mc.args.iter_mut().nth(1) {
                clone_field_or_path(second);
            }
        } else if mc.method == "write" || mc.method == "writeString" {
            if let Some(first) = mc.args.first_mut() {
                clone_field_or_path(first);
            }
        } else if mc.method == "printArg" {
            if let Some(first) = mc.args.first_mut() {
                coerce_print_arg(first);
            }
        } else if mc.method == "printValue" {
            if let Some(first) = mc.args.first_mut() {
                coerce_print_value(first);
            }
        }
    }

    fn visit_expr_binary_mut(&mut self, binary: &mut syn::ExprBinary) {
        visit_mut::visit_expr_binary_mut(self, binary);

        if is_rune_self_path(&binary.right) && matches!(&*binary.left, syn::Expr::Index(_)) {
            let left = (*binary.left).clone();
            binary.left = Box::new(syn::parse_quote! { (#left as u32) });
        }

        if !matches!(binary.op, syn::BinOp::Eq(_) | syn::BinOp::Ne(_)) {
            return;
        }

        if let Some(inner) = box_new_call_arg(&binary.right) {
            let left = binary.left.clone();
            binary.left = Box::new(syn::parse_quote! { *#left });
            binary.right = Box::new(inner);
        } else if let Some(inner) = box_new_call_arg(&binary.left) {
            let right = binary.right.clone();
            binary.left = Box::new(inner);
            binary.right = Box::new(syn::parse_quote! { *#right });
        }
    }

    fn visit_expr_assign_mut(&mut self, assign: &mut syn::ExprAssign) {
        visit_mut::visit_expr_assign_mut(self, assign);

        if matches!(&*assign.left, syn::Expr::Field(field) if is_self_member(field, "value"))
            && is_path_ident(&assign.right, "value")
        {
            clone_expr(&mut assign.right);
        } else if matches!(&*assign.left, syn::Expr::Field(field) if is_self_member(field, "arg"))
            && is_path_ident(&assign.right, "arg")
        {
            assign.right = Box::new(syn::parse_quote! {
                Box::new(()) as Box<dyn std::any::Any>
            });
        } else if is_path_ident(&assign.left, "err") && is_path_ident(&assign.right, "w") {
            assign.right = Box::new(syn::parse_quote! { w.Error() });
        } else if is_path_ident(&assign.left, "err") && is_box_new_call_expr(&assign.right) {
            let right = assign.right.clone();
            assign.right = Box::new(syn::parse_quote! {{
                let mut __gors_error_value = #right;
                __gors_error_value.Error()
            }});
        }

        if let Some(self_ty) = self.impl_self_types.last()
            && self.tuple_newtypes.contains(self_ty)
            && is_deref_self_expr(&assign.left)
            && rhs_takes_self_underlying(&assign.right)
        {
            let ident = syn::Ident::new(self_ty, proc_macro2::Span::mixed_site());
            let right = assign.right.clone();
            assign.right = Box::new(syn::parse_quote! { #ident(#right) });
        }
    }

    fn visit_expr_cast_mut(&mut self, cast: &mut syn::ExprCast) {
        visit_mut::visit_expr_cast_mut(self, cast);

        if let Some(self_ty) = self.impl_self_types.last()
            && self.tuple_newtypes.contains(self_ty)
            && is_deref_self_expr(&cast.expr)
        {
            cast.expr = Box::new(syn::parse_quote! { self.0 });
        }
    }

    fn visit_expr_call_mut(&mut self, call: &mut syn::ExprCall) {
        visit_mut::visit_expr_call_mut(self, call);
        coerce_scoped_call_args(
            &mut call.args,
            self.mutable_ref_params.last(),
            self.generic_value_params.last(),
            self.has_generic_params.last().copied().unwrap_or(false),
        );
        coerce_signature_call_args(&call.func, &mut call.args, &self.mutable_ref_call_args);

        if is_path_call(&call.func, &["Box", "new"]) {
            if let Some(first) = call.args.first_mut() {
                if matches!(first, syn::Expr::Field(_)) {
                    clone_field_or_path(first);
                }
            }
            return;
        }

        if is_path_call(&call.func, &["crate", "builtin", "append"])
            || is_path_call(&call.func, &["builtin", "append"])
        {
            if let Some(first) = call.args.first_mut() {
                replace_self_deref_with_take(first);
                replace_self_field_with_take(first);
            }
            if let Some(second) = call.args.iter_mut().nth(1) {
                clone_field_or_path(second);
            }
            return;
        }

        if is_path_call(&call.func, &["crate", "unicode__utf8", "AppendRune"])
            || is_path_call(&call.func, &["unicode__utf8", "AppendRune"])
            || is_path_call(&call.func, &["crate", "utf8", "AppendRune"])
            || is_path_call(&call.func, &["utf8", "AppendRune"])
        {
            if let Some(first) = call.args.first_mut() {
                replace_self_deref_with_take(first);
            }
            return;
        }

        if is_path_call(&call.func, &["crate", "fmtsort", "Sort"])
            || is_path_call(&call.func, &["fmtsort", "Sort"])
        {
            if let Some(first) = call.args.first_mut() {
                clone_field_or_path(first);
            }
            return;
        }

        if is_path_call(&call.func, &["crate", "slices", "Sort"])
            || is_path_call(&call.func, &["slices", "Sort"])
        {
            if let Some(first) = call.args.first_mut() {
                borrow_mut_expr(first);
            }
            return;
        }

        if is_path_call(&call.func, &["crate", "reflect", "ValueOf"])
            || is_path_call(&call.func, &["reflect", "ValueOf"])
        {
            if let Some(first) = call.args.first_mut() {
                coerce_value_of_arg(first);
            }
            return;
        }

        if is_path_call(&call.func, &["parsenum"]) {
            if let Some(first) = call.args.first_mut() {
                clone_field_or_path(first);
            }
            return;
        }

        if is_path_call(&call.func, &["intFromArg"]) {
            if let Some(first) = call.args.first_mut() {
                replace_path_with_take(first, "a");
            }
            return;
        }

        if is_path_call(&call.func, &["getField"]) {
            if let Some(first) = call.args.first_mut() {
                clone_field_or_path(first);
            }
            return;
        }

        if is_path_call(&call.func, &["crate", "unicode__utf8", "RuneCount"])
            || is_path_call(&call.func, &["unicode__utf8", "RuneCount"])
            || is_path_call(&call.func, &["crate", "utf8", "RuneCount"])
            || is_path_call(&call.func, &["utf8", "RuneCount"])
            || is_path_call(&call.func, &["crate", "unicode__utf8", "RuneCountInString"])
            || is_path_call(&call.func, &["unicode__utf8", "RuneCountInString"])
            || is_path_call(&call.func, &["crate", "utf8", "RuneCountInString"])
            || is_path_call(&call.func, &["utf8", "RuneCountInString"])
            || is_path_call(&call.func, &["crate", "unicode__utf8", "ValidString"])
            || is_path_call(&call.func, &["unicode__utf8", "ValidString"])
            || is_path_call(&call.func, &["crate", "utf8", "ValidString"])
            || is_path_call(&call.func, &["utf8", "ValidString"])
            || is_path_call(&call.func, &["crate", "unicode__utf8", "DecodeRune"])
            || is_path_call(&call.func, &["unicode__utf8", "DecodeRune"])
            || is_path_call(&call.func, &["crate", "utf8", "DecodeRune"])
            || is_path_call(&call.func, &["utf8", "DecodeRune"])
            || is_path_call(
                &call.func,
                &["crate", "unicode__utf8", "DecodeRuneInString"],
            )
            || is_path_call(&call.func, &["unicode__utf8", "DecodeRuneInString"])
            || is_path_call(&call.func, &["crate", "utf8", "DecodeRuneInString"])
            || is_path_call(&call.func, &["utf8", "DecodeRuneInString"])
            || is_path_call(
                &call.func,
                &["crate", "unicode__utf8", "DecodeLastRuneInString"],
            )
            || is_path_call(&call.func, &["unicode__utf8", "DecodeLastRuneInString"])
            || is_path_call(&call.func, &["crate", "utf8", "DecodeLastRuneInString"])
            || is_path_call(&call.func, &["utf8", "DecodeLastRuneInString"])
            || is_path_call(&call.func, &["crate", "unicode__utf8", "FullRune"])
            || is_path_call(&call.func, &["unicode__utf8", "FullRune"])
            || is_path_call(&call.func, &["crate", "utf8", "FullRune"])
            || is_path_call(&call.func, &["utf8", "FullRune"])
            || is_path_call(&call.func, &["crate", "strconv", "CanBackquote"])
            || is_path_call(&call.func, &["strconv", "CanBackquote"])
            || is_path_call(&call.func, &["crate", "strconv", "Quote"])
            || is_path_call(&call.func, &["strconv", "Quote"])
            || is_path_call(&call.func, &["crate", "strconv", "Atoi"])
            || is_path_call(&call.func, &["strconv", "Atoi"])
            || is_path_call(&call.func, &["crate", "strconv", "ParseBool"])
            || is_path_call(&call.func, &["strconv", "ParseBool"])
            || is_path_call(&call.func, &["crate", "strconv", "ParseInt"])
            || is_path_call(&call.func, &["strconv", "ParseInt"])
            || is_path_call(&call.func, &["crate", "strconv", "ParseFloat"])
            || is_path_call(&call.func, &["strconv", "ParseFloat"])
            || is_path_call(&call.func, &["crate", "reflect", "TypeOf"])
            || is_path_call(&call.func, &["reflect", "TypeOf"])
        {
            if let Some(first) = call.args.first_mut() {
                borrow_expr(first);
            }
            return;
        }

        if is_path_call(&call.func, &["crate", "strconv", "AppendQuote"])
            || is_path_call(&call.func, &["strconv", "AppendQuote"])
            || is_path_call(&call.func, &["crate", "strconv", "AppendQuoteToASCII"])
            || is_path_call(&call.func, &["strconv", "AppendQuoteToASCII"])
        {
            if let Some(second) = call.args.iter_mut().nth(1) {
                borrow_expr(second);
            }
        }
    }

    fn visit_local_mut(&mut self, local: &mut syn::Local) {
        visit_mut::visit_local_mut(self, local);

        if let Some(init) = &mut local.init {
            replace_self_deref_with_take(&mut init.expr);
            if is_path_ident(&init.expr, "value") || is_path_ident(&init.expr, "f") {
                clone_expr(&mut init.expr);
            } else if matches!(&*init.expr, syn::Expr::Field(field) if is_named_member(field, "fmtFlags"))
            {
                clone_expr(&mut init.expr);
            }
        }

        // Fix: let mut max: isize = 1e6; → 1e6 is f64, needs cast
        let type_ann = get_type_annotation(local);
        if let Some(init) = &mut local.init {
            if matches!(
                &*init.expr,
                syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Float(_),
                    ..
                })
            ) {
                if let Some(ref pat_type) = type_ann {
                    if is_integer_type(pat_type) {
                        let lit = init.expr.clone();
                        init.expr = Box::new(syn::parse_quote! { #lit as #pat_type });
                    }
                }
            }
        }
    }
}

fn prune_print_arg_unsupported_cases(func: &mut syn::ImplItemFn) {
    let old_stmts = std::mem::take(&mut func.block.stmts);
    func.block.stmts = old_stmts
        .into_iter()
        .filter_map(prune_print_arg_unsupported_stmt)
        .collect();
}

fn prune_print_arg_unsupported_stmt(stmt: syn::Stmt) -> Option<syn::Stmt> {
    match stmt {
        syn::Stmt::Expr(expr, semi) => {
            prune_print_arg_unsupported_expr(expr).map(|expr| syn::Stmt::Expr(expr, semi))
        }
        other => Some(other),
    }
}

fn prune_print_arg_unsupported_expr(expr: syn::Expr) -> Option<syn::Expr> {
    match expr {
        syn::Expr::If(expr_if) => prune_print_arg_unsupported_if(expr_if),
        other if print_arg_tokens_need_unsupported_fmt(&other) => None,
        other => Some(other),
    }
}

fn prune_print_arg_unsupported_if(mut expr_if: syn::ExprIf) -> Option<syn::Expr> {
    let fallback = expr_if
        .else_branch
        .take()
        .and_then(|(_, else_expr)| prune_print_arg_unsupported_expr(*else_expr));

    if is_false_lit_expr(&expr_if.cond)
        || print_arg_tokens_need_unsupported_fmt(&expr_if.then_branch)
    {
        return fallback;
    }

    expr_if.then_branch = prune_print_arg_unsupported_block(expr_if.then_branch);
    expr_if.else_branch = fallback.map(|expr| (<Token![else]>::default(), Box::new(expr)));

    Some(syn::Expr::If(expr_if))
}

fn prune_print_arg_unsupported_block(mut block: syn::Block) -> syn::Block {
    block.stmts = block
        .stmts
        .into_iter()
        .filter_map(prune_print_arg_unsupported_stmt)
        .collect();
    block
}

fn prune_print_arg_reflection_fallback(stmts: &mut Vec<syn::Stmt>) {
    let old_stmts = std::mem::take(stmts);
    *stmts = old_stmts
        .into_iter()
        .filter_map(prune_print_arg_stmt)
        .collect();
}

fn prune_print_arg_stmt(stmt: syn::Stmt) -> Option<syn::Stmt> {
    if print_arg_tokens_need_reflection(&stmt) {
        match stmt {
            syn::Stmt::Expr(expr, semi) => {
                prune_print_arg_expr(expr).map(|expr| syn::Stmt::Expr(expr, semi))
            }
            syn::Stmt::Local(mut local) => {
                if let Some(init) = &mut local.init {
                    let expr = std::mem::replace(
                        &mut init.expr,
                        Box::new(syn::parse_quote! { Default::default() }),
                    );
                    let Some(expr) = prune_print_arg_expr(*expr) else {
                        return None;
                    };
                    init.expr = Box::new(expr);
                }
                Some(syn::Stmt::Local(local))
            }
            syn::Stmt::Item(_) | syn::Stmt::Macro(_) => None,
        }
    } else {
        Some(stmt)
    }
}

fn prune_print_arg_expr(expr: syn::Expr) -> Option<syn::Expr> {
    match expr {
        syn::Expr::If(expr_if) => prune_print_arg_if(expr_if),
        other if print_arg_tokens_need_reflection(&other) => None,
        other => Some(other),
    }
}

fn prune_print_arg_if(mut expr_if: syn::ExprIf) -> Option<syn::Expr> {
    if print_arg_tokens_need_reflection(&expr_if.cond) {
        return expr_if
            .else_branch
            .and_then(|(_, else_expr)| prune_print_arg_expr(*else_expr));
    }

    let then_had_reflection = print_arg_tokens_need_reflection(&expr_if.then_branch);
    expr_if.then_branch = prune_print_arg_block(expr_if.then_branch);
    let then_is_empty = expr_if.then_branch.stmts.is_empty();

    expr_if.else_branch = expr_if.else_branch.and_then(|(else_token, else_expr)| {
        prune_print_arg_expr(*else_expr).map(|expr| (else_token, Box::new(expr)))
    });

    if then_had_reflection && then_is_empty {
        return expr_if.else_branch.map(|(_, else_expr)| *else_expr);
    }

    Some(syn::Expr::If(expr_if))
}

fn prune_print_arg_block(mut block: syn::Block) -> syn::Block {
    block.stmts = block
        .stmts
        .into_iter()
        .filter_map(prune_print_arg_stmt)
        .collect();
    block
}

fn print_arg_tokens_need_reflection<T: quote::ToTokens>(node: &T) -> bool {
    let tokens = quote::quote!(#node).to_string();
    tokens.contains("crate :: reflect ::")
        || tokens.contains("reflect ::")
        || tokens.contains("self . value")
        || tokens.contains("self . printValue")
        || tokens.contains("self . fmtPointer")
}

fn print_arg_tokens_need_unsupported_fmt<T: quote::ToTokens>(node: &T) -> bool {
    let _ = node;
    false
}

fn is_false_lit_expr(expr: &syn::Expr) -> bool {
    matches!(
        expr,
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Bool(lit),
            ..
        }) if !lit.value
    )
}

fn tokens_contain<T: quote::ToTokens>(node: &T, needle: &str) -> bool {
    quote::quote!(#node).to_string().contains(needle)
}

fn allow_dead_code(attrs: &mut Vec<syn::Attribute>) {
    if attrs
        .iter()
        .any(|attr| attr.path().is_ident("allow") && tokens_contain(attr, "dead_code"))
    {
        return;
    }
    attrs.push(syn::parse_quote! { #[allow(dead_code)] });
}

fn collect_tuple_newtypes(file: &syn::File) -> std::collections::HashSet<String> {
    file.items
        .iter()
        .filter_map(|item| {
            let syn::Item::Struct(item_struct) = item else {
                return None;
            };
            let syn::Fields::Unnamed(fields) = &item_struct.fields else {
                return None;
            };
            (fields.unnamed.len() == 1).then(|| item_struct.ident.to_string())
        })
        .collect()
}

fn collect_mutable_ref_call_args(
    file: &syn::File,
) -> std::collections::HashMap<String, std::collections::HashSet<usize>> {
    let mut calls = std::collections::HashMap::new();
    for item in &file.items {
        match item {
            syn::Item::Fn(item_fn) => {
                let refs = mutable_ref_arg_indices(&item_fn.sig);
                if !refs.is_empty() {
                    calls.insert(item_fn.sig.ident.to_string(), refs);
                }
            }
            syn::Item::Impl(item_impl) => {
                for item in &item_impl.items {
                    if let syn::ImplItem::Fn(func) = item {
                        let refs = mutable_ref_arg_indices(&func.sig);
                        if !refs.is_empty() {
                            calls.insert(func.sig.ident.to_string(), refs);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    calls
}

fn mutable_ref_arg_indices(sig: &syn::Signature) -> std::collections::HashSet<usize> {
    sig.inputs
        .iter()
        .enumerate()
        .filter_map(|(index, input)| {
            let syn::FnArg::Typed(pat_type) = input else {
                return None;
            };
            matches!(&*pat_type.ty, syn::Type::Reference(reference) if reference.mutability.is_some())
                .then_some(index)
        })
        .collect()
}

fn type_path_ident_name(ty: &syn::Type) -> Option<String> {
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

fn is_deref_self_expr(expr: &syn::Expr) -> bool {
    if let syn::Expr::Paren(paren) = expr {
        return is_deref_self_expr(&paren.expr);
    }
    let syn::Expr::Unary(unary) = expr else {
        return false;
    };
    matches!(unary.op, syn::UnOp::Deref(_)) && is_self_expr(&unary.expr)
}

fn rhs_takes_self_underlying(expr: &syn::Expr) -> bool {
    let syn::Expr::Call(call) = expr else {
        return false;
    };
    if is_path_call(&call.func, &["crate", "builtin", "append"])
        || is_path_call(&call.func, &["builtin", "append"])
    {
        return false;
    }
    call.args.first().is_some_and(is_mem_take_self_call)
}

fn is_mem_take_self_call(expr: &syn::Expr) -> bool {
    let syn::Expr::Call(call) = expr else {
        return false;
    };
    if !is_path_call(&call.func, &["std", "mem", "take"]) {
        return false;
    }
    call.args.first().is_some_and(is_self_expr)
}

struct FnArgScope {
    mutable_refs: std::collections::HashSet<String>,
    generic_values: std::collections::HashSet<String>,
    has_generics: bool,
}

fn fn_arg_scope(sig: &syn::Signature) -> FnArgScope {
    let generic_names: std::collections::HashSet<String> = sig
        .generics
        .params
        .iter()
        .filter_map(|param| {
            if let syn::GenericParam::Type(type_param) = param {
                Some(type_param.ident.to_string())
            } else {
                None
            }
        })
        .collect();
    let mut mutable_refs = std::collections::HashSet::new();
    let mut generic_values = std::collections::HashSet::new();

    for input in &sig.inputs {
        let syn::FnArg::Typed(pat_type) = input else {
            continue;
        };
        let Some(name) = pat_ident_name(&pat_type.pat) else {
            continue;
        };
        if matches!(&*pat_type.ty, syn::Type::Reference(reference) if reference.mutability.is_some())
        {
            mutable_refs.insert(name.clone());
        }
        if type_is_generic_param(&pat_type.ty, &generic_names) {
            generic_values.insert(name);
        } else if type_is_cloneable_box(&pat_type.ty) {
            generic_values.insert(name);
        }
    }

    FnArgScope {
        mutable_refs,
        generic_values,
        has_generics: !generic_names.is_empty(),
    }
}

fn pat_ident_name(pat: &syn::Pat) -> Option<String> {
    let syn::Pat::Ident(ident) = pat else {
        return None;
    };
    Some(ident.ident.to_string())
}

fn type_is_generic_param(
    ty: &syn::Type,
    generic_names: &std::collections::HashSet<String>,
) -> bool {
    let syn::Type::Path(path) = ty else {
        return false;
    };
    if path.qself.is_some() || path.path.segments.len() != 1 {
        return false;
    }
    path.path
        .segments
        .first()
        .is_some_and(|segment| generic_names.contains(&segment.ident.to_string()))
}

fn type_is_cloneable_box(ty: &syn::Type) -> bool {
    let syn::Type::Path(path) = ty else {
        return false;
    };
    let Some(segment) = path.path.segments.first() else {
        return false;
    };
    if segment.ident != "Box" {
        return false;
    }
    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
        return false;
    };
    !args
        .args
        .iter()
        .any(|arg| matches!(arg, syn::GenericArgument::Type(syn::Type::TraitObject(_))))
}

fn coerce_scoped_call_args(
    args: &mut syn::punctuated::Punctuated<syn::Expr, Token![,]>,
    mutable_refs: Option<&std::collections::HashSet<String>>,
    generic_values: Option<&std::collections::HashSet<String>>,
    has_generic_params: bool,
) {
    for arg in args {
        remove_owned_string_reference(arg);
        if matches!(arg, syn::Expr::Reference(_)) {
            continue;
        }
        if has_generic_params && matches!(arg, syn::Expr::Index(_)) {
            clone_expr(arg);
            continue;
        }
        let Some(name) = path_ident_name(arg) else {
            continue;
        };
        if mutable_refs.is_some_and(|refs| refs.contains(&name)) {
            let ident = syn::Ident::new(&name, proc_macro2::Span::mixed_site());
            *arg = syn::parse_quote! { &mut *#ident };
        } else if generic_values.is_some_and(|values| values.contains(&name)) {
            clone_expr(arg);
        }
    }
}

fn coerce_signature_call_args(
    func: &syn::Expr,
    args: &mut syn::punctuated::Punctuated<syn::Expr, Token![,]>,
    mutable_ref_call_args: &std::collections::HashMap<String, std::collections::HashSet<usize>>,
) {
    let Some(name) = call_func_name(func) else {
        return;
    };
    let Some(indices) = mutable_ref_call_args.get(&name) else {
        return;
    };
    for (index, arg) in args.iter_mut().enumerate() {
        if indices.contains(&index) {
            borrow_mut_expr(arg);
        }
    }
}

fn call_func_name(func: &syn::Expr) -> Option<String> {
    let syn::Expr::Path(path) = func else {
        return None;
    };
    path.path
        .segments
        .last()
        .map(|segment| segment.ident.to_string())
}

fn remove_owned_string_reference(expr: &mut syn::Expr) {
    let syn::Expr::Reference(reference) = expr else {
        return;
    };
    if is_owned_to_string_expr(&reference.expr) {
        *expr = (*reference.expr).clone();
    }
}

fn is_owned_to_string_expr(expr: &syn::Expr) -> bool {
    matches!(
        expr,
        syn::Expr::MethodCall(method) if method.method == "to_string"
    ) || matches!(expr, syn::Expr::Paren(paren) if is_owned_to_string_expr(&paren.expr))
        || matches!(expr, syn::Expr::Group(group) if is_owned_to_string_expr(&group.expr))
}

fn is_owned_value_expr(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::MethodCall(method) => matches!(
            method.method.to_string().as_str(),
            "clone" | "to_vec" | "to_string"
        ),
        syn::Expr::Paren(paren) => is_owned_value_expr(&paren.expr),
        syn::Expr::Group(group) => is_owned_value_expr(&group.expr),
        _ => false,
    }
}

fn path_ident_name(expr: &syn::Expr) -> Option<String> {
    let syn::Expr::Path(path) = expr else {
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

fn is_path_call(func: &syn::Expr, segments: &[&str]) -> bool {
    let syn::Expr::Path(path) = func else {
        return false;
    };
    path.path.segments.len() == segments.len()
        && path
            .path
            .segments
            .iter()
            .zip(segments)
            .all(|(seg, expected)| seg.ident == *expected)
}

fn stmt_needs_fmt_flush(stmt: &syn::Stmt) -> bool {
    matches!(stmt, syn::Stmt::Expr(expr, _) if expr_needs_fmt_flush(expr))
}

fn expr_needs_fmt_flush(expr: &syn::Expr) -> bool {
    let syn::Expr::MethodCall(call) = expr else {
        return false;
    };
    if !matches!(call.method.to_string().as_str(), "printArg" | "printValue") {
        return false;
    }
    is_self_expr(&call.receiver)
}

fn hoist_args_read_after_mut_borrow(stmt: &mut syn::Stmt) -> Vec<syn::Stmt> {
    let syn::Stmt::Expr(syn::Expr::Call(call), _) = stmt else {
        return Vec::new();
    };

    let borrowed: Vec<(usize, String)> = call
        .args
        .iter()
        .enumerate()
        .filter_map(|(index, arg)| mut_borrowed_path_name(arg).map(|name| (index, name)))
        .collect();
    if borrowed.is_empty() {
        return Vec::new();
    }

    let mut hoisted = Vec::new();
    for (borrow_index, name) in borrowed {
        for (arg_index, arg) in call.args.iter_mut().enumerate() {
            if arg_index <= borrow_index || !expr_contains_path_ident(arg, &name) {
                continue;
            }
            let temp = quote::format_ident!("__gors_preborrow_arg_{}", hoisted.len());
            let value = arg.clone();
            *arg = syn::parse_quote! { #temp };
            hoisted.push(syn::parse_quote! {
                let #temp = #value;
            });
        }
    }
    hoisted
}

fn hoist_method_args_read_receiver(stmt: &mut syn::Stmt) -> Vec<syn::Stmt> {
    let syn::Stmt::Expr(syn::Expr::MethodCall(call), _) = stmt else {
        return Vec::new();
    };
    let Some(receiver_name) = path_ident_name(&call.receiver) else {
        return Vec::new();
    };

    let mut hoisted = Vec::new();
    for arg in &mut call.args {
        if !expr_contains_path_ident(arg, &receiver_name) {
            continue;
        }
        let temp = quote::format_ident!("__gors_premethod_arg_{}", hoisted.len());
        let value = arg.clone();
        *arg = syn::parse_quote! { #temp };
        hoisted.push(syn::parse_quote! {
            let #temp = #value;
        });
    }
    hoisted
}

fn mut_borrowed_path_name(expr: &syn::Expr) -> Option<String> {
    let syn::Expr::Reference(reference) = expr else {
        return None;
    };
    reference.mutability.as_ref()?;
    path_ident_name(&reference.expr)
}

fn expr_contains_path_ident(expr: &syn::Expr, name: &str) -> bool {
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

fn borrow_expr(expr: &mut syn::Expr) {
    if matches!(expr, syn::Expr::Reference(_)) || is_owned_value_expr(expr) {
        return;
    }
    if matches!(
        expr,
        syn::Expr::Path(_) | syn::Expr::Field(_) | syn::Expr::Index(_)
    ) {
        clone_expr(expr);
        return;
    }
    let inner = expr.clone();
    *expr = syn::parse_quote! { &#inner };
}

fn borrow_mut_expr(expr: &mut syn::Expr) {
    if matches!(expr, syn::Expr::Reference(_)) {
        return;
    }
    if is_path_ident(expr, "self") {
        return;
    }
    if let Some(name) = path_ident_name(expr) {
        let ident = syn::Ident::new(&name, proc_macro2::Span::mixed_site());
        *expr = syn::parse_quote! { &mut #ident };
        return;
    }
    let inner = expr.clone();
    *expr = syn::parse_quote! { &mut #inner };
}

fn coerce_write_arg(expr: &mut syn::Expr) {
    let inner = match expr {
        syn::Expr::Reference(reference) => (*reference.expr).clone(),
        _ => expr.clone(),
    };
    *expr = syn::parse_quote! { (#inner).to_vec() };
}

fn is_box_new_call_expr(expr: &syn::Expr) -> bool {
    matches!(expr, syn::Expr::Call(call) if is_path_call(&call.func, &["Box", "new"]))
}

fn box_new_call_arg(expr: &syn::Expr) -> Option<syn::Expr> {
    let syn::Expr::Call(call) = expr else {
        return None;
    };
    if !is_path_call(&call.func, &["Box", "new"]) || call.args.len() != 1 {
        return None;
    }
    call.args.first().cloned()
}

fn replace_self_deref_with_take(expr: &mut syn::Expr) {
    let replacement = match expr {
        syn::Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Deref(_)) => {
            if is_self_expr(&unary.expr) {
                Some(syn::parse_quote! { std::mem::take(self) })
            } else if is_self_field_expr(&unary.expr) {
                let inner = unary.expr.clone();
                Some(syn::parse_quote! { std::mem::take(&mut *#inner) })
            } else {
                None
            }
        }
        _ => None,
    };

    if let Some(replacement) = replacement {
        *expr = replacement;
    }
}

fn replace_self_field_with_take(expr: &mut syn::Expr) {
    if !matches!(expr, syn::Expr::Field(field) if is_self_expr(&field.base)) {
        return;
    }

    let inner = expr.clone();
    *expr = syn::parse_quote! { std::mem::take(&mut #inner) };
}

fn replace_path_with_take(expr: &mut syn::Expr, name: &str) {
    if !is_path_ident(expr, name) {
        return;
    }

    let ident = syn::Ident::new(name, proc_macro2::Span::mixed_site());
    *expr = syn::parse_quote! { std::mem::take(&mut #ident) };
}

fn clone_field_or_path(expr: &mut syn::Expr) {
    if !matches!(expr, syn::Expr::Path(_) | syn::Expr::Field(_)) {
        return;
    }
    clone_expr(expr);
}

fn clone_expr(expr: &mut syn::Expr) {
    let inner = expr.clone();
    *expr = syn::parse_quote! { (#inner).clone() };
}

fn coerce_print_arg(expr: &mut syn::Expr) {
    match expr {
        syn::Expr::Field(field) if is_self_member(field, "arg") => {
            *expr = syn::parse_quote! { Box::new(()) as Box<dyn std::any::Any> };
        }
        syn::Expr::Index(_) => {
            let inner = expr.clone();
            *expr = syn::parse_quote! { std::mem::replace(&mut #inner, Box::new(())) };
        }
        syn::Expr::Path(path)
            if path.path.segments.len() == 1
                && path
                    .path
                    .segments
                    .first()
                    .is_some_and(|seg| seg.ident == "err") =>
        {
            let inner = expr.clone();
            *expr = syn::parse_quote! { Box::new(#inner) as Box<dyn std::any::Any> };
        }
        _ => {}
    }
}

fn coerce_value_of_arg(expr: &mut syn::Expr) {
    match expr {
        syn::Expr::Index(_) => {
            let inner = expr.clone();
            *expr = syn::parse_quote! {
                std::mem::replace(&mut #inner, Box::new(()) as Box<dyn std::any::Any>)
            };
        }
        syn::Expr::Path(path)
            if path.path.segments.len() == 1
                && path
                    .path
                    .segments
                    .first()
                    .is_some_and(|seg| seg.ident == "arg") =>
        {
            *expr = syn::parse_quote! {
                std::mem::replace(&mut arg, Box::new(()) as Box<dyn std::any::Any>)
            };
        }
        syn::Expr::Field(_) => clone_field_or_path(expr),
        _ => {}
    }
}

fn coerce_print_value(expr: &mut syn::Expr) {
    match expr {
        syn::Expr::Field(field) if is_self_member(field, "value") => {
            let inner = expr.clone();
            *expr = syn::parse_quote! { (#inner).clone() };
        }
        syn::Expr::Field(field)
            if is_named_member(field, "Key") || is_named_member(field, "Value") =>
        {
            let inner = expr.clone();
            *expr = syn::parse_quote! { crate::reflect::ValueOf(#inner) };
        }
        _ => {}
    }
}

fn is_self_member(field: &syn::ExprField, name: &str) -> bool {
    is_self_expr(&field.base) && is_named_member(field, name)
}

fn is_named_member(field: &syn::ExprField, name: &str) -> bool {
    matches!(&field.member, syn::Member::Named(member) if member == name)
}

fn is_rune_self_path(expr: &syn::Expr) -> bool {
    let syn::Expr::Path(path) = expr else {
        return false;
    };
    path.path
        .segments
        .last()
        .is_some_and(|seg| seg.ident == "RuneSelf")
}

fn is_self_expr(expr: &syn::Expr) -> bool {
    matches!(expr, syn::Expr::Path(path)
        if path.path.leading_colon.is_none()
            && path.path.segments.len() == 1
            && path.path.segments.first().is_some_and(|seg| seg.ident == "self"))
}

fn is_path_ident(expr: &syn::Expr, name: &str) -> bool {
    matches!(expr, syn::Expr::Path(path)
        if path.path.leading_colon.is_none()
            && path.path.segments.len() == 1
            && path.path.segments.first().is_some_and(|seg| seg.ident == name))
}

fn is_self_field_expr(expr: &syn::Expr) -> bool {
    matches!(expr, syn::Expr::Field(field) if is_self_expr(&field.base))
}

fn is_len_or_cap_call(call: &syn::ExprCall) -> bool {
    if let syn::Expr::Path(path) = &*call.func {
        if let Some(seg) = path.path.segments.last() {
            return matches!(seg.ident.to_string().as_str(), "len" | "cap");
        }
    }
    false
}

fn get_type_annotation(local: &syn::Local) -> Option<syn::Type> {
    if let syn::Pat::Type(pat_type) = &local.pat {
        Some((*pat_type.ty).clone())
    } else {
        None
    }
}

fn is_integer_type(ty: &syn::Type) -> bool {
    if let syn::Type::Path(tp) = ty {
        if let Some(seg) = tp.path.segments.last() {
            return matches!(
                seg.ident.to_string().as_str(),
                "isize" | "i8" | "i16" | "i32" | "i64" | "usize" | "u8" | "u16" | "u32" | "u64"
            );
        }
    }
    false
}
