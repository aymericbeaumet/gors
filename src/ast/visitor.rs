#![allow(non_snake_case)]

use crate::ast;

pub trait Visitor<'a> {
    fn visit<T>(&mut self, node: T)
    where
        T: Visitable<'a, Self>,
        Self: Sized,
    {
        node.visit(self);
    }

    fn AssignStmt(&mut self, _: &'a ast::AssignStmt<'a>) {}
    fn BasicLit(&mut self, _: &'a ast::BasicLit<'a>) {}
    fn BinaryExpr(&mut self, _: &'a ast::BinaryExpr<'a>) {}
    fn BlockStmt(&mut self, _: &'a ast::BlockStmt<'a>) {}
    fn File(&mut self, _: &'a ast::File<'a>) {}
    fn FuncDecl(&mut self, _: &'a ast::FuncDecl<'a>) {}
    fn GenDecl(&mut self, _: &'a ast::GenDecl<'a>) {}
    fn Ident(&mut self, _: &'a ast::Ident<'a>) {}
    fn ImportSpec(&mut self, _: &'a ast::ImportSpec<'a>) {}
    fn ReturnStmt(&mut self, _: &'a ast::ReturnStmt<'a>) {}
    fn ValueSpec(&mut self, _: &'a ast::ValueSpec<'a>) {}
}

pub trait Visitable<'a, V: Visitor<'a>> {
    fn visit(&self, v: &mut V);
}
