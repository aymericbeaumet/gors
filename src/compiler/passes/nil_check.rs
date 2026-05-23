use syn::visit_mut::{self, VisitMut};

pub fn pass(file: &mut syn::File) {
    NilCheck.visit_file_mut(file);
}

struct NilCheck;

impl VisitMut for NilCheck {
    fn visit_expr_mut(&mut self, expr: &mut syn::Expr) {
        visit_mut::visit_expr_mut(self, expr);

        if let syn::Expr::Binary(binary) = expr {
            if is_none_expr(&binary.right) {
                let left = (*binary.left).clone();
                match binary.op {
                    syn::BinOp::Ne(_) => {
                        if is_unsafe_pointer_call(&left) {
                            *expr = syn::parse_quote! { #left != 0 };
                        } else {
                            *expr = syn::parse_quote! { !#left.is_empty() };
                        }
                    }
                    syn::BinOp::Eq(_) => {
                        if is_unsafe_pointer_call(&left) {
                            *expr = syn::parse_quote! { #left == 0 };
                        } else {
                            *expr = syn::parse_quote! { #left.is_empty() };
                        }
                    }
                    _ => {}
                }
            } else if is_none_expr(&binary.left) {
                let right = (*binary.right).clone();
                match binary.op {
                    syn::BinOp::Ne(_) => {
                        if is_unsafe_pointer_call(&right) {
                            *expr = syn::parse_quote! { #right != 0 };
                        } else {
                            *expr = syn::parse_quote! { !#right.is_empty() };
                        }
                    }
                    syn::BinOp::Eq(_) => {
                        if is_unsafe_pointer_call(&right) {
                            *expr = syn::parse_quote! { #right == 0 };
                        } else {
                            *expr = syn::parse_quote! { #right.is_empty() };
                        }
                    }
                    _ => {}
                }
            }
        }

        // Convert None to Default::default() in tuple expressions and assignments
        if let syn::Expr::Tuple(tuple) = expr {
            for elem in tuple.elems.iter_mut() {
                if is_none_expr(elem) {
                    *elem = syn::parse_quote! { Default::default() };
                }
            }
        }
    }

    fn visit_expr_assign_mut(&mut self, assign: &mut syn::ExprAssign) {
        visit_mut::visit_expr_assign_mut(self, assign);
        if is_none_expr(&assign.right) {
            *assign.right = syn::parse_quote! { Default::default() };
        }
    }

    fn visit_local_mut(&mut self, local: &mut syn::Local) {
        visit_mut::visit_local_mut(self, local);
        if let Some(ref mut init) = local.init {
            if is_none_expr(&init.expr) {
                *init.expr = syn::parse_quote! { Default::default() };
            }
        }
    }
}

fn is_unsafe_pointer_call(expr: &syn::Expr) -> bool {
    matches!(expr, syn::Expr::MethodCall(mc) if mc.method == "UnsafePointer")
}

fn is_none_expr(expr: &syn::Expr) -> bool {
    if let syn::Expr::Path(path) = expr {
        if path.path.segments.len() == 1
            && path
                .path
                .segments
                .first()
                .is_some_and(|seg| seg.ident == "None")
        {
            return true;
        }
    }
    false
}
