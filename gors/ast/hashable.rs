use crate::ast;
use std::hash::{Hash, Hasher};

impl<'a> Hash for &ast::ImportSpec<'a> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::ptr::hash((*self) as *const ast::ImportSpec, state);
    }
}
