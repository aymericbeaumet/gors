//! One-pass projection from parser observations into owned function syntax.

use std::fmt;

use crate::ast;
use crate::parser::TokenObservation;
use crate::source::{TextRange, TextSize};
use crate::token::Token;

use super::{FunctionLayout, SemanticTokenStream, SyntaxAnchor};

pub struct ProjectedFunctionSyntax {
    pub(crate) anchor: SyntaxAnchor,
    pub(crate) layout: FunctionLayout,
    pub(crate) header: SemanticTokenStream,
    pub(crate) body: Option<SemanticTokenStream>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectionError {
    MissingFunctionToken,
    MissingBodyBrace,
    MissingBodylessTerminator,
    OffsetOutsideTextDomain { offset: usize },
    ReversedRange { start: usize, end: usize },
}

impl fmt::Display for ProjectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingFunctionToken => {
                formatter.write_str("parser observations omitted the function token")
            }
            Self::MissingBodyBrace => {
                formatter.write_str("parser observations omitted a function body brace")
            }
            Self::MissingBodylessTerminator => {
                formatter.write_str("parser observations omitted a bodyless declaration terminator")
            }
            Self::OffsetOutsideTextDomain { offset } => {
                write!(
                    formatter,
                    "function layout offset {offset} exceeds the text domain"
                )
            }
            Self::ReversedRange { start, end } => {
                write!(
                    formatter,
                    "function layout range {start}..{end} is reversed"
                )
            }
        }
    }
}

pub fn project_function(
    function: &ast::FuncDecl<'_>,
    observations: &[TokenObservation<'_>],
) -> Result<ProjectedFunctionSyntax, ProjectionError> {
    let declaration_start = function
        .type_
        .func
        .as_ref()
        .map_or(function.name.name_pos.offset, |position| position.offset);
    let start_index = observations
        .iter()
        .position(|observation| {
            observation.byte_offset() == declaration_start && observation.token() == Token::FUNC
        })
        .ok_or(ProjectionError::MissingFunctionToken)?;
    let anchor = SyntaxAnchor::named_function(function.name.name);

    match &function.body {
        Some(body) => project_with_body(anchor, declaration_start, start_index, body, observations),
        None => project_without_body(anchor, declaration_start, start_index, observations),
    }
}

fn project_with_body(
    anchor: SyntaxAnchor,
    declaration_start: usize,
    start_index: usize,
    body: &ast::BlockStmt<'_>,
    observations: &[TokenObservation<'_>],
) -> Result<ProjectedFunctionSyntax, ProjectionError> {
    let left_index = observations
        .iter()
        .enumerate()
        .skip(start_index)
        .find_map(|(index, observation)| {
            (observation.byte_offset() == body.lbrace.offset
                && observation.token() == Token::LBRACE)
                .then_some(index)
        })
        .ok_or(ProjectionError::MissingBodyBrace)?;
    let right_index = observations
        .iter()
        .enumerate()
        .skip(left_index)
        .find_map(|(index, observation)| {
            (observation.byte_offset() == body.rbrace.offset
                && observation.token() == Token::RBRACE)
                .then_some(index)
        })
        .ok_or(ProjectionError::MissingBodyBrace)?;
    let declaration_end = observations.get(right_index).map_or_else(
        || body.rbrace.offset.saturating_add(1),
        |token| token.byte_end(),
    );
    let header_range = text_range(declaration_start, body.lbrace.offset)?;
    let body_range = text_range(body.lbrace.offset, declaration_end)?;
    let declaration_range = text_range(declaration_start, declaration_end)?;
    let header = SemanticTokenStream::from_observations(
        observations
            .get(start_index..left_index)
            .ok_or(ProjectionError::MissingBodyBrace)?,
    );
    let body = SemanticTokenStream::from_observations(
        observations
            .get(left_index..=right_index)
            .ok_or(ProjectionError::MissingBodyBrace)?,
    );
    Ok(ProjectedFunctionSyntax {
        anchor: anchor.clone(),
        layout: FunctionLayout::new(anchor, declaration_range, header_range, Some(body_range)),
        header,
        body: Some(body),
    })
}

fn project_without_body(
    anchor: SyntaxAnchor,
    declaration_start: usize,
    start_index: usize,
    observations: &[TokenObservation<'_>],
) -> Result<ProjectedFunctionSyntax, ProjectionError> {
    let mut nesting = 0_u32;
    let terminator_index = observations
        .iter()
        .enumerate()
        .skip(start_index)
        .find_map(|(index, observation)| {
            match observation.token() {
                Token::LPAREN | Token::LBRACK | Token::LBRACE => {
                    nesting = nesting.saturating_add(1);
                }
                Token::RPAREN | Token::RBRACK | Token::RBRACE => {
                    nesting = nesting.saturating_sub(1);
                }
                Token::SEMICOLON if nesting == 0 => return Some(index),
                Token::EOF => return Some(index),
                _ => {}
            }
            None
        })
        .ok_or(ProjectionError::MissingBodylessTerminator)?;
    let terminator = observations
        .get(terminator_index)
        .ok_or(ProjectionError::MissingBodylessTerminator)?;
    if terminator.token() == Token::EOF {
        return Err(ProjectionError::MissingBodylessTerminator);
    }
    let header_end = terminator.byte_offset();
    let declaration_end = terminator.byte_end();
    let header_range = text_range(declaration_start, header_end)?;
    let declaration_range = text_range(declaration_start, declaration_end)?;
    let header = SemanticTokenStream::from_observations(
        observations
            .get(start_index..terminator_index)
            .ok_or(ProjectionError::MissingBodylessTerminator)?,
    );
    Ok(ProjectedFunctionSyntax {
        anchor: anchor.clone(),
        layout: FunctionLayout::new(anchor, declaration_range, header_range, None),
        header,
        body: None,
    })
}

fn text_range(start: usize, end: usize) -> Result<TextRange, ProjectionError> {
    if start > end {
        return Err(ProjectionError::ReversedRange { start, end });
    }
    let start = TextSize::try_from(start)
        .map_err(|_| ProjectionError::OffsetOutsideTextDomain { offset: start })?;
    let end = TextSize::try_from(end)
        .map_err(|_| ProjectionError::OffsetOutsideTextDomain { offset: end })?;
    TextRange::new(start, end).map_err(|_| ProjectionError::ReversedRange {
        start: start.to_usize(),
        end: end.to_usize(),
    })
}
