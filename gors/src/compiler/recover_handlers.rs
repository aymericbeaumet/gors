use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use super::{ast, token};

pub(super) type HandlerMap = BTreeMap<String, BTreeSet<String>>;

pub(super) struct ActiveHandlerGuard {
    previous: BTreeSet<String>,
}

thread_local! {
    static ACTIVE_HANDLERS: RefCell<BTreeSet<String>> = const { RefCell::new(BTreeSet::new()) };
}

impl ActiveHandlerGuard {
    pub(super) fn set(handlers: BTreeSet<String>) -> Self {
        let previous =
            ACTIVE_HANDLERS.with(|active| std::mem::replace(&mut *active.borrow_mut(), handlers));
        Self { previous }
    }
}

impl Drop for ActiveHandlerGuard {
    fn drop(&mut self) {
        ACTIVE_HANDLERS.with(|active| {
            *active.borrow_mut() = std::mem::take(&mut self.previous);
        });
    }
}

pub(super) fn is_active(method_name: &str) -> bool {
    ACTIVE_HANDLERS.with(|active| active.borrow().contains(method_name))
}

pub(super) fn collect(decls: &[ast::Decl<'_>]) -> HandlerMap {
    let mut handlers = HandlerMap::new();
    for decl in decls {
        let ast::Decl::FuncDecl(func_decl) = decl else {
            continue;
        };
        let Some(receiver) = receiver_type_name(func_decl) else {
            continue;
        };
        if method_starts_with_recover_guard(func_decl) {
            handlers
                .entry(receiver)
                .or_default()
                .insert(func_decl.name.name.to_string());
        }
    }
    handlers
}

fn receiver_type_name(func_decl: &ast::FuncDecl<'_>) -> Option<String> {
    let recv_type = func_decl.recv.as_ref()?.list.first()?.type_.as_ref()?;
    match recv_type {
        ast::Expr::Ident(ident) => Some(ident.name.to_string()),
        ast::Expr::StarExpr(star) => {
            let ast::Expr::Ident(ident) = star.x.as_ref() else {
                return None;
            };
            Some(ident.name.to_string())
        }
        _ => None,
    }
}

fn method_starts_with_recover_guard(func_decl: &ast::FuncDecl<'_>) -> bool {
    let Some(body) = &func_decl.body else {
        return false;
    };
    let Some(ast::Stmt::IfStmt(if_stmt)) = body.list.first() else {
        return false;
    };
    let Some(recover_ident) = recover_guard_init_ident(if_stmt.init.as_ref().as_ref()) else {
        return false;
    };
    if if_stmt.else_.as_ref().is_some() {
        return false;
    }
    expr_is_ident_nil_comparison(&if_stmt.cond, recover_ident, token::Token::NEQ)
}

fn recover_guard_init_ident<'a>(init: Option<&'a ast::Stmt<'a>>) -> Option<&'a str> {
    let ast::Stmt::AssignStmt(assign) = init? else {
        return None;
    };
    if assign.tok != token::Token::DEFINE {
        return None;
    }
    let [ast::Expr::Ident(ident)] = assign.lhs.as_slice() else {
        return None;
    };
    if ident.name == "_" {
        return None;
    }
    let [rhs] = assign.rhs.as_slice() else {
        return None;
    };
    call_is_predeclared_recover(rhs).then_some(ident.name)
}

fn call_is_predeclared_recover(expr: &ast::Expr<'_>) -> bool {
    let ast::Expr::CallExpr(call) = expr else {
        return false;
    };
    matches!(call.fun.as_ref(), ast::Expr::Ident(ident) if ident.name == "recover")
        && call.args.as_ref().is_none_or(Vec::is_empty)
}

fn expr_is_ident_nil_comparison(expr: &ast::Expr, ident_name: &str, op: token::Token) -> bool {
    let ast::Expr::BinaryExpr(binary) = expr else {
        return false;
    };
    binary.op == op
        && ((expr_is_ident_name(&binary.x, ident_name) && expr_is_nil(&binary.y))
            || (expr_is_nil(&binary.x) && expr_is_ident_name(&binary.y, ident_name)))
}

fn expr_is_ident_name(expr: &ast::Expr, name: &str) -> bool {
    matches!(expr, ast::Expr::Ident(ident) if ident.name == name)
}

fn expr_is_nil(expr: &ast::Expr) -> bool {
    matches!(expr, ast::Expr::Ident(ident) if ident.name == "nil")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_file;

    #[test]
    fn collect_records_methods_with_recover_guards_by_receiver() {
        let parsed = parse_file(
            "test.go",
            r#"
package main

type Printer struct{}

func (p *Printer) handle() {
	if err := recover(); err != nil {
		_ = err
	}
}

func (p *Printer) catchPanic() {}
"#,
        );
        assert!(parsed.is_ok(), "expected test fixture to parse: {parsed:?}");
        let Ok(parsed) = parsed else {
            return;
        };

        let handlers = collect(&parsed.decls);
        let Some(printer) = handlers.get("Printer") else {
            assert!(handlers.contains_key("Printer"), "{handlers:?}");
            return;
        };

        assert!(printer.contains("handle"));
        assert!(!printer.contains("catchPanic"));
    }

    #[test]
    fn active_handler_guard_restores_previous_set() {
        let outer = BTreeSet::from(["outer".to_string()]);
        let inner = BTreeSet::from(["inner".to_string()]);
        let _outer = ActiveHandlerGuard::set(outer);
        assert!(is_active("outer"));
        {
            let _inner = ActiveHandlerGuard::set(inner);
            assert!(is_active("inner"));
            assert!(!is_active("outer"));
        }
        assert!(is_active("outer"));
        assert!(!is_active("inner"));
    }
}
