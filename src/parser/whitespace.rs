// https://github.com/Geal/nom/blob/main/doc/nom_recipes.md#wrapper-combinators-that-eat-whitespace-before-and-after-a-parser

use nom::{
    character::complete::{multispace0, multispace1},
    error::ParseError,
    sequence::preceded,
    IResult,
};

pub fn before_opt<'a, F: 'a, O, E: ParseError<&'a str>>(
    inner: F,
) -> impl FnMut(&'a str) -> IResult<&'a str, O, E>
where
    F: Fn(&'a str) -> IResult<&'a str, O, E>,
{
    preceded(multispace0, inner)
}

pub fn before_req<'a, F: 'a, O, E: ParseError<&'a str>>(
    inner: F,
) -> impl FnMut(&'a str) -> IResult<&'a str, O, E>
where
    F: Fn(&'a str) -> IResult<&'a str, O, E>,
{
    preceded(multispace1, inner)
}
