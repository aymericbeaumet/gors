//! Errors from projecting parser observations into owned structural syntax.

use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectionError {
    MissingFunctionToken,
    MissingBodyBrace,
    MissingBodylessTerminator,
    InvalidSwitchBody,
    InvalidTypeSwitchGuard,
    InvalidChannelDirection,
    InvalidSelectBody,
    MissingTypeName,
    InvalidMethodReceiver,
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
            Self::InvalidSwitchBody => {
                formatter.write_str("parser produced a non-case statement in a switch body")
            }
            Self::InvalidTypeSwitchGuard => formatter.write_str("invalid parser type-switch guard"),
            Self::InvalidChannelDirection => {
                formatter.write_str("parser produced an invalid channel direction")
            }
            Self::InvalidSelectBody => {
                formatter.write_str("parser produced a select body without communication clauses")
            }
            Self::MissingTypeName => formatter.write_str("parser produced a type without a name"),
            Self::InvalidMethodReceiver => {
                formatter.write_str("parser produced an invalid method receiver")
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
