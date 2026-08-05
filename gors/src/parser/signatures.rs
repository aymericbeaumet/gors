use super::{Result, ResultExt, core::Parser};
use crate::ast;
use crate::token::{Position, Token};

impl<'scanner> Parser<'scanner> {
    // Signature = Parameters [ Result ] .
    pub(super) fn parse_signature(
        &mut self,
        func: Option<Position<'scanner>>,
    ) -> Result<Option<ast::FuncType<'scanner>>> {
        log::debug!("Parser::parse_signature()");

        let params = match self.parse_parameters()? {
            Some(v) => v,
            None => return Ok(None),
        };
        let results = self.parse_result()?;

        Ok(Some(ast::FuncType {
            func,
            type_params: None,
            params,
            results,
        }))
    }

    // Result = Parameters | Type .
    pub(super) fn parse_result(&mut self) -> Result<Option<ast::FieldList<'scanner>>> {
        log::debug!("Parser::parse_result()");

        if let Some(parameters) = self.parse_parameters()? {
            Ok(Some(parameters))
        } else if let Some(type_) = self.parse_type()? {
            Ok(Some(ast::FieldList {
                opening: None,
                list: vec![ast::Field {
                    doc: None,
                    names: None,
                    tag: None,
                    type_: Some(type_),
                    comment: None,
                }],
                closing: None,
            }))
        } else {
            Ok(None)
        }
    }

    // Parameters = "(" [ ParameterList [ "," ] ] ")" .
    pub(super) fn parse_parameters(&mut self) -> Result<Option<ast::FieldList<'scanner>>> {
        log::debug!("Parser::parse_parameters()");

        let lparen = match self.token(Token::LPAREN)? {
            Some(v) => v,
            None => return Ok(None),
        };
        let list = self
            .parse_parameter_list()?
            .inspect(|_| {
                let _ = self.token(Token::COMMA);
            })
            .unwrap_or_default();
        let rparen = self.token(Token::RPAREN).required()?;

        Ok(Some(ast::FieldList {
            opening: Some(lparen.0),
            list,
            closing: Some(rparen.0),
        }))
    }
}
