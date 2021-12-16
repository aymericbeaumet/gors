use syn::visit_mut::{self, VisitMut};

pub struct TypeConversion;

impl VisitMut for TypeConversion {
    fn visit_expr_mut(&mut self, expr: &mut syn::Expr) {
        if let syn::Expr::Call(expr_call) = expr {
            if let syn::Expr::Path(path) = &*expr_call.func {
                let segments = &path.path.segments;
                if segments.len() == 1 && is_type(&segments[0].ident) {
                    *expr = syn::Expr::Cast(syn::ExprCast {
                        attrs: vec![],
                        expr: Box::new(expr_call.args[0].clone()),
                        as_token: <syn::Token![as]>::default(),
                        ty: Box::new(syn::Type::Path(syn::TypePath {
                            path: syn::Path {
                                leading_colon: None,
                                segments: segments.clone(),
                            },
                            qself: None,
                        })),
                    })
                }
            }
        }

        visit_mut::visit_expr_mut(self, expr);
    }
}

fn is_type(ident: &syn::Ident) -> bool {
    let s = ident.to_string();
    matches!(
        s.as_str(),
        "f32"
            | "f64"
            | "isize"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "usize"
            | "u8"
            | "u32"
            | "u16"
            | "u64"
    )
}
