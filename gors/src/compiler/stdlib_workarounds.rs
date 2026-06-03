use crate::ast;
use proc_macro2::Span;

pub(super) enum StdlibCallWorkaround {
    SortSlice,
}

impl StdlibCallWorkaround {
    pub(super) fn classify(call_expr: &ast::CallExpr<'_>) -> Option<Self> {
        if is_sort_slice_call(call_expr) {
            return Some(Self::SortSlice);
        }
        None
    }

    pub(super) fn compile(self, call_expr: ast::CallExpr<'_>) -> syn::Expr {
        match self {
            Self::SortSlice => compile_sort_slice_call(call_expr)
                .unwrap_or_else(|| super::compile_error_expr("invalid sort slice call")),
        }
    }
}

fn compile_sort_slice_call(call_expr: ast::CallExpr) -> Option<syn::Expr> {
    let ast::Expr::SelectorExpr(selector) = *call_expr.fun else {
        return None;
    };
    if !matches!(*selector.x, ast::Expr::Ident(pkg) if pkg.name == "sort") {
        return None;
    }
    if !matches!(selector.sel.name, "Slice" | "SliceStable" | "SliceIsSorted") {
        return None;
    }

    let mut args = call_expr.args.unwrap_or_default().into_iter();
    let ast::Expr::Ident(slice_ident) = args.next()? else {
        return None;
    };
    let less_arg = args.next()?;
    if args.next().is_some() {
        return None;
    }

    let slice_ident = syn::Ident::new(
        &super::rust_safe_ident_name(slice_ident.name),
        Span::mixed_site(),
    );
    let less: syn::Expr = less_arg.into();
    match selector.sel.name {
        "Slice" | "SliceStable" => Some(syn::parse_quote! {{
            let mut __gors_less = #less;
            let __gors_len = #slice_ident.len();
            for __gors_i in 0..__gors_len {
                for __gors_j in (__gors_i + 1)..__gors_len {
                    if __gors_less(__gors_j as isize, __gors_i as isize) {
                        #slice_ident.swap(__gors_i, __gors_j);
                    }
                }
            }
        }}),
        "SliceIsSorted" => Some(syn::parse_quote! {{
            let mut __gors_less = #less;
            let mut __gors_sorted = true;
            let __gors_len = #slice_ident.len();
            let mut __gors_i = 1usize;
            while __gors_i < __gors_len {
                if __gors_less(__gors_i as isize, (__gors_i - 1) as isize) {
                    __gors_sorted = false;
                    break;
                }
                __gors_i += 1;
            }
            __gors_sorted
        }}),
        _ => None,
    }
}

fn is_sort_slice_call(call_expr: &ast::CallExpr) -> bool {
    let ast::Expr::SelectorExpr(selector) = &*call_expr.fun else {
        return false;
    };
    matches!(&*selector.x, ast::Expr::Ident(pkg) if pkg.name == "sort")
        && matches!(selector.sel.name, "Slice" | "SliceStable" | "SliceIsSorted")
}
