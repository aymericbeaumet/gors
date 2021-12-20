use syn::visit_mut::{self, VisitMut};

pub fn pass(file: &mut syn::File) {
    FlattenBlock.visit_file_mut(file);
}

struct FlattenBlock;

impl VisitMut for FlattenBlock {
    fn visit_block_mut(&mut self, block: &mut syn::Block) {
        visit_mut::visit_block_mut(self, block); // depth-first

        for stmt in block.stmts.iter_mut() {
            if let syn::Stmt::Expr(syn::Expr::Block(b)) = stmt {
                let bstmts = &b.block.stmts;
                if bstmts.len() == 1 {
                    if let syn::Stmt::Expr(expr) = &bstmts[0] {
                        *stmt = syn::Stmt::Expr(expr.clone());
                        continue;
                    }
                }
            }
        }
    }
}
