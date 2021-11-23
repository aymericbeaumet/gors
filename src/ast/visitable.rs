use crate::ast;
use crate::ast::visitor::{Visitable, Visitor};

impl<V: Visitor, T: Visitable<V>> Visitable<V> for Option<T> {
    fn visit(&self, visitor: &mut V) {
        if let Some(some) = self {
            some.visit(visitor);
        }
    }
}

impl<V: Visitor, T: Visitable<V>> Visitable<V> for Vec<T> {
    fn visit(&self, visitor: &mut V) {
        self.iter().for_each(|visitable| visitable.visit(visitor));
    }
}

impl<V: Visitor> Visitable<V> for &ast::File<'_> {
    fn visit(&self, visitor: &mut V) {
        visitor.File(self);
        self.decls.visit(visitor);
    }
}

impl<V: Visitor> Visitable<V> for &ast::FuncDecl<'_> {
    fn visit(&self, visitor: &mut V) {
        visitor.FuncDecl(self);
        self.body.visit(visitor);
    }
}

impl<V: Visitor> Visitable<V> for &ast::BlockStmt<'_> {
    fn visit(&self, visitor: &mut V) {
        visitor.BlockStmt(self);
    }
}

impl<V: Visitor> Visitable<V> for &ast::GenDecl<'_> {
    fn visit(&self, visitor: &mut V) {
        visitor.GenDecl(self);
    }
}

impl<V: Visitor> Visitable<V> for ast::Decl<'_> {
    fn visit(&self, visitor: &mut V) {
        match self {
            ast::Decl::FuncDecl(func_decl) => visitor.FuncDecl(func_decl),
            ast::Decl::GenDecl(gen_decl) => visitor.GenDecl(gen_decl),
        };
    }
}
