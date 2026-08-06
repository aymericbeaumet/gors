//! One-pass projection from parser observations into owned function syntax.

use std::fmt;
use std::sync::Arc;

use crate::ast;
use crate::parser::TokenObservation;
use crate::source::{TextRange, TextSize};
use crate::token::{Position, Token};

use super::{
    BlockSyntax, ChannelDirectionSyntax, ConstantLayout, ConstantSyntax, ConstantValueSyntax,
    DeclSyntax, ExprSyntax, ExprSyntaxKind, FieldListSyntax, FieldSyntax, FunctionBodySyntax,
    FunctionHeaderSyntax, FunctionLayout, IdentSyntax, SemanticTokenStream, StmtSyntax,
    StmtSyntaxKind, SwitchCaseSyntax, SyntaxAnchor, SyntaxSource, SyntaxSourceRegion,
    TypeAliasSyntax, TypeDefinitionSyntax, ValueSpecSyntax,
};

pub struct ProjectedFunctionSyntax {
    pub(crate) anchor: SyntaxAnchor,
    pub(crate) layout: FunctionLayout,
    pub(crate) header: SemanticTokenStream,
    pub(crate) body: Option<SemanticTokenStream>,
    pub(crate) structural_header: FunctionHeaderSyntax,
    pub(crate) structural_body: FunctionBodySyntax,
}

pub struct ProjectedConstantSyntax {
    pub(crate) syntax: ConstantSyntax,
    pub(crate) layout: ConstantLayout,
}

struct FunctionProjectionParts {
    anchor: SyntaxAnchor,
    declaration_start: usize,
    start_index: usize,
    source_len: TextSize,
    header_sources: Arc<[TextRange]>,
    body_sources: Arc<[TextRange]>,
    structural_header: FunctionHeaderSyntax,
    structural_body: FunctionBodySyntax,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectionError {
    MissingFunctionToken,
    MissingBodyBrace,
    MissingBodylessTerminator,
    InvalidSwitchBody,
    InvalidChannelDirection,
    MissingTypeName,
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
            Self::InvalidChannelDirection => {
                formatter.write_str("parser produced an invalid channel direction")
            }
            Self::MissingTypeName => formatter.write_str("parser produced a type without a name"),
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
    let source_len = observations
        .last()
        .map(|observation| observation.byte_offset())
        .ok_or(ProjectionError::MissingFunctionToken)?;
    let source_len = TextSize::try_from(source_len)
        .map_err(|_| ProjectionError::OffsetOutsideTextDomain { offset: source_len })?;
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
    let mut header_projector = StructuralProjector::new(SyntaxSourceRegion::Header);
    let structural_header = header_projector.function_header(function)?;
    let header_sources = header_projector.finish();
    let mut body_projector = StructuralProjector::new(SyntaxSourceRegion::Body);
    let structural_body = FunctionBodySyntax {
        block: function
            .body
            .as_ref()
            .map(|body| body_projector.block(body))
            .transpose()?,
    };
    let body_sources = body_projector.finish();
    let parts = FunctionProjectionParts {
        anchor,
        declaration_start,
        start_index,
        source_len,
        header_sources,
        body_sources,
        structural_header,
        structural_body,
    };

    match &function.body {
        Some(body) => project_with_body(parts, body, observations),
        None => project_without_body(parts, observations),
    }
}

fn project_with_body(
    parts: FunctionProjectionParts,
    body: &ast::BlockStmt<'_>,
    observations: &[TokenObservation<'_>],
) -> Result<ProjectedFunctionSyntax, ProjectionError> {
    let FunctionProjectionParts {
        anchor,
        declaration_start,
        start_index,
        source_len,
        header_sources,
        body_sources,
        structural_header,
        structural_body,
    } = parts;
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
        layout: FunctionLayout::new(
            anchor,
            declaration_range,
            header_range,
            Some(body_range),
            source_len,
            header_sources,
            body_sources,
        ),
        header,
        body: Some(body),
        structural_header,
        structural_body,
    })
}

fn project_without_body(
    parts: FunctionProjectionParts,
    observations: &[TokenObservation<'_>],
) -> Result<ProjectedFunctionSyntax, ProjectionError> {
    let FunctionProjectionParts {
        anchor,
        declaration_start,
        start_index,
        source_len,
        header_sources,
        body_sources: _,
        structural_header,
        structural_body,
    } = parts;
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
        layout: FunctionLayout::new(
            anchor,
            declaration_range,
            header_range,
            None,
            source_len,
            header_sources,
            Arc::from([]),
        ),
        header,
        body: None,
        structural_header,
        structural_body,
    })
}

pub fn project_constant(
    name: &ast::Ident<'_>,
    explicit_type: Option<&ast::Expr<'_>>,
    value: Option<&ast::Expr<'_>>,
    arity_mismatch: bool,
    iota: u64,
    source_len: TextSize,
) -> Result<ProjectedConstantSyntax, ProjectionError> {
    let mut projector = StructuralProjector::new(SyntaxSourceRegion::Constant);
    let name = projector.ident(name)?;
    let declaration = projector.range(name.source)?;
    let explicit_type = explicit_type
        .map(|expression| projector.expression(expression))
        .transpose()?;
    let value = if arity_mismatch {
        ConstantValueSyntax::ArityMismatch
    } else {
        value
            .map(|expression| projector.expression(expression))
            .transpose()?
            .map_or(
                ConstantValueSyntax::ImplicitOrIota,
                ConstantValueSyntax::Expression,
            )
    };
    Ok(ProjectedConstantSyntax {
        syntax: ConstantSyntax {
            name,
            explicit_type,
            value,
            iota,
        },
        layout: ConstantLayout::new(declaration, source_len, projector.finish()),
    })
}

pub fn project_type_alias(spec: &ast::TypeSpec<'_>) -> Result<TypeAliasSyntax, ProjectionError> {
    let name = spec.name.as_ref().ok_or(ProjectionError::MissingTypeName)?;
    let mut projector = StructuralProjector::new(SyntaxSourceRegion::Constant);
    Ok(TypeAliasSyntax {
        name: projector.ident(name)?,
        target: projector.expression(&spec.type_)?,
    })
}

pub fn project_type_definition(
    spec: &ast::TypeSpec<'_>,
) -> Result<TypeDefinitionSyntax, ProjectionError> {
    let name = spec.name.as_ref().ok_or(ProjectionError::MissingTypeName)?;
    let mut projector = StructuralProjector::new(SyntaxSourceRegion::Constant);
    Ok(TypeDefinitionSyntax {
        name: projector.ident(name)?,
        underlying: projector.expression(&spec.type_)?,
    })
}

struct StructuralProjector {
    region: SyntaxSourceRegion,
    ranges: Vec<TextRange>,
}

impl StructuralProjector {
    const fn new(region: SyntaxSourceRegion) -> Self {
        Self {
            region,
            ranges: Vec::new(),
        }
    }

    fn finish(self) -> Arc<[TextRange]> {
        self.ranges.into()
    }

    fn source(&mut self, position: &Position<'_>) -> Result<SyntaxSource, ProjectionError> {
        let index = u32::try_from(self.ranges.len()).map_err(|_| {
            ProjectionError::OffsetOutsideTextDomain {
                offset: self.ranges.len(),
            }
        })?;
        let offset = TextSize::try_from(position.offset).map_err(|_| {
            ProjectionError::OffsetOutsideTextDomain {
                offset: position.offset,
            }
        })?;
        self.ranges.push(TextRange::empty(offset));
        Ok(SyntaxSource::new(self.region, index))
    }

    fn range(&self, source: SyntaxSource) -> Result<TextRange, ProjectionError> {
        usize::try_from(source.index())
            .ok()
            .and_then(|index| self.ranges.get(index))
            .copied()
            .ok_or_else(|| ProjectionError::OffsetOutsideTextDomain {
                offset: source.index() as usize,
            })
    }

    fn ident(&mut self, ident: &ast::Ident<'_>) -> Result<IdentSyntax, ProjectionError> {
        Ok(IdentSyntax {
            name: Arc::from(ident.name),
            source: self.source(&ident.name_pos)?,
        })
    }

    fn function_header(
        &mut self,
        function: &ast::FuncDecl<'_>,
    ) -> Result<FunctionHeaderSyntax, ProjectionError> {
        Ok(FunctionHeaderSyntax {
            name: self.ident(&function.name)?,
            has_receiver: function.recv.is_some(),
            has_type_parameters: function.type_.type_params.is_some(),
            params: self.field_list(&function.type_.params)?,
            results: function
                .type_
                .results
                .as_ref()
                .map(|fields| self.field_list(fields))
                .transpose()?,
        })
    }

    fn field_list(
        &mut self,
        fields: &ast::FieldList<'_>,
    ) -> Result<FieldListSyntax, ProjectionError> {
        let fields = fields
            .list
            .iter()
            .map(|field| {
                let (ty, variadic) = match field.type_.as_ref() {
                    Some(ast::Expr::Ellipsis(ellipsis)) => (
                        ellipsis
                            .elt
                            .as_deref()
                            .map(|expression| self.expression(expression))
                            .transpose()?,
                        true,
                    ),
                    expression => (
                        expression
                            .map(|expression| self.expression(expression))
                            .transpose()?,
                        false,
                    ),
                };
                Ok(FieldSyntax {
                    names: field
                        .names
                        .as_ref()
                        .map(|names| {
                            names
                                .iter()
                                .map(|name| self.ident(name))
                                .collect::<Result<Vec<_>, _>>()
                                .map(Arc::from)
                        })
                        .transpose()?,
                    ty,
                    variadic,
                })
            })
            .collect::<Result<Vec<_>, ProjectionError>>()?;
        Ok(FieldListSyntax {
            fields: fields.into(),
        })
    }

    fn block(&mut self, block: &ast::BlockStmt<'_>) -> Result<BlockSyntax, ProjectionError> {
        let source = self.source(&block.lbrace)?;
        let statements = block
            .list
            .iter()
            .map(|statement| self.statement(statement))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(BlockSyntax {
            source,
            statements: statements.into(),
        })
    }

    fn statement(&mut self, statement: &ast::Stmt<'_>) -> Result<StmtSyntax, ProjectionError> {
        let source = self.source(&statement_position(statement))?;
        let kind = match statement {
            ast::Stmt::EmptyStmt(_) => StmtSyntaxKind::Empty,
            ast::Stmt::BlockStmt(block) => StmtSyntaxKind::Block(self.block(block)?),
            ast::Stmt::ExprStmt(expression) => {
                StmtSyntaxKind::Expr(self.expression(&expression.x)?)
            }
            ast::Stmt::DeclStmt(declaration) => {
                StmtSyntaxKind::Decl(self.declaration(&declaration.decl)?)
            }
            ast::Stmt::AssignStmt(assignment) => StmtSyntaxKind::Assign {
                left: assignment
                    .lhs
                    .iter()
                    .map(|expression| self.expression(expression))
                    .collect::<Result<Vec<_>, _>>()?
                    .into(),
                token: assignment.tok,
                right: assignment
                    .rhs
                    .iter()
                    .map(|expression| self.expression(expression))
                    .collect::<Result<Vec<_>, _>>()?
                    .into(),
            },
            ast::Stmt::IncDecStmt(statement) => StmtSyntaxKind::IncDec {
                expression: self.expression(&statement.x)?,
                token: statement.tok,
            },
            ast::Stmt::ReturnStmt(statement) => StmtSyntaxKind::Return(
                statement
                    .results
                    .iter()
                    .map(|expression| self.expression(expression))
                    .collect::<Result<Vec<_>, _>>()?
                    .into(),
            ),
            ast::Stmt::IfStmt(statement) => StmtSyntaxKind::If {
                init: statement
                    .init
                    .as_ref()
                    .as_ref()
                    .map(|statement| self.statement(statement).map(Box::new))
                    .transpose()?,
                condition: self.expression(&statement.cond)?,
                then_block: self.block(&statement.body)?,
                else_branch: statement
                    .else_
                    .as_ref()
                    .as_ref()
                    .map(|statement| self.statement(statement).map(Box::new))
                    .transpose()?,
            },
            ast::Stmt::ForStmt(statement) => self.for_statement(statement, None)?,
            ast::Stmt::BranchStmt(statement) => StmtSyntaxKind::Branch {
                token: statement.tok,
                label: statement
                    .label
                    .as_ref()
                    .map(|label| self.ident(label))
                    .transpose()?,
            },
            ast::Stmt::CaseClause(_) => StmtSyntaxKind::Unsupported("case clause"),
            ast::Stmt::CommClause(_) => StmtSyntaxKind::Unsupported("communication clause"),
            ast::Stmt::DeferStmt(statement) => {
                let ast::Expr::FuncLit(function) = statement.call.fun.as_ref() else {
                    return Ok(StmtSyntax {
                        source,
                        kind: StmtSyntaxKind::Unsupported(
                            "defer call whose callee is not a function literal",
                        ),
                    });
                };
                StmtSyntaxKind::Defer {
                    has_type_parameters: function.type_.type_params.is_some(),
                    params: self.field_list(&function.type_.params)?,
                    results: function
                        .type_
                        .results
                        .as_ref()
                        .map(|fields| self.field_list(fields))
                        .transpose()?,
                    body: self.block(&function.body)?,
                    arguments: statement
                        .call
                        .args
                        .as_deref()
                        .unwrap_or_default()
                        .iter()
                        .map(|argument| self.expression(argument))
                        .collect::<Result<Vec<_>, _>>()?
                        .into(),
                    spread: statement.call.ellipsis.is_some(),
                }
            }
            ast::Stmt::GoStmt(_) => StmtSyntaxKind::Unsupported("go statement"),
            ast::Stmt::LabeledStmt(statement) => match statement.stmt.as_ref() {
                ast::Stmt::ForStmt(for_statement) => {
                    let label = self.ident(&statement.label)?;
                    self.for_statement(for_statement, Some(label))?
                }
                ast::Stmt::RangeStmt(range_statement) => {
                    let label = self.ident(&statement.label)?;
                    self.range_statement(range_statement, Some(label))?
                }
                statement_body => StmtSyntaxKind::Labeled {
                    label: self.ident(&statement.label)?,
                    statement: Box::new(self.statement(statement_body)?),
                },
            },
            ast::Stmt::RangeStmt(statement) => self.range_statement(statement, None)?,
            ast::Stmt::SelectStmt(_) => StmtSyntaxKind::Unsupported("select statement"),
            ast::Stmt::SendStmt(statement) => StmtSyntaxKind::Send {
                channel: self.expression(&statement.chan)?,
                value: self.expression(&statement.value)?,
            },
            ast::Stmt::SwitchStmt(statement) => self.switch_statement(statement)?,
            ast::Stmt::TypeSwitchStmt(_) => StmtSyntaxKind::Unsupported("type switch statement"),
        };
        Ok(StmtSyntax { source, kind })
    }

    fn for_statement(
        &mut self,
        statement: &ast::ForStmt<'_>,
        label: Option<IdentSyntax>,
    ) -> Result<StmtSyntaxKind, ProjectionError> {
        Ok(StmtSyntaxKind::For {
            label,
            init: statement
                .init
                .as_deref()
                .map(|statement| self.statement(statement).map(Box::new))
                .transpose()?,
            condition: statement
                .cond
                .as_ref()
                .map(|expression| self.expression(expression))
                .transpose()?,
            post: statement
                .post
                .as_deref()
                .map(|statement| self.statement(statement).map(Box::new))
                .transpose()?,
            body: self.block(&statement.body)?,
        })
    }

    fn range_statement(
        &mut self,
        statement: &ast::RangeStmt<'_>,
        label: Option<IdentSyntax>,
    ) -> Result<StmtSyntaxKind, ProjectionError> {
        Ok(StmtSyntaxKind::Range {
            label,
            key: statement
                .key
                .as_ref()
                .map(|expression| self.expression(expression))
                .transpose()?,
            value: statement
                .value
                .as_ref()
                .map(|expression| self.expression(expression))
                .transpose()?,
            token: statement.tok,
            expression: self.expression(&statement.x)?,
            body: self.block(&statement.body)?,
        })
    }

    fn switch_statement(
        &mut self,
        statement: &ast::SwitchStmt<'_>,
    ) -> Result<StmtSyntaxKind, ProjectionError> {
        let init = statement
            .init
            .as_deref()
            .map(|statement| self.statement(statement).map(Box::new))
            .transpose()?;
        let tag = statement
            .tag
            .as_ref()
            .map(|expression| self.expression(expression))
            .transpose()?;
        let mut cases = Vec::new();
        for statement in &statement.body.list {
            let ast::Stmt::CaseClause(case) = statement else {
                return Err(ProjectionError::InvalidSwitchBody);
            };
            let source = self.source(&case.case)?;
            let expressions = case
                .list
                .as_deref()
                .unwrap_or_default()
                .iter()
                .map(|expression| self.expression(expression))
                .collect::<Result<Vec<_>, _>>()?;
            let body_source = self.source(&case.colon)?;
            let body = case
                .body
                .iter()
                .map(|statement| self.statement(statement))
                .collect::<Result<Vec<_>, _>>()?;
            cases.push(SwitchCaseSyntax {
                source,
                expressions: expressions.into(),
                body: BlockSyntax {
                    source: body_source,
                    statements: body.into(),
                },
            });
        }
        Ok(StmtSyntaxKind::Switch {
            init,
            tag,
            cases: cases.into(),
        })
    }

    fn declaration(
        &mut self,
        declaration: &ast::GenDecl<'_>,
    ) -> Result<DeclSyntax, ProjectionError> {
        let source = self.source(&declaration.tok_pos)?;
        let mut contains_non_value_spec = false;
        let mut specs = Vec::new();
        for spec in &declaration.specs {
            match spec {
                ast::Spec::ValueSpec(spec) => specs.push(self.value_spec(spec)?),
                ast::Spec::ImportSpec(_) | ast::Spec::TypeSpec(_) => {
                    contains_non_value_spec = true;
                }
            }
        }
        Ok(DeclSyntax {
            source,
            token: declaration.tok,
            specs: specs.into(),
            contains_non_value_spec,
        })
    }

    fn value_spec(
        &mut self,
        spec: &ast::ValueSpec<'_>,
    ) -> Result<ValueSpecSyntax, ProjectionError> {
        Ok(ValueSpecSyntax {
            names: spec
                .names
                .iter()
                .map(|name| self.ident(name))
                .collect::<Result<Vec<_>, _>>()?
                .into(),
            explicit_type: spec
                .type_
                .as_ref()
                .map(|expression| self.expression(expression))
                .transpose()?,
            values: spec
                .values
                .as_ref()
                .map(|values| {
                    values
                        .iter()
                        .map(|expression| self.expression(expression))
                        .collect::<Result<Vec<_>, _>>()
                        .map(Arc::from)
                })
                .transpose()?,
        })
    }

    fn expression(&mut self, expression: &ast::Expr<'_>) -> Result<ExprSyntax, ProjectionError> {
        let source = self.source(&expression_position(expression))?;
        let kind = match expression {
            ast::Expr::Ident(ident) => ExprSyntaxKind::Ident(IdentSyntax {
                name: Arc::from(ident.name),
                source,
            }),
            ast::Expr::BasicLit(literal) => ExprSyntaxKind::Literal {
                token: literal.kind,
                spelling: Arc::from(literal.value),
            },
            ast::Expr::ParenExpr(expression) => {
                ExprSyntaxKind::Paren(Box::new(self.expression(&expression.x)?))
            }
            ast::Expr::UnaryExpr(expression) => ExprSyntaxKind::Unary {
                token: expression.op,
                expression: Box::new(self.expression(&expression.x)?),
            },
            ast::Expr::BinaryExpr(expression) => ExprSyntaxKind::Binary {
                left: Box::new(self.expression(&expression.x)?),
                token: expression.op,
                right: Box::new(self.expression(&expression.y)?),
            },
            ast::Expr::CallExpr(expression) => ExprSyntaxKind::Call {
                callee: Box::new(self.expression(&expression.fun)?),
                arguments: expression
                    .args
                    .as_deref()
                    .unwrap_or_default()
                    .iter()
                    .map(|argument| self.expression(argument))
                    .collect::<Result<Vec<_>, _>>()?
                    .into(),
                spread: expression.ellipsis.is_some(),
            },
            ast::Expr::SelectorExpr(expression) => ExprSyntaxKind::Selector {
                base: Box::new(self.expression(&expression.x)?),
                member: self.ident(&expression.sel)?,
            },
            ast::Expr::ArrayType(expression) => ExprSyntaxKind::ArrayType {
                length: expression
                    .len
                    .as_ref()
                    .map(|length| self.expression(length).map(Box::new))
                    .transpose()?,
                element: Box::new(self.expression(&expression.elt)?),
            },
            ast::Expr::ChanType(expression) => ExprSyntaxKind::ChannelType {
                direction: match expression.dir {
                    direction
                        if direction == (ast::ChanDir::SEND as u8 | ast::ChanDir::RECV as u8) =>
                    {
                        ChannelDirectionSyntax::SendReceive
                    }
                    direction if direction == ast::ChanDir::SEND as u8 => {
                        ChannelDirectionSyntax::SendOnly
                    }
                    direction if direction == ast::ChanDir::RECV as u8 => {
                        ChannelDirectionSyntax::ReceiveOnly
                    }
                    _ => return Err(ProjectionError::InvalidChannelDirection),
                },
                element: Box::new(self.expression(&expression.value)?),
            },
            ast::Expr::CompositeLit(expression) => ExprSyntaxKind::CompositeLiteral {
                ty: expression
                    .type_
                    .as_ref()
                    .map(|ty| self.expression(ty).map(Box::new))
                    .transpose()?,
                elements: expression
                    .elts
                    .as_deref()
                    .unwrap_or_default()
                    .iter()
                    .map(|element| self.expression(element))
                    .collect::<Result<Vec<_>, _>>()?
                    .into(),
            },
            ast::Expr::Ellipsis(_) => ExprSyntaxKind::Unsupported("ellipsis"),
            ast::Expr::FuncLit(function) => ExprSyntaxKind::FunctionLiteral {
                has_type_parameters: function.type_.type_params.is_some(),
                params: self.field_list(&function.type_.params)?,
                results: function
                    .type_
                    .results
                    .as_ref()
                    .map(|fields| self.field_list(fields))
                    .transpose()?,
                body: self.block(&function.body)?,
            },
            ast::Expr::FuncType(_) => ExprSyntaxKind::Unsupported("function type"),
            ast::Expr::IndexExpr(expression) => ExprSyntaxKind::Index {
                base: Box::new(self.expression(&expression.x)?),
                index: Box::new(self.expression(&expression.index)?),
            },
            ast::Expr::IndexListExpr(_) => ExprSyntaxKind::Unsupported("generic index expression"),
            ast::Expr::InterfaceType(_) => ExprSyntaxKind::Unsupported("interface type"),
            ast::Expr::KeyValueExpr(expression) => ExprSyntaxKind::KeyValue {
                key: Box::new(self.expression(&expression.key)?),
                value: Box::new(self.expression(&expression.value)?),
            },
            ast::Expr::MapType(expression) => ExprSyntaxKind::MapType {
                key: Box::new(self.expression(&expression.key)?),
                value: Box::new(self.expression(&expression.value)?),
            },
            ast::Expr::SliceExpr(expression) => ExprSyntaxKind::Slice {
                base: Box::new(self.expression(&expression.x)?),
                low: expression
                    .low
                    .as_ref()
                    .map(|bound| self.expression(bound).map(Box::new))
                    .transpose()?,
                high: expression
                    .high
                    .as_ref()
                    .map(|bound| self.expression(bound).map(Box::new))
                    .transpose()?,
                max: expression
                    .max
                    .as_ref()
                    .map(|bound| self.expression(bound).map(Box::new))
                    .transpose()?,
            },
            ast::Expr::StarExpr(expression) => ExprSyntaxKind::Unary {
                token: Token::MUL,
                expression: Box::new(self.expression(&expression.x)?),
            },
            ast::Expr::StructType(_) => ExprSyntaxKind::Unsupported("struct type"),
            ast::Expr::TypeAssertExpr(_) => ExprSyntaxKind::Unsupported("type assertion"),
        };
        Ok(ExprSyntax { source, kind })
    }
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

fn expression_position<'a>(expression: &'a ast::Expr<'a>) -> Position<'a> {
    match expression {
        ast::Expr::ArrayType(expression) => expression.lbrack,
        ast::Expr::BasicLit(expression) => expression.value_pos,
        ast::Expr::BinaryExpr(expression) => expression.op_pos,
        ast::Expr::CallExpr(expression) => expression.lparen,
        ast::Expr::ChanType(expression) => expression.begin,
        ast::Expr::CompositeLit(expression) => expression.lbrace,
        ast::Expr::Ellipsis(expression) => expression.ellipsis,
        ast::Expr::FuncLit(expression) => expression.type_.func.unwrap_or_default(),
        ast::Expr::FuncType(expression) => expression.func.unwrap_or_default(),
        ast::Expr::Ident(expression) => expression.name_pos,
        ast::Expr::IndexExpr(expression) => expression.lbrack,
        ast::Expr::IndexListExpr(expression) => expression.lbrack,
        ast::Expr::InterfaceType(expression) => expression.interface,
        ast::Expr::KeyValueExpr(expression) => expression.colon,
        ast::Expr::MapType(expression) => expression.map,
        ast::Expr::ParenExpr(expression) => expression.lparen,
        ast::Expr::SelectorExpr(expression) => expression.sel.name_pos,
        ast::Expr::SliceExpr(expression) => expression.lbrack,
        ast::Expr::StarExpr(expression) => expression.star,
        ast::Expr::StructType(expression) => expression.struct_,
        ast::Expr::TypeAssertExpr(expression) => expression.lparen,
        ast::Expr::UnaryExpr(expression) => expression.op_pos,
    }
}

fn statement_position<'a>(statement: &'a ast::Stmt<'a>) -> Position<'a> {
    match statement {
        ast::Stmt::AssignStmt(statement) => statement.tok_pos,
        ast::Stmt::BlockStmt(statement) => statement.lbrace,
        ast::Stmt::BranchStmt(statement) => statement.tok_pos,
        ast::Stmt::CaseClause(statement) => statement.case,
        ast::Stmt::CommClause(statement) => statement.case,
        ast::Stmt::DeclStmt(statement) => statement.decl.tok_pos,
        ast::Stmt::DeferStmt(statement) => statement.defer,
        ast::Stmt::EmptyStmt(statement) => statement.semicolon,
        ast::Stmt::ExprStmt(statement) => expression_position(&statement.x),
        ast::Stmt::ForStmt(statement) => statement.for_,
        ast::Stmt::GoStmt(statement) => statement.go,
        ast::Stmt::IfStmt(statement) => statement.if_,
        ast::Stmt::IncDecStmt(statement) => statement.tok_pos,
        ast::Stmt::LabeledStmt(statement) => statement.colon,
        ast::Stmt::RangeStmt(statement) => statement.for_,
        ast::Stmt::ReturnStmt(statement) => statement.return_,
        ast::Stmt::SelectStmt(statement) => statement.select,
        ast::Stmt::SendStmt(statement) => statement.arrow,
        ast::Stmt::SwitchStmt(statement) => statement.switch,
        ast::Stmt::TypeSwitchStmt(statement) => statement.switch,
    }
}
