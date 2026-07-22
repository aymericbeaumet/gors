//! Source-position extraction for parser AST nodes.

use crate::ast;
use crate::token::Position;

pub(super) fn expr_position<'a>(expr: &'a ast::Expr<'a>) -> Position<'a> {
    match expr {
        ast::Expr::ArrayType(expr) => expr.lbrack,
        ast::Expr::BasicLit(expr) => expr.value_pos,
        ast::Expr::BinaryExpr(expr) => expr.op_pos,
        ast::Expr::CallExpr(expr) => expr.lparen,
        ast::Expr::ChanType(expr) => expr.begin,
        ast::Expr::CompositeLit(expr) => expr.lbrace,
        ast::Expr::Ellipsis(expr) => expr.ellipsis,
        ast::Expr::FuncLit(expr) => expr.type_.func.unwrap_or_default(),
        ast::Expr::FuncType(expr) => expr.func.unwrap_or_default(),
        ast::Expr::Ident(expr) => expr.name_pos,
        ast::Expr::IndexExpr(expr) => expr.lbrack,
        ast::Expr::IndexListExpr(expr) => expr.lbrack,
        ast::Expr::InterfaceType(expr) => expr.interface,
        ast::Expr::KeyValueExpr(expr) => expr.colon,
        ast::Expr::MapType(expr) => expr.map,
        ast::Expr::ParenExpr(expr) => expr.lparen,
        ast::Expr::SelectorExpr(expr) => expr.sel.name_pos,
        ast::Expr::SliceExpr(expr) => expr.lbrack,
        ast::Expr::StarExpr(expr) => expr.star,
        ast::Expr::StructType(expr) => expr.struct_,
        ast::Expr::TypeAssertExpr(expr) => expr.lparen,
        ast::Expr::UnaryExpr(expr) => expr.op_pos,
    }
}

pub(super) fn stmt_position<'a>(stmt: &'a ast::Stmt<'a>) -> Position<'a> {
    match stmt {
        ast::Stmt::AssignStmt(stmt) => stmt.tok_pos,
        ast::Stmt::BlockStmt(stmt) => stmt.lbrace,
        ast::Stmt::BranchStmt(stmt) => stmt.tok_pos,
        ast::Stmt::CaseClause(stmt) => stmt.case,
        ast::Stmt::CommClause(stmt) => stmt.case,
        ast::Stmt::DeclStmt(stmt) => stmt.decl.tok_pos,
        ast::Stmt::DeferStmt(stmt) => stmt.defer,
        ast::Stmt::EmptyStmt(stmt) => stmt.semicolon,
        ast::Stmt::ExprStmt(stmt) => expr_position(&stmt.x),
        ast::Stmt::ForStmt(stmt) => stmt.for_,
        ast::Stmt::GoStmt(stmt) => stmt.go,
        ast::Stmt::IfStmt(stmt) => stmt.if_,
        ast::Stmt::IncDecStmt(stmt) => stmt.tok_pos,
        ast::Stmt::LabeledStmt(stmt) => stmt.colon,
        ast::Stmt::RangeStmt(stmt) => stmt.for_,
        ast::Stmt::ReturnStmt(stmt) => stmt.return_,
        ast::Stmt::SelectStmt(stmt) => stmt.select,
        ast::Stmt::SendStmt(stmt) => stmt.arrow,
        ast::Stmt::SwitchStmt(stmt) => stmt.switch,
        ast::Stmt::TypeSwitchStmt(stmt) => stmt.switch,
    }
}
