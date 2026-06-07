//! Shared Go method-expression classification.

use crate::ast;

use super::typeinfer::{GoType, TypeEnv};

#[derive(Debug, Clone, PartialEq)]
pub(super) struct TypeMethodExpressionReceiver {
    pub(super) method_name: String,
    pub(super) go_type: GoType,
}

pub(super) fn selector_base_is_declared_value(sel: &ast::SelectorExpr<'_>, env: &TypeEnv) -> bool {
    match unparen_expr(&sel.x) {
        ast::Expr::Ident(id) => {
            env.get_var(id.name).is_some() || env.get_top_level_var(id.name).is_some()
        }
        ast::Expr::SelectorExpr(inner) => {
            let ast::Expr::Ident(pkg) = inner.x.as_ref() else {
                return false;
            };
            let key = format!("{}.{}", pkg.name, inner.sel.name);
            env.get_var(&key).is_some() || env.get_top_level_var(&key).is_some()
        }
        _ => false,
    }
}

pub(super) fn receiver_for_method(
    expr: &ast::Expr<'_>,
    method: &str,
    env: &TypeEnv,
) -> Option<TypeMethodExpressionReceiver> {
    let receiver = receiver(expr, env)?;
    env.has_func(&format!("{}.{}", receiver.method_name, method))
        .then_some(receiver)
}

pub(super) fn receiver(
    expr: &ast::Expr<'_>,
    env: &TypeEnv,
) -> Option<TypeMethodExpressionReceiver> {
    match unparen_expr(expr) {
        ast::Expr::Ident(ident) if env.get_type_kind(ident.name).is_some() => {
            let name = ident.name.to_string();
            Some(TypeMethodExpressionReceiver {
                method_name: name.clone(),
                go_type: GoType::Named(name),
            })
        }
        ast::Expr::SelectorExpr(selector) => {
            let ast::Expr::Ident(pkg) = selector.x.as_ref() else {
                return None;
            };
            let name = format!("{}.{}", pkg.name, selector.sel.name);
            env.get_type_kind(&name)
                .is_some()
                .then_some(TypeMethodExpressionReceiver {
                    method_name: name.clone(),
                    go_type: GoType::Named(name),
                })
        }
        ast::Expr::StarExpr(star) => {
            let inner = receiver(&star.x, env)?;
            Some(TypeMethodExpressionReceiver {
                method_name: inner.method_name,
                go_type: GoType::Pointer(Box::new(inner.go_type)),
            })
        }
        ast::Expr::IndexExpr(_) | ast::Expr::IndexListExpr(_) => {
            let base_expr = indexed_base_expr(expr)?;
            let base = receiver(base_expr, env)?;
            let go_type = GoType::from_expr(expr);
            let go_type = if matches!(go_type, GoType::Unknown) {
                base.go_type
            } else {
                go_type
            };
            Some(TypeMethodExpressionReceiver {
                method_name: method_receiver_name(&go_type, env).unwrap_or(base.method_name),
                go_type,
            })
        }
        ast::Expr::ParenExpr(paren) => receiver(&paren.x, env),
        _ => None,
    }
}

pub(super) fn method_receiver_name(receiver_type: &GoType, env: &TypeEnv) -> Option<String> {
    match env.resolve_alias(receiver_type) {
        GoType::Named(name) | GoType::Interface(name) | GoType::Instantiated { name, .. } => {
            Some(name)
        }
        GoType::Pointer(inner) => method_receiver_name(&inner, env),
        _ => None,
    }
}

fn indexed_base_expr<'a>(expr: &'a ast::Expr<'a>) -> Option<&'a ast::Expr<'a>> {
    match unparen_expr(expr) {
        ast::Expr::IndexExpr(index) => Some(&index.x),
        ast::Expr::IndexListExpr(index) => Some(&index.x),
        _ => None,
    }
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
    use crate::compiler::typeinfer::TypeKind;
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
    fn selector_base_declared_value_detects_shadowing() {
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
        env.set_type_kind("T", TypeKind::Struct);

        assert!(!selector_base_is_declared_value(selector, &env));

        env.set_var("T", GoType::Int);

        assert!(selector_base_is_declared_value(selector, &env));
    }

    #[test]
    fn indexed_type_receiver_preserves_instantiated_receiver_type() {
        let file = parse_file(
            "test.go",
            r#"
                package main

                func main() {
                    T[int].M
                }
            "#,
        )
        .unwrap();
        let selector = first_selector_expr(&file);
        let mut env = TypeEnv::new();
        env.set_type_kind("T", TypeKind::Struct);
        env.set_func("T.M", vec![GoType::Int]);
        env.set_func_params("T.M", Vec::new());

        let receiver = receiver_for_method(&selector.x, selector.sel.name, &env).unwrap();

        assert_eq!(receiver.method_name, "T");
        assert_eq!(
            receiver.go_type,
            GoType::Instantiated {
                name: "T".to_string(),
                args: vec![GoType::Int],
            }
        );
    }
}
