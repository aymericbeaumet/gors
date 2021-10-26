// https://golang.org/ref/spec

use crate::ast;
use nom::{bytes::complete::tag, IResult};

use self::identifier::identifier;
mod identifier;
mod string;
mod whitespace;

#[derive(Debug)]
pub enum ParseError {
    Unexpected,
    Remaining(String),
}

impl std::error::Error for ParseError {}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Parse Error")
    }
}

pub fn parse(input: &str) -> Result<ast::File, ParseError> {
    let (input, package_name) = package(input).map_err(|err| {
        println!("TODO: {:?}", err);
        ParseError::Unexpected
    })?;

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
    let (input, _) = whitespace::whitespace(tag("package"))(input)?;
    let (input, name) = whitespace::whitespace(identifier)(input)?;
    Ok((input, name))
}
