use super::super::syn_inspect::is_zero_arg_method_call;
use syn::visit_mut::{self, VisitMut};

pub fn pass(file: &mut syn::File) {
    StringLit.visit_file_mut(file);
}

struct StringLit;

impl VisitMut for StringLit {
    fn visit_expr_mut(&mut self, expr: &mut syn::Expr) {
        visit_mut::visit_expr_mut(self, expr);
    }

    fn visit_expr_binary_mut(&mut self, binary: &mut syn::ExprBinary) {
        visit_mut::visit_expr_binary_mut(self, binary);
        if matches!(binary.op, syn::BinOp::Add(_)) {
            if is_string_lit(&binary.left) {
                let inner = (*binary.left).clone();
                *binary.left = syn::parse_quote! { #inner.to_string() };
            }

            if is_string_concat_expr(&binary.left)
                && !is_string_lit(&binary.right)
                && !matches!(&*binary.right, syn::Expr::Reference(_))
            {
                let right = (*binary.right).clone();
                *binary.right = syn::parse_quote! { &#right };
            }
        }
    }

    fn visit_expr_return_mut(&mut self, ret: &mut syn::ExprReturn) {
        visit_mut::visit_expr_return_mut(self, ret);
        if let Some(ref mut expr) = ret.expr {
            wrap_string_lit_boxed(expr);
        }
    }

    fn visit_block_mut(&mut self, block: &mut syn::Block) {
        visit_mut::visit_block_mut(self, block);
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

    fn visit_expr_method_call_mut(&mut self, mc: &mut syn::ExprMethodCall) {
        visit_mut::visit_expr_method_call_mut(self, mc);
        for arg in mc.args.iter_mut() {
            wrap_string_lit(arg);
        }
    }

    fn visit_expr_call_mut(&mut self, call: &mut syn::ExprCall) {
        visit_mut::visit_expr_call_mut(self, call);
        // Wrap string literal args in local function calls (1-segment path)
        // Skip cross-module calls that may take &str
        if let syn::Expr::Path(path) = &*call.func {
            if path.path.segments.len() == 1 {
                for arg in call.args.iter_mut() {
                    wrap_string_lit(arg);
                }
            }
        }
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

fn is_string_concat_expr(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Lit(_) => is_string_lit(expr),
        syn::Expr::MethodCall(_) => is_zero_arg_method_call(expr, "to_string"),
        syn::Expr::Binary(binary) if matches!(binary.op, syn::BinOp::Add(_)) => {
            is_string_concat_expr(&binary.left)
        }
        syn::Expr::Paren(paren) => is_string_concat_expr(&paren.expr),
        _ => false,
    }
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
