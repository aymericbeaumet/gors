use crate::ast;
use std::hash::{Hash, Hasher};

impl<'a> Hash for &ast::ImportSpec<'a> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::ptr::hash((*self) as *const ast::ImportSpec, state);
    }
}

impl<'a> Hash for &ast::CommentGroup<'a> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Use physical byte offset for hashing — adjusted line/column from
        // //line directives can collide for distinct comments.
        if let Some(first) = self.list.first() {
            first.slash.offset.hash(state);
        }
    }
}
