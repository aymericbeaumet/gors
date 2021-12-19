use syn::visit_mut::VisitMut;

pub fn pass(file: &mut syn::File) {
    SimplifyReturn.visit_file_mut(file);
}

struct SimplifyReturn;

impl VisitMut for SimplifyReturn {
    fn visit_item_fn_mut(&mut self, item_fn: &mut syn::ItemFn) {
        if let Some(last) = item_fn.block.stmts.last_mut() {
            let expr = match last {
                syn::Stmt::Semi(expr, _) => expr,
                syn::Stmt::Expr(expr) => expr,
                _ => return,
            };

            if let syn::Expr::Return(ret) = expr {
                if let Some(expr) = ret.expr.take() {
                    *last = syn::Stmt::Expr(*expr);
                }
            }
        }
    }
}
