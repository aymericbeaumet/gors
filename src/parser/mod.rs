// https://golang.org/ref/spec

mod whitespace;

use crate::ast;
use nom::{
    branch::alt,
    bytes::complete::tag,
    character::complete::{alpha1, alphanumeric1},
    combinator::recognize,
    multi::many0,
    sequence::pair,
    IResult,
};

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

pub fn parse(filename: &str, src: &str) -> Result<ast::File, ParseError> {
    let (input, name) = package(src).map_err(|err| ParseError::Unexpected(err.to_string()))?;

    let (input, decls) = decls(input).map_err(|err| ParseError::Unexpected(err.to_string()))?;

    if !input.trim().is_empty() {
        return Err(ParseError::Remaining(input.to_owned()));
    }

    Ok(ast::File {
        filename: filename.to_owned(),
        name,
        decls,
    })
}

fn package(input: &str) -> IResult<&str, ast::Ident> {
    let (input, _) = whitespace::before_opt(tag("package"))(input)?;
    let (input, name) = whitespace::before_req(ident)(input)?;
    Ok((input, name))
}

fn decls(input: &str) -> IResult<&str, Vec<ast::Decl>> {
    let (input, decl) = decl(input)?;
    Ok((input, vec![decl]))
}

fn decl(input: &str) -> IResult<&str, ast::Decl> {
    let (input, _) = whitespace::before_req(tag("func"))(input)?;
    let (input, name) = whitespace::before_req(ident)(input)?;

    let (input, _) = whitespace::before_opt(tag("("))(input)?;
    // TODO: parse parameters
    let (input, _) = whitespace::before_opt(tag(")"))(input)?;

    let (input, _) = whitespace::before_opt(tag("{"))(input)?;
    // TODO: parse body
    let (input, _) = whitespace::before_opt(tag("}"))(input)?;

    Ok((
        input,
        ast::Decl::FuncDecl(ast::FuncDecl {
            name,
            type_: ast::FuncType {
                params: ast::FieldList {},
            },
            body: ast::BlockStmt {},
        }),
    ))
}

pub fn ident(input: &str) -> IResult<&str, ast::Ident> {
    let (input, name) = recognize(pair(
        alt((alpha1, tag("_"))),
        many0(alt((alphanumeric1, tag("_")))),
    ))(input)?;

    Ok((
        input,
        ast::Ident {
            name: name.to_owned(),
        },
    ))
}
