use crate::{ast, token};

pub(super) struct Compare {
    pub(super) side: CompareSide,
    pub(super) kind: String,
}

pub(super) enum CompareSide {
    Left,
    Right,
}

pub(super) fn detect_compare(
    binary_expr: &ast::BinaryExpr,
    is_reflect_qualifier: impl Fn(&str) -> bool + Copy,
) -> Option<Compare> {
    if !matches!(binary_expr.op, token::Token::EQL | token::Token::NEQ) {
        return None;
    }

    if typeof_kind_arg_ref(&binary_expr.x, is_reflect_qualifier) {
        let kind = kind_const_ref(&binary_expr.y, is_reflect_qualifier)?;
        kind_variant_expr(kind)?;
        return Some(Compare {
            side: CompareSide::Left,
            kind: kind.to_string(),
        });
    }

    if typeof_kind_arg_ref(&binary_expr.y, is_reflect_qualifier) {
        let kind = kind_const_ref(&binary_expr.x, is_reflect_qualifier)?;
        kind_variant_expr(kind)?;
        return Some(Compare {
            side: CompareSide::Right,
            kind: kind.to_string(),
        });
    }

    None
}

fn typeof_kind_arg_ref(
    expr: &ast::Expr,
    is_reflect_qualifier: impl Fn(&str) -> bool + Copy,
) -> bool {
    let ast::Expr::CallExpr(kind_call) = expr else {
        return false;
    };
    if kind_call.args.as_ref().is_some_and(|args| !args.is_empty()) {
        return false;
    }
    let ast::Expr::SelectorExpr(kind_selector) = &*kind_call.fun else {
        return false;
    };
    if kind_selector.sel.name != "Kind" {
        return false;
    }
    let ast::Expr::CallExpr(type_of_call) = &*kind_selector.x else {
        return false;
    };
    let ast::Expr::SelectorExpr(type_of_selector) = &*type_of_call.fun else {
        return false;
    };
    matches!(&*type_of_selector.x, ast::Expr::Ident(pkg) if is_reflect_qualifier(pkg.name))
        && type_of_selector.sel.name == "TypeOf"
        && matches!(type_of_call.args.as_deref(), Some([_]))
}

pub(super) fn typeof_kind_arg(expr: ast::Expr) -> Option<ast::Expr> {
    let ast::Expr::CallExpr(kind_call) = expr else {
        return None;
    };
    let ast::Expr::SelectorExpr(kind_selector) = *kind_call.fun else {
        return None;
    };
    let ast::Expr::CallExpr(type_of_call) = *kind_selector.x else {
        return None;
    };
    let mut args = type_of_call.args?;
    if args.len() == 1 {
        Some(args.remove(0))
    } else {
        None
    }
}

fn kind_const_ref<'ast>(
    expr: &ast::Expr<'ast>,
    is_reflect_qualifier: impl Fn(&str) -> bool + Copy,
) -> Option<&'ast str> {
    let ast::Expr::SelectorExpr(selector) = expr else {
        return None;
    };
    if !matches!(&*selector.x, ast::Expr::Ident(pkg) if is_reflect_qualifier(pkg.name)) {
        return None;
    }
    Some(selector.sel.name)
}

pub(super) fn kind_variant_expr(name: &str) -> Option<syn::Expr> {
    match name {
        "Invalid" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Invalid }),
        "Bool" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Bool }),
        "Int" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Int }),
        "Int8" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Int8 }),
        "Int16" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Int16 }),
        "Int32" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Int32 }),
        "Int64" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Int64 }),
        "Uint" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Uint }),
        "Uint8" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Uint8 }),
        "Uint16" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Uint16 }),
        "Uint32" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Uint32 }),
        "Uint64" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Uint64 }),
        "Uintptr" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Uintptr }),
        "Float32" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Float32 }),
        "Float64" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Float64 }),
        "Complex64" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Complex64 }),
        "Complex128" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Complex128 }),
        "Array" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Array }),
        "Chan" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Chan }),
        "Func" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Func }),
        "Interface" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Interface }),
        "Map" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Map }),
        "Pointer" | "Ptr" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Pointer }),
        "Slice" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Slice }),
        "String" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::String }),
        "Struct" => Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::Struct }),
        "UnsafePointer" => {
            Some(syn::parse_quote! { crate::builtin::__GorsReflectKind::UnsafePointer })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first_return_binary_expr(source: &str) -> ast::BinaryExpr<'_> {
        let file = crate::parser::parse_file("fixture.go", source).expect("parse");
        let func = file
            .decls
            .into_iter()
            .find_map(|decl| match decl {
                ast::Decl::FuncDecl(func) => Some(func),
                ast::Decl::GenDecl(_) => None,
            })
            .expect("function");
        let stmt = func
            .body
            .expect("body")
            .list
            .into_iter()
            .find_map(|stmt| match stmt {
                ast::Stmt::ReturnStmt(stmt) => Some(stmt),
                _ => None,
            })
            .expect("return");
        let expr = stmt.results.into_iter().next().expect("return expression");
        let ast::Expr::BinaryExpr(binary) = expr else {
            panic!("expected binary expression");
        };
        binary
    }

    #[test]
    fn detects_typeof_kind_compare_on_left() {
        let binary = first_return_binary_expr(
            r#"
                package main

                import "reflect"

                func isString(v any) bool {
                    return reflect.TypeOf(v).Kind() == reflect.String
                }
            "#,
        );

        let compare =
            detect_compare(&binary, |name| name == "reflect").expect("reflect kind compare");
        assert!(matches!(compare.side, CompareSide::Left));
        assert_eq!(compare.kind, "String");
    }

    #[test]
    fn detects_aliased_typeof_kind_compare() {
        let binary = first_return_binary_expr(
            r#"
                package main

                import r "reflect"

                func isString(v any) bool {
                    return r.TypeOf(v).Kind() != r.String
                }
            "#,
        );

        let compare = detect_compare(&binary, |name| name == "r").expect("reflect kind compare");
        assert!(matches!(compare.side, CompareSide::Left));
        assert_eq!(compare.kind, "String");
    }

    #[test]
    fn maps_pointer_and_legacy_ptr_kind_names() {
        assert!(kind_variant_expr("Pointer").is_some());
        assert!(kind_variant_expr("Ptr").is_some());
        assert!(kind_variant_expr("NotAKind").is_none());
    }
}
