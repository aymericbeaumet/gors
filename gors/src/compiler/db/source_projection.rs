//! Projection of parser-borrowed syntax into owned source metadata.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::ast;
use crate::parser::{SourceContent, decode_import_path_literal};
use crate::scanner::Scanner;
use crate::token::Token;

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
            let (line, column) = content
                .line_column(byte_start)
                .unwrap_or((comment.slash.line, comment.slash.column));
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

pub(super) fn signature_source(
    content: &SourceContent,
    logical_path: &str,
    function: &ast::FuncDecl<'_>,
) -> Arc<str> {
    let start = function
        .type_
        .func
        .as_ref()
        .map_or(function.name.name_pos.offset, |position| position.offset);
    let end = function.body.as_ref().map_or_else(
        || bodyless_signature_end(content, logical_path, start),
        |body| body.lbrace.offset,
    );
    source_range(content.source(), start, end)
}

pub(super) fn body_source(content: &SourceContent, body: &ast::BlockStmt<'_>) -> Arc<str> {
    source_range(
        content.source(),
        body.lbrace.offset,
        body.rbrace.offset.saturating_add(1),
    )
}

fn source_range(source: &str, start: usize, end: usize) -> Arc<str> {
    source
        .get(start..end)
        .map_or_else(|| Arc::from(""), Arc::from)
}

fn bodyless_signature_end(content: &SourceContent, logical_path: &str, start: usize) -> usize {
    let Some(suffix) = content.source().get(start..) else {
        return content.source().len();
    };
    let mut scanner = Scanner::new(logical_path, suffix);
    let mut nesting = 0_u32;
    loop {
        let Ok((position, token, _)) = scanner.scan() else {
            return content.source().len();
        };
        match token {
            Token::LPAREN | Token::LBRACK | Token::LBRACE => {
                nesting = nesting.saturating_add(1);
            }
            Token::RPAREN | Token::RBRACK | Token::RBRACE => {
                nesting = nesting.saturating_sub(1);
            }
            Token::SEMICOLON if nesting == 0 => {
                return start.saturating_add(position.offset);
            }
            Token::EOF => return content.source().len(),
            _ => {}
        }
    }
}
