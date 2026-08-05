//! One-pass projection from parser observations into owned function syntax.

use std::fmt;
use std::sync::Arc;

use crate::ast;
use crate::parser::TokenObservation;
use crate::source::{TextRange, TextSize};
use crate::token::{Position, Token};

use super::{
    BlockSyntax, ConstantLayout, ConstantSyntax, ConstantValueSyntax, DeclSyntax, ExprSyntax,
    ExprSyntaxKind, FieldListSyntax, FieldSyntax, FunctionBodySyntax, FunctionHeaderSyntax,
    FunctionLayout, IdentSyntax, SemanticTokenStream, StmtSyntax, StmtSyntaxKind, SyntaxAnchor,
    SyntaxSource, SyntaxSourceRegion, ValueSpecSyntax,
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
        },
        layout: ConstantLayout::new(declaration, source_len, projector.finish()),
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
                    ty: field
                        .type_
                        .as_ref()
                        .map(|expression| self.expression(expression))
                        .transpose()?,
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
            ast::Stmt::DeferStmt(_) => StmtSyntaxKind::Unsupported("defer statement"),
            ast::Stmt::GoStmt(_) => StmtSyntaxKind::Unsupported("go statement"),
            ast::Stmt::LabeledStmt(statement) => match statement.stmt.as_ref() {
                ast::Stmt::ForStmt(for_statement) => {
                    let label = self.ident(&statement.label)?;
                    self.for_statement(for_statement, Some(label))?
                }
                _ => StmtSyntaxKind::Unsupported("label on a non-for statement"),
            },
            ast::Stmt::RangeStmt(_) => StmtSyntaxKind::Unsupported("range statement"),
            ast::Stmt::SelectStmt(_) => StmtSyntaxKind::Unsupported("select statement"),
            ast::Stmt::SendStmt(_) => StmtSyntaxKind::Unsupported("send statement"),
            ast::Stmt::SwitchStmt(_) => StmtSyntaxKind::Unsupported("switch statement"),
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
            },
            ast::Expr::SelectorExpr(expression) => ExprSyntaxKind::Selector {
                base: Box::new(self.expression(&expression.x)?),
                member: self.ident(&expression.sel)?,
            },
            ast::Expr::ArrayType(_) => ExprSyntaxKind::Unsupported("array or slice type"),
            ast::Expr::ChanType(_) => ExprSyntaxKind::Unsupported("channel type"),
            ast::Expr::CompositeLit(_) => ExprSyntaxKind::Unsupported("composite literal"),
            ast::Expr::Ellipsis(_) => ExprSyntaxKind::Unsupported("ellipsis"),
            ast::Expr::FuncLit(_) => ExprSyntaxKind::Unsupported("function literal"),
            ast::Expr::FuncType(_) => ExprSyntaxKind::Unsupported("function type"),
            ast::Expr::IndexExpr(_) => ExprSyntaxKind::Unsupported("index expression"),
            ast::Expr::IndexListExpr(_) => ExprSyntaxKind::Unsupported("generic index expression"),
            ast::Expr::InterfaceType(_) => ExprSyntaxKind::Unsupported("interface type"),
            ast::Expr::KeyValueExpr(_) => ExprSyntaxKind::Unsupported("key-value expression"),
            ast::Expr::MapType(_) => ExprSyntaxKind::Unsupported("map type"),
            ast::Expr::SliceExpr(_) => ExprSyntaxKind::Unsupported("slice expression"),
            ast::Expr::StarExpr(_) => ExprSyntaxKind::Unsupported("pointer expression"),
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
