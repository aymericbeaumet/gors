use super::ast;

pub(super) fn selector_is_unsafe_pointer(selector: &ast::SelectorExpr<'_>) -> bool {
    matches!(&*selector.x, ast::Expr::Ident(pkg) if pkg.name == "unsafe")
        && selector.sel.name == "Pointer"
}

pub(super) fn expr_is_unsafe_pointer_selector(expr: &ast::Expr<'_>) -> bool {
    matches!(expr, ast::Expr::SelectorExpr(selector) if selector_is_unsafe_pointer(selector))
}

pub(super) fn call_is_unsafe_pointer_conversion(call: &ast::CallExpr<'_>) -> bool {
    expr_is_unsafe_pointer_selector(&call.fun)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::Position;

    fn ident(name: &'static str) -> ast::Ident<'static> {
        ast::Ident {
            name_pos: Position::default(),
            name,
            obj: None,
        }
    }

    fn selector(pkg: &'static str, name: &'static str) -> ast::SelectorExpr<'static> {
        ast::SelectorExpr {
            x: Box::new(ast::Expr::Ident(ident(pkg))),
            sel: ident(name),
        }
    }

    #[test]
    fn unsafe_pointer_selector_matches_exact_package_member() {
        assert!(selector_is_unsafe_pointer(&selector("unsafe", "Pointer")));
        assert!(!selector_is_unsafe_pointer(&selector("unsafe", "Sizeof")));
        assert!(!selector_is_unsafe_pointer(&selector("safe", "Pointer")));
    }

    #[test]
    fn unsafe_pointer_call_matches_selector_function() {
        let call = ast::CallExpr {
            fun: Box::new(ast::Expr::SelectorExpr(selector("unsafe", "Pointer"))),
            lparen: Position::default(),
            args: Some(vec![ast::Expr::Ident(ident("value"))]),
            ellipsis: None,
            rparen: Position::default(),
        };
        let other = ast::CallExpr {
            fun: Box::new(ast::Expr::Ident(ident("Pointer"))),
            lparen: Position::default(),
            args: Some(vec![ast::Expr::Ident(ident("value"))]),
            ellipsis: None,
            rparen: Position::default(),
        };

        assert!(call_is_unsafe_pointer_conversion(&call));
        assert!(!call_is_unsafe_pointer_conversion(&other));
    }
}
