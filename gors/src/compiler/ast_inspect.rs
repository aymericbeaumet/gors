use super::ast;

pub(super) fn func_decl_is_package_init(func_decl: &ast::FuncDecl<'_>) -> bool {
    func_decl.recv.is_none() && func_decl.name.name == "init"
}

pub(super) fn selector_unsafe_member<'src>(
    selector: &ast::SelectorExpr<'src>,
) -> Option<&'src str> {
    matches!(&*selector.x, ast::Expr::Ident(pkg) if pkg.name == "unsafe")
        .then_some(selector.sel.name)
}

pub(super) fn selector_is_unsafe_pointer(selector: &ast::SelectorExpr<'_>) -> bool {
    selector_unsafe_member(selector) == Some("Pointer")
}

pub(super) fn selector_is_unsafe_constant(selector: &ast::SelectorExpr<'_>) -> bool {
    matches!(
        selector_unsafe_member(selector),
        Some("Alignof" | "Offsetof" | "Sizeof")
    )
}

pub(super) fn expr_is_unsafe_pointer_selector(expr: &ast::Expr<'_>) -> bool {
    matches!(expr, ast::Expr::SelectorExpr(selector) if selector_is_unsafe_pointer(selector))
}

pub(super) fn call_is_unsafe_pointer_conversion(call: &ast::CallExpr<'_>) -> bool {
    expr_is_unsafe_pointer_selector(&call.fun)
}

pub(super) fn call_unsafe_member<'src>(call: &ast::CallExpr<'src>) -> Option<&'src str> {
    let ast::Expr::SelectorExpr(selector) = call.fun.as_ref() else {
        return None;
    };
    selector_unsafe_member(selector)
}

pub(super) fn call_unsafe_constant_member<'src>(call: &ast::CallExpr<'src>) -> Option<&'src str> {
    call_unsafe_member(call).filter(|member| matches!(*member, "Alignof" | "Offsetof" | "Sizeof"))
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

    fn field_list() -> ast::FieldList<'static> {
        ast::FieldList {
            opening: None,
            list: Vec::new(),
            closing: None,
        }
    }

    fn func_decl(
        name: &'static str,
        recv: Option<ast::FieldList<'static>>,
    ) -> ast::FuncDecl<'static> {
        ast::FuncDecl {
            doc: None,
            recv,
            name: ident(name),
            type_: ast::FuncType {
                func: None,
                type_params: None,
                params: field_list(),
                results: None,
            },
            body: None,
        }
    }

    #[test]
    fn package_init_matches_only_receiverless_init_declarations() {
        assert!(func_decl_is_package_init(&func_decl("init", None)));
        assert!(!func_decl_is_package_init(&func_decl("Init", None)));
        assert!(!func_decl_is_package_init(&func_decl(
            "init",
            Some(field_list())
        )));
    }

    #[test]
    fn unsafe_pointer_selector_matches_exact_package_member() {
        assert_eq!(
            selector_unsafe_member(&selector("unsafe", "Pointer")),
            Some("Pointer")
        );
        assert!(selector_is_unsafe_pointer(&selector("unsafe", "Pointer")));
        assert!(!selector_is_unsafe_pointer(&selector("unsafe", "Sizeof")));
        assert!(!selector_is_unsafe_pointer(&selector("safe", "Pointer")));
        assert!(selector_is_unsafe_constant(&selector("unsafe", "Sizeof")));
        assert!(!selector_is_unsafe_constant(&selector("unsafe", "Pointer")));
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
        assert_eq!(call_unsafe_member(&call), Some("Pointer"));
        assert_eq!(call_unsafe_constant_member(&call), None);
    }
}
