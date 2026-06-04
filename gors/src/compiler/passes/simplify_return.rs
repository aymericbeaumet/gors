use syn::visit_mut::VisitMut;

pub fn pass(file: &mut syn::File) {
    SimplifyReturn.visit_file_mut(file);
}

struct SimplifyReturn;

impl SimplifyReturn {
    fn simplify_last_return(&self, stmts: &mut [syn::Stmt]) {
        if let Some(last) = stmts.last_mut() {
            let expr = match last {
                syn::Stmt::Expr(expr, _) => expr,
                _ => return,
            };

            if let syn::Expr::Return(ret) = expr {
                if let Some(expr) = ret.expr.take() {
                    *last = syn::Stmt::Expr(*expr, None);
                }
            }
        }
    }
}

impl VisitMut for SimplifyReturn {
    fn visit_item_fn_mut(&mut self, item_fn: &mut syn::ItemFn) {
        self.simplify_last_return(&mut item_fn.block.stmts);
    }

    fn visit_impl_item_fn_mut(&mut self, item_fn: &mut syn::ImplItemFn) {
        self.simplify_last_return(&mut item_fn.block.stmts);
    }
}
