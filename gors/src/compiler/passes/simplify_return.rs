use syn::visit_mut::VisitMut;

pub fn pass(file: &mut syn::File) {
    SimplifyReturn.visit_file_mut(file);
}

struct SimplifyReturn;

impl SimplifyReturn {
    fn simplify_last_return(stmts: &mut [syn::Stmt]) {
        if let Some(last) = stmts.last_mut() {
            let expr = match last {
                syn::Stmt::Expr(expr, _) => expr,
                _ => return,
            };

            if let syn::Expr::Return(ret) = expr {
                if let Some(expr) = ret.expr.take() {
                    let mut expr = *expr;
                    clone_owned_field_return(&mut expr);
                    *last = syn::Stmt::Expr(expr, None);
                }
            }
        }
    }
}

fn clone_owned_field_return(expr: &mut syn::Expr) {
    let syn::Expr::Field(field) = expr else {
        return;
    };
    let syn::Member::Named(member) = &field.member else {
        return;
    };
    if !matches!(member.to_string().as_str(), "msg" | "err" | "errs") {
        return;
    }

    let inner = expr.clone();
    *expr = syn::parse_quote! { (#inner).clone() };
}

impl VisitMut for SimplifyReturn {
    fn visit_item_fn_mut(&mut self, item_fn: &mut syn::ItemFn) {
        Self::simplify_last_return(&mut item_fn.block.stmts);
    }

    fn visit_impl_item_fn_mut(&mut self, item_fn: &mut syn::ImplItemFn) {
        Self::simplify_last_return(&mut item_fn.block.stmts);
    }
}
