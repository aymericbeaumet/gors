#![allow(non_snake_case)]

use crate::ast;

pub trait Visitor {
    fn BlockStmt(&self, _: &ast::BlockStmt) {}
    fn File(&self, _: &ast::File) {}
    fn FuncDecl(&self, _: &ast::FuncDecl) {}
    fn GenDecl(&self, _: &ast::GenDecl) {}
}

pub trait Visitable<V: Visitor> {
    fn visit(&self, v: &mut V);
}
