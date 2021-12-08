use proc_macro2::Span;
use quote::quote;
use syn::visit_mut::{self, VisitMut};
use syn::Token;

pub struct InlineFmt;

impl VisitMut for InlineFmt {
    fn visit_expr_mut(&mut self, expr: &mut syn::Expr) {
        if let syn::Expr::Call(call) = expr {
            if let syn::Expr::Path(path) = call.func.as_mut() {
                let sgmts = &path.path.segments;
                if sgmts.len() == 2 && sgmts[0].ident == "fmt" && sgmts[1].ident == "Println" {
                    // Prepare the segments
                    let mut segments = syn::punctuated::Punctuated::new();
                    segments.push(syn::PathSegment {
                        ident: syn::Ident::new("std", Span::mixed_site()),
                        arguments: syn::PathArguments::None,
                    });
                    segments.push(syn::PathSegment {
                        ident: syn::Ident::new("println", Span::mixed_site()),
                        arguments: syn::PathArguments::None,
                    });
                    // Prepare the arguments
                    let mut fmt_str = String::new();
                    let mut fmt_args = quote! {};
                    for arg in call.args.iter() {
                        fmt_str.push_str(if fmt_str.is_empty() { "{}" } else { " {}" });
                        fmt_args.extend(quote! { , #arg })
                    }
                    // Prepare the macro
                    let mac = syn::Macro {
                        path: syn::Path {
                            leading_colon: Some(<Token![::]>::default()),
                            segments,
                        },
                        bang_token: <Token![!]>::default(),
                        tokens: quote! { #fmt_str #fmt_args },
                        delimiter: syn::MacroDelimiter::Paren(syn::token::Paren {
                            span: Span::mixed_site(),
                        }),
                    };
                    // Replace the expr call by the macro call
                    *expr = syn::Expr::Macro(syn::ExprMacro { attrs: vec![], mac });
                }
            }
        }

        visit_mut::visit_expr_mut(self, expr);
    }
}
