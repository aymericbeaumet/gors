use syn::visit_mut::{self, VisitMut};

pub fn pass(file: &mut syn::File) {
    IndexCast.visit_file_mut(file);
}

struct IndexCast;

impl VisitMut for IndexCast {
    fn visit_expr_mut(&mut self, expr: &mut syn::Expr) {
        visit_mut::visit_expr_mut(self, expr);

        if let syn::Expr::Index(index) = expr {
            let idx = &*index.index;
            if !is_already_usize(idx) && !is_range(idx) {
                let inner = index.index.clone();
                index.index = Box::new(syn::parse_quote! { (#inner) as usize });
            }
        }
    }
}

fn is_range(expr: &syn::Expr) -> bool {
    matches!(expr, syn::Expr::Range(_))
}

fn is_already_usize(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Cast(cast) => {
            if let syn::Type::Path(tp) = &*cast.ty {
                tp.path.is_ident("usize")
            } else {
                false
            }
        }
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Int(_),
            ..
        }) => true,
        // Paren expression wrapping a cast to isize (from coerce_types len() wrapping)
        syn::Expr::Paren(paren) => matches!(&*paren.expr, syn::Expr::Cast(_)),
        _ => false,
    }
}
