//! Parser-node positions used as stable projection source observations.

use crate::ast;
use crate::token::Position;

pub(super) fn expression_position<'a>(expression: &'a ast::Expr<'a>) -> Position<'a> {
    match expression {
        ast::Expr::ArrayType(expression) => expression.lbrack,
        ast::Expr::BasicLit(expression) => expression.value_pos,
        ast::Expr::BinaryExpr(expression) => expression.op_pos,
        ast::Expr::CallExpr(expression) => expression.lparen,
        ast::Expr::ChanType(expression) => expression.begin,
        ast::Expr::CompositeLit(expression) => expression.lbrace,
        ast::Expr::Ellipsis(expression) => expression.ellipsis,
        ast::Expr::FuncLit(expression) => expression.type_.func.unwrap_or_default(),
        ast::Expr::FuncType(expression) => expression.func.unwrap_or_default(),
        ast::Expr::Ident(expression) => expression.name_pos,
        ast::Expr::IndexExpr(expression) => expression.lbrack,
        ast::Expr::IndexListExpr(expression) => expression.lbrack,
        ast::Expr::InterfaceType(expression) => expression.interface,
        ast::Expr::KeyValueExpr(expression) => expression.colon,
        ast::Expr::MapType(expression) => expression.map,
        ast::Expr::ParenExpr(expression) => expression.lparen,
        ast::Expr::SelectorExpr(expression) => expression.sel.name_pos,
        ast::Expr::SliceExpr(expression) => expression.lbrack,
        ast::Expr::StarExpr(expression) => expression.star,
        ast::Expr::StructType(expression) => expression.struct_,
        ast::Expr::TypeAssertExpr(expression) => expression.lparen,
        ast::Expr::UnaryExpr(expression) => expression.op_pos,
    }
}

pub(super) fn statement_position<'a>(statement: &'a ast::Stmt<'a>) -> Position<'a> {
    match statement {
        ast::Stmt::AssignStmt(statement) => statement.tok_pos,
        ast::Stmt::BlockStmt(statement) => statement.lbrace,
        ast::Stmt::BranchStmt(statement) => statement.tok_pos,
        ast::Stmt::CaseClause(statement) => statement.case,
        ast::Stmt::CommClause(statement) => statement.case,
        ast::Stmt::DeclStmt(statement) => statement.decl.tok_pos,
        ast::Stmt::DeferStmt(statement) => statement.defer,
        ast::Stmt::EmptyStmt(statement) => statement.semicolon,
        ast::Stmt::ExprStmt(statement) => expression_position(&statement.x),
        ast::Stmt::ForStmt(statement) => statement.for_,
        ast::Stmt::GoStmt(statement) => statement.go,
        ast::Stmt::IfStmt(statement) => statement.if_,
        ast::Stmt::IncDecStmt(statement) => statement.tok_pos,
        ast::Stmt::LabeledStmt(statement) => statement.colon,
        ast::Stmt::RangeStmt(statement) => statement.for_,
        ast::Stmt::ReturnStmt(statement) => statement.return_,
        ast::Stmt::SelectStmt(statement) => statement.select,
        ast::Stmt::SendStmt(statement) => statement.arrow,
        ast::Stmt::SwitchStmt(statement) => statement.switch,
        ast::Stmt::TypeSwitchStmt(statement) => statement.switch,
    }
}
