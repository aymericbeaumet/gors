#![allow(non_snake_case)]

use crate::ast;

pub trait Visitor<'a> {
    fn BlockStmt(&mut self, _: &'a ast::BlockStmt<'a>) {}
    fn File(&mut self, _: &'a ast::File<'a>) {}
    fn FuncDecl(&mut self, _: &'a ast::FuncDecl<'a>) {}
    fn GenDecl(&mut self, _: &'a ast::GenDecl<'a>) {}
    fn ImportSpec(&mut self, _: &'a ast::ImportSpec<'a>) {}
    fn ValueSpec(&mut self, _: &'a ast::ValueSpec<'a>) {}
}

pub trait Visitable<'a, V: Visitor<'a>> {
    fn visit(&self, v: &mut V);
}
