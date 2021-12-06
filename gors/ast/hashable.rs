use crate::ast;
use std::hash::{Hash, Hasher};

impl<'a> Hash for &ast::FuncDecl<'a> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::ptr::hash((*self) as *const ast::FuncDecl, state);
    }
}

impl<'a> Hash for &ast::Ident<'a> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::ptr::hash((*self) as *const ast::Ident, state);
    }
}

impl<'a> Hash for &ast::ImportSpec<'a> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::ptr::hash((*self) as *const ast::ImportSpec, state);
    }
}

impl<'a> Hash for &ast::Object<'a> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::ptr::hash((*self) as *const ast::Object, state)
    }
}

impl<'a> Hash for &ast::ValueSpec<'a> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::ptr::hash((*self) as *const ast::ValueSpec, state);
    }
}
