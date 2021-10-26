// https://golang.org/ref/spec

use crate::ast;
use nom::{bytes::complete::tag, IResult};

mod identifier;
mod string;
mod whitespace;

#[derive(Debug)]
pub enum ParseError {
    Unexpected(String),
    Remaining(String),
}

impl std::error::Error for ParseError {}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ParseError::Unexpected(cause) => {
                write!(f, "unexpected parsing error: {:?}", cause)
            }
            ParseError::Remaining(code) => {
                write!(
                    f,
                    "remaining code after the parsing is finished: {:?}",
                    code
                )
            }
        }
    }
}

pub fn parse(input: &str) -> Result<ast::File, ParseError> {
    let (input, package_name) =
        package(input).map_err(|err| ParseError::Unexpected(err.to_string()))?;

    if !input.is_empty() {
        return Err(ParseError::Remaining(input.to_owned()));
    }

    Ok(ast::File {
        name: ast::Ident {
            name: package_name.to_owned(),
        },
    })
}

fn package(input: &str) -> IResult<&str, &str> {
    let (input, _) = whitespace::before_opt(tag("package"))(input)?;
    let (input, name) = whitespace::before_req(identifier::identifier)(input)?;
    Ok((input, name))
}
