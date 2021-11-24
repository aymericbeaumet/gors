use crate::ast;
use crate::ast::visitor::{Visitable, Visitor};

impl<'a, V: Visitor<'a>, T: Visitable<'a, V>> Visitable<'a, V> for Option<T> {
    fn visit(&self, visitor: &mut V) {
        if let Some(some) = self {
            some.visit(visitor);
        }
    }
}

impl<'a, V: Visitor<'a>, T: Visitable<'a, V>> Visitable<'a, V> for &Vec<T> {
    fn visit(&self, visitor: &mut V) {
        self.iter().for_each(|visitable| visitable.visit(visitor));
    }
}

impl<'a, V: Visitor<'a>, T: Visitable<'a, V>> Visitable<'a, V> for Vec<T> {
    fn visit(&self, visitor: &mut V) {
        self.iter().for_each(|visitable| visitable.visit(visitor));
    }
}

impl<'a, V: Visitor<'a>> Visitable<'a, V> for &'a ast::File<'a> {
    fn visit(&self, visitor: &mut V) {
        visitor.File(self);
        self.decls.visit(visitor);
    }
}

impl<'a, V: Visitor<'a>> Visitable<'a, V> for &'a ast::Ident<'a> {
    fn visit(&self, visitor: &mut V) {
        visitor.Ident(self);
    }
}

impl<'a, V: Visitor<'a>> Visitable<'a, V> for &'a ast::FuncDecl<'a> {
    fn visit(&self, visitor: &mut V) {
        visitor.FuncDecl(self);
        self.name.visit(visitor);
        self.body.visit(visitor);
    }
}

impl<'a, V: Visitor<'a>> Visitable<'a, V> for &'a ast::BlockStmt<'a> {
    fn visit(&self, visitor: &mut V) {
        visitor.BlockStmt(self);
        self.list.visit(visitor);
    }
}

impl<'a, V: Visitor<'a>> Visitable<'a, V> for &'a ast::GenDecl<'a> {
    fn visit(&self, visitor: &mut V) {
        visitor.GenDecl(self);
        self.specs.visit(visitor);
    }
}

impl<'a, V: Visitor<'a>> Visitable<'a, V> for &'a ast::ImportSpec<'a> {
    fn visit(&self, visitor: &mut V) {
        visitor.ImportSpec(self);
    }
}

impl<'a, V: Visitor<'a>> Visitable<'a, V> for &'a ast::ValueSpec<'a> {
    fn visit(&self, visitor: &mut V) {
        visitor.ValueSpec(self);
        self.names.visit(visitor);
        self.type_.visit(visitor);
    }
}

impl<'a, V: Visitor<'a>> Visitable<'a, V> for &'a ast::AssignStmt<'a> {
    fn visit(&self, visitor: &mut V) {
        visitor.AssignStmt(self);
        self.lhs.visit(visitor);
        self.rhs.visit(visitor);
    }
}

impl<'a, V: Visitor<'a>> Visitable<'a, V> for &'a ast::BasicLit<'a> {
    fn visit(&self, visitor: &mut V) {
        visitor.BasicLit(self);
    }
}

impl<'a, V: Visitor<'a>> Visitable<'a, V> for ast::Decl<'a> {
    fn visit(&self, visitor: &mut V) {
        match self {
            ast::Decl::FuncDecl(f) => f.visit(visitor),
            ast::Decl::GenDecl(g) => g.visit(visitor),
        };
    }
}

impl<'a, V: Visitor<'a>> Visitable<'a, V> for ast::Spec<'a> {
    fn visit(&self, visitor: &mut V) {
        match self {
            ast::Spec::ImportSpec(i) => i.visit(visitor),
            ast::Spec::ValueSpec(v) => v.visit(visitor),
        };
    }
}

impl<'a, V: Visitor<'a>> Visitable<'a, V> for ast::Stmt<'a> {
    fn visit(&self, visitor: &mut V) {
        match self {
            ast::Stmt::AssignStmt(i) => i.visit(visitor),
        };
    }
}

impl<'a, V: Visitor<'a>> Visitable<'a, V> for ast::Expr<'a> {
    fn visit(&self, visitor: &mut V) {
        match self {
            ast::Expr::BasicLit(b) => b.visit(visitor),
            ast::Expr::Ident(i) => i.visit(visitor),
        };
    }
}
