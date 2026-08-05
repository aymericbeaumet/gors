//! Projection of parser-borrowed syntax into owned source metadata.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::ast;
use crate::compiler::input::SourceContent;
use crate::compiler::provenance::FileRange;
use crate::import_path::CanonicalImportPath;
use crate::parser::decode_import_path_literal;
use crate::source::{TextRange, TextSize};

use super::super::ids::FileId;
use super::source_metadata::{
    DirectImport, FileComments, FileImports, ImportBinding, ImportOccurrenceLocation,
    InvalidImport, SourceComment,
};

pub(super) fn project_imports(
    file: FileId,
    content: &SourceContent,
    parsed: &ast::File<'_>,
) -> FileImports {
    let mut direct = Vec::new();
    let mut invalid = Vec::new();
    for spec in parsed.imports() {
        let literal: Arc<str> = Arc::from(spec.path.value);
        let position = spec.path.value_pos;
        let virtual_file = position
            .origin
            .is_line_directive()
            .then(|| Arc::<str>::from(position.filename().as_ref()));
        let source = source_range(
            file,
            position.offset,
            spec.path.value.len(),
            content.text_len(),
        );
        let location = ImportOccurrenceLocation::new(
            file,
            source,
            position.offset,
            position.line,
            position.column,
            virtual_file,
        );
        match decode_import_path_literal(spec.path.value).and_then(CanonicalImportPath::new) {
            Ok(path) => {
                let binding = project_import_binding(file, content.text_len(), spec, source);
                direct.push(DirectImport::new(path, binding, literal, location));
            }
            Err(issue) => invalid.push(InvalidImport::new(literal, issue, location)),
        }
    }
    FileImports::new(file, direct.into(), invalid.into())
}

fn project_import_binding(
    file: FileId,
    source_len: TextSize,
    spec: &ast::ImportSpec<'_>,
    default_source: FileRange,
) -> ImportBinding {
    let Some(name) = &spec.name else {
        return ImportBinding::Default {
            source: default_source,
        };
    };
    let source = source_range(file, name.name_pos.offset, name.name.len(), source_len);
    match name.name {
        "_" => ImportBinding::Blank { source },
        "." => ImportBinding::Dot { source },
        _ => ImportBinding::Named {
            name: Arc::from(name.name),
            source,
        },
    }
}

fn source_range(file: FileId, offset: usize, length: usize, source_len: TextSize) -> FileRange {
    let source_len_usize = source_len.to_usize();
    let start = TextSize::try_from(offset.min(source_len_usize)).unwrap_or(source_len);
    let end = TextSize::try_from(offset.saturating_add(length).min(source_len_usize))
        .unwrap_or(source_len);
    let range = TextRange::new(start, end).unwrap_or_else(|_| TextRange::empty(start));
    FileRange::new(file, range)
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
