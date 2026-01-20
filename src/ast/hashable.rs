use crate::ast;
use std::hash::{Hash, Hasher};

impl<'a> Hash for &ast::ImportSpec<'a> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::ptr::hash((*self) as *const ast::ImportSpec, state);
    }
}

impl<'a> Hash for &ast::CommentGroup<'a> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Use content-based hashing since CommentGroups are stored by value
        // and the same logical group can exist in multiple places (Doc and Comments).
        // The position of the first comment uniquely identifies the group.
        if let Some(first) = self.list.first() {
            first.slash.line.hash(state);
            first.slash.column.hash(state);
            first.slash.file.hash(state);
        }
    }
}
