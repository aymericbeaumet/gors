use syn::visit_mut::{self, VisitMut};

pub fn pass(file: &mut syn::File) {
    SimplifyReturn.visit_file_mut(file);
}

struct SimplifyReturn;

impl VisitMut for SimplifyReturn {
    fn visit_block_mut(&mut self, block: &mut syn::Block) {
        if let Some(last) = block.stmts.last_mut() {
            if let syn::Stmt::Expr(syn::Expr::Return(ret)) = last {
                if let Some(expr) = ret.expr.clone() {
                    *last = syn::Stmt::Expr(*expr);
                }
            }
        }

        visit_mut::visit_block_mut(self, block);
    }
}
