// https://golang.org/ref/spec

use crate::ast;
use nom::{bytes::complete::tag, IResult};

#[derive(Debug)]
pub struct ParseError {}

impl std::error::Error for ParseError {}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Parse Error")
    }
}

pub fn parse(input: &str) -> Result<ast::File, ParseError> {
    let (_, package_name) = package(input).map_err(|_| ParseError {
        // TODO
    })?;

    Ok(ast::File {
        name: ast::Ident {
            name: package_name.to_owned(),
        },
    })
}

fn package(input: &str) -> IResult<&str, &str> {
    let (input, _) = tag("package")(input)?;
    Ok((input, "main"))
}
