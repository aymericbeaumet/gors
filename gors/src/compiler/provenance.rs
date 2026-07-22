//! Compiler provenance anchored in stable file identity and physical source bytes.

use crate::source::TextRange;

use super::ids::FileId;

/// A physical byte range paired with its stable compiler file identity.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FileRange {
    file: FileId,
    range: TextRange,
}

impl FileRange {
    #[must_use]
    pub const fn new(file: FileId, range: TextRange) -> Self {
        Self { file, range }
    }

    #[must_use]
    pub const fn file(self) -> FileId {
        self.file
    }

    #[must_use]
    pub const fn range(self) -> TextRange {
        self.range
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::compiler::ids::IdentityInterner;
    use crate::compiler::input::{PackageKey, WorkspaceKey};
    use crate::source::TextSize;

    #[test]
    fn keeps_stable_file_identity_separate_from_bytes() {
        let mut interner = IdentityInterner::default();
        let workspace = interner
            .workspace(&WorkspaceKey::ad_hoc("coordinates").unwrap())
            .unwrap();
        let package = interner
            .package(workspace, &PackageKey::command_line())
            .unwrap();
        let file = interner.file(package, "main.go").unwrap();
        let range = TextRange::new(TextSize::new(3), TextSize::new(8)).unwrap();
        let file_range = FileRange::new(file, range);

        assert_eq!(file_range.file(), file);
        assert_eq!(file_range.range(), range);
    }
}
