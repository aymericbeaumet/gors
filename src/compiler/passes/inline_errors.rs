use syn::visit_mut::{self, VisitMut};

pub fn pass(file: &mut syn::File) {
    InlineErrors.visit_file_mut(file);
}

struct InlineErrors;

impl VisitMut for InlineErrors {
    fn visit_expr_mut(&mut self, expr: &mut syn::Expr) {
        visit_mut::visit_expr_mut(self, expr); // depth-first

        if let syn::Expr::Call(call) = expr {
            if let syn::Expr::Path(path) = call.func.as_ref() {
                let sgmts = &path.path.segments;
                // errors.New("msg") -> "msg".to_string()
                let is_errors_new = sgmts.len() == 2
                    && sgmts.first().is_some_and(|seg| seg.ident == "errors")
                    && sgmts.iter().nth(1).is_some_and(|seg| seg.ident == "New");
                if is_errors_new {
                    if let Some(first_arg) = call.args.first() {
                        let arg = first_arg.clone();
                        *expr = syn::parse_quote! { #arg.to_string() };
                    }
                }
            }
        }
    }
}
