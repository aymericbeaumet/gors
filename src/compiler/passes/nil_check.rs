use syn::visit_mut::{self, VisitMut};

pub fn pass(file: &mut syn::File) {
    NilCheck.visit_file_mut(file);
}

struct NilCheck;

impl VisitMut for NilCheck {
    fn visit_expr_mut(&mut self, expr: &mut syn::Expr) {
        visit_mut::visit_expr_mut(self, expr); // depth-first

        if let syn::Expr::Binary(binary) = expr {
            // x != None -> !x.is_empty()  (for error strings)
            // x == None -> x.is_empty()   (for error strings)
            if is_none_expr(&binary.right) {
                let left = (*binary.left).clone();
                match binary.op {
                    syn::BinOp::Ne(_) => {
                        *expr = syn::parse_quote! { !#left.is_empty() };
                    }
                    syn::BinOp::Eq(_) => {
                        *expr = syn::parse_quote! { #left.is_empty() };
                    }
                    _ => {}
                }
            } else if is_none_expr(&binary.left) {
                let right = (*binary.right).clone();
                match binary.op {
                    syn::BinOp::Ne(_) => {
                        *expr = syn::parse_quote! { !#right.is_empty() };
                    }
                    syn::BinOp::Eq(_) => {
                        *expr = syn::parse_quote! { #right.is_empty() };
                    }
                    _ => {}
                }
            }
        }

        // Convert None to String::new() in tuple expressions
        // (for multi-return with error type)
        if let syn::Expr::Tuple(tuple) = expr {
            for elem in tuple.elems.iter_mut() {
                if is_none_expr(elem) {
                    *elem = syn::parse_quote! { String::new() };
                }
            }
        }
    }
}

fn is_none_expr(expr: &syn::Expr) -> bool {
    if let syn::Expr::Path(path) = expr {
        if path.path.segments.len() == 1 && path.path.segments[0].ident == "None" {
            return true;
        }
    }
    false
}
