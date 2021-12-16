use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::punctuated::Punctuated;
use syn::token::{Colon2, Comma};
use syn::visit_mut::{self, VisitMut};
use syn::{Expr, PathSegment, Token};

pub struct InlineFmt;

impl VisitMut for InlineFmt {
    fn visit_expr_mut(&mut self, expr: &mut syn::Expr) {
        if let syn::Expr::Call(call) = expr {
            if let syn::Expr::Path(path) = call.func.as_mut() {
                let sgmts = &path.path.segments;
                if sgmts.len() == 2 && sgmts[0].ident == "fmt" && sgmts[1].ident == "Println" {
                    *expr = syn::Expr::Macro(syn::ExprMacro {
                        attrs: vec![],
                        mac: syn::Macro {
                            path: syn::Path {
                                leading_colon: Some(<Token![::]>::default()),
                                segments: segments(),
                            },
                            bang_token: <Token![!]>::default(),
                            tokens: tokens(&call.args),
                            delimiter: syn::MacroDelimiter::Paren(syn::token::Paren {
                                span: Span::mixed_site(),
                            }),
                        },
                    });
                }
            }
        }

        visit_mut::visit_expr_mut(self, expr);
    }
}

fn segments() -> Punctuated<PathSegment, Colon2> {
    let mut segments = syn::punctuated::Punctuated::new();
    segments.push(syn::PathSegment {
        ident: syn::Ident::new("std", Span::mixed_site()),
        arguments: syn::PathArguments::None,
    });
    segments.push(syn::PathSegment {
        ident: syn::Ident::new("println", Span::mixed_site()),
        arguments: syn::PathArguments::None,
    });
    segments
}

fn tokens(call_args: &Punctuated<Expr, Comma>) -> TokenStream {
    if call_args.len() == 1 {
        if let syn::Expr::Lit(expr_lit) = &call_args[0] {
            if let syn::Lit::Str(lit_str) = &expr_lit.lit {
                return quote! { #lit_str };
            }
        }
    }

    let mut fmt_str = String::new();
    let mut fmt_args = quote! {};
    for arg in call_args.iter() {
        fmt_str.push_str(if fmt_str.is_empty() { "{}" } else { " {}" });
        fmt_args.extend(quote! { , #arg })
    }
    quote! { #fmt_str #fmt_args }
}
