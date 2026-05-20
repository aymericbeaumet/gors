use syn::visit_mut::{self, VisitMut};

pub fn pass(file: &mut syn::File) {
    StringLit.visit_file_mut(file);
}

struct StringLit;

impl VisitMut for StringLit {
    fn visit_expr_mut(&mut self, expr: &mut syn::Expr) {
        visit_mut::visit_expr_mut(self, expr); // depth-first
    }

    fn visit_expr_return_mut(&mut self, ret: &mut syn::ExprReturn) {
        visit_mut::visit_expr_return_mut(self, ret);

        if let Some(ref mut expr) = ret.expr {
            wrap_string_lit_boxed(expr);
        }
    }

    fn visit_block_mut(&mut self, block: &mut syn::Block) {
        visit_mut::visit_block_mut(self, block);

        // Wrap the tail expression if it's a string literal
        if let Some(syn::Stmt::Expr(expr, None)) = block.stmts.last_mut() {
            wrap_string_lit(expr);
        }
    }

    fn visit_field_value_mut(&mut self, fv: &mut syn::FieldValue) {
        visit_mut::visit_field_value_mut(self, fv);
        wrap_string_lit(&mut fv.expr);
    }

    fn visit_local_mut(&mut self, local: &mut syn::Local) {
        visit_mut::visit_local_mut(self, local);
        if let Some(ref mut init) = local.init {
            wrap_string_lit_boxed(&mut init.expr);
        }
    }

    fn visit_expr_assign_mut(&mut self, assign: &mut syn::ExprAssign) {
        visit_mut::visit_expr_assign_mut(self, assign);
        wrap_string_lit_boxed(&mut assign.right);
    }
}

fn is_string_lit(expr: &syn::Expr) -> bool {
    matches!(
        expr,
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(_),
            ..
        })
    )
}

fn wrap_string_lit(expr: &mut syn::Expr) {
    if is_string_lit(expr) {
        let inner = expr.clone();
        *expr = syn::parse_quote! { #inner.to_string() };
    }
}

fn wrap_string_lit_boxed(expr: &mut Box<syn::Expr>) {
    if is_string_lit(expr) {
        let inner = (**expr).clone();
        **expr = syn::parse_quote! { #inner.to_string() };
    }
}
