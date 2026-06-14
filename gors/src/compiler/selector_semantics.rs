//! Shared Go selector facts derived from the type environment.

use crate::ast;

use super::typeinfer::{GoType, TypeEnv};

pub(super) fn declared_value_type(name: &str, env: &TypeEnv) -> Option<GoType> {
    env.get_var(name).or_else(|| env.get_top_level_var(name))
}

pub(super) fn selector_base_declared_value_type(
    selector: &ast::SelectorExpr<'_>,
    env: &TypeEnv,
) -> Option<GoType> {
    match unparen_expr(&selector.x) {
        ast::Expr::Ident(base) => declared_value_type(base.name, env),
        ast::Expr::SelectorExpr(base) => {
            let key = qualified_member_key(base)?;
            declared_value_type(&key, env)
        }
        _ => None,
    }
}

pub(super) fn selector_base_is_declared_value(
    selector: &ast::SelectorExpr<'_>,
    env: &TypeEnv,
) -> bool {
    selector_base_declared_value_type(selector, env).is_some()
}

pub(super) fn qualified_member_key(selector: &ast::SelectorExpr<'_>) -> Option<String> {
    let ast::Expr::Ident(base) = selector.x.as_ref() else {
        return None;
    };
    Some(format!("{}.{}", base.name, selector.sel.name))
}

fn unparen_expr<'a>(expr: &'a ast::Expr<'a>) -> &'a ast::Expr<'a> {
    match expr {
        ast::Expr::ParenExpr(paren) => unparen_expr(&paren.x),
        _ => expr,
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::parser::parse_file;

    fn first_selector_expr<'a>(file: &'a ast::File<'a>) -> &'a ast::SelectorExpr<'a> {
        let ast::Decl::FuncDecl(func) = file.decls.first().expect("function declaration") else {
            panic!("expected function");
        };
        let ast::Stmt::ExprStmt(stmt) = func
            .body
            .as_ref()
            .expect("function body")
            .list
            .first()
            .expect("expression statement")
        else {
            panic!("expected expression statement");
        };
        let ast::Expr::SelectorExpr(selector) = &stmt.x else {
            panic!("expected selector expression");
        };
        selector
    }

    #[test]
    fn selector_base_declared_value_detects_identifier_values() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func main() {
                    T.M
                }
            "#,
        )
        .unwrap();
        let selector = first_selector_expr(&file);
        let mut env = TypeEnv::new();

        assert!(!selector_base_is_declared_value(selector, &env));

        env.set_var("T", GoType::Int);

        assert_eq!(
            selector_base_declared_value_type(selector, &env),
            Some(GoType::Int)
        );
    }

    #[test]
    fn selector_base_declared_value_detects_package_member_values() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func main() {
                    pkg.Value.M
                }
            "#,
        )
        .unwrap();
        let selector = first_selector_expr(&file);
        let mut env = TypeEnv::new();

        assert!(!selector_base_is_declared_value(selector, &env));

        env.set_top_level_var("pkg.Value", GoType::Named("Value".to_string()));

        assert_eq!(
            selector_base_declared_value_type(selector, &env),
            Some(GoType::Named("Value".to_string()))
        );
    }
}
