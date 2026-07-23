//! Projection of parser-borrowed syntax into owned source metadata.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::ast;
use crate::compiler::input::SourceContent;
use crate::parser::decode_import_path_literal;
use crate::source::TextSize;

use super::super::ids::FileId;
use super::source_metadata::{
    DirectImport, FileComments, FileImports, InvalidImport, SourceComment,
};

pub(super) fn project_imports(file: FileId, parsed: &ast::File<'_>) -> FileImports {
    let mut direct = Vec::new();
    let mut invalid = Vec::new();
    for spec in parsed.imports() {
        let literal: Arc<str> = Arc::from(spec.path.value);
        let position = spec.path.value_pos;
        let virtual_file = position
            .origin
            .is_line_directive()
            .then(|| Arc::<str>::from(position.filename().as_ref()));
        match decode_import_path_literal(spec.path.value) {
            Ok(path) => direct.push(DirectImport::new(
                file,
                Arc::from(path),
                literal,
                position.offset,
                position.line,
                position.column,
                virtual_file,
            )),
            Err(issue) => invalid.push(InvalidImport::new(
                file,
                literal,
                position.offset,
                position.line,
                position.column,
                virtual_file,
                issue,
            )),
        }
    }
    FileImports::new(file, direct.into(), invalid.into())
}

pub(super) fn project_comments(
    file: FileId,
    content: &SourceContent,
    parsed: &ast::File<'_>,
) -> FileComments {
    let mut doc_comment_offsets = BTreeSet::new();
    for declaration in &parsed.decls {
        if let ast::Decl::FuncDecl(function) = declaration
            && let Some(doc) = &function.doc
        {
            doc_comment_offsets.extend(doc.list.iter().map(|comment| comment.slash.offset));
        }
    }

    let comments = parsed
        .comments
        .iter()
        .flat_map(|group| &group.list)
        .map(|comment| {
            let byte_start = comment.slash.offset;
            let physical = TextSize::try_from(byte_start)
                .ok()
                .and_then(|offset| content.physical_line_column(offset).ok().flatten())
                .and_then(|position| {
                    Some((
                        usize::try_from(position.line().get()).ok()?,
                        usize::try_from(position.byte_column().get()).ok()?,
                    ))
                });
            let (line, column) = physical.unwrap_or((comment.slash.line, comment.slash.column));
            SourceComment::new(
                file,
                Arc::from(comment.text),
                byte_start,
                byte_start.saturating_add(comment.text.len()),
                line,
                column,
                doc_comment_offsets.contains(&byte_start),
            )
        })
        .collect::<Vec<_>>();
    FileComments::new(file, comments.into())
}
