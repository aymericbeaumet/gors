//! Owned structural syntax consumed by semantic lowering.

use std::sync::Arc;

use crate::token::Token;

/// Source-table region owning one dense structural source index.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SyntaxSourceRegion {
    Header,
    Body,
    Constant,
}

/// Trivia-independent source identity inside one owned declaration.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SyntaxSource {
    region: SyntaxSourceRegion,
    index: u32,
}

impl SyntaxSource {
    pub(super) const fn new(region: SyntaxSourceRegion, index: u32) -> Self {
        Self { region, index }
    }

    #[must_use]
    pub const fn region(self) -> SyntaxSourceRegion {
        self.region
    }

    #[must_use]
    pub const fn index(self) -> u32 {
        self.index
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IdentSyntax {
    pub(crate) name: Arc<str>,
    pub(crate) source: SyntaxSource,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FieldSyntax {
    pub(crate) names: Option<Arc<[IdentSyntax]>>,
    pub(crate) ty: Option<ExprSyntax>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FieldListSyntax {
    pub(crate) fields: Arc<[FieldSyntax]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FunctionHeaderSyntax {
    pub(crate) name: IdentSyntax,
    pub(crate) has_receiver: bool,
    pub(crate) has_type_parameters: bool,
    pub(crate) params: FieldListSyntax,
    pub(crate) results: Option<FieldListSyntax>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FunctionBodySyntax {
    pub(crate) block: Option<BlockSyntax>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConstantSyntax {
    pub(crate) name: IdentSyntax,
    pub(crate) explicit_type: Option<ExprSyntax>,
    pub(crate) value: ConstantValueSyntax,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConstantValueSyntax {
    Expression(ExprSyntax),
    ImplicitOrIota,
    ArityMismatch,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockSyntax {
    pub(crate) source: SyntaxSource,
    pub(crate) statements: Arc<[StmtSyntax]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StmtSyntax {
    pub(crate) source: SyntaxSource,
    pub(crate) kind: StmtSyntaxKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StmtSyntaxKind {
    Empty,
    Block(BlockSyntax),
    Expr(ExprSyntax),
    Decl(DeclSyntax),
    Assign {
        left: Arc<[ExprSyntax]>,
        token: Token,
        right: Arc<[ExprSyntax]>,
    },
    IncDec {
        expression: ExprSyntax,
        token: Token,
    },
    Return(Arc<[ExprSyntax]>),
    If {
        init: Option<Box<StmtSyntax>>,
        condition: ExprSyntax,
        then_block: BlockSyntax,
        else_branch: Option<Box<StmtSyntax>>,
    },
    For {
        init: Option<Box<StmtSyntax>>,
        condition: Option<ExprSyntax>,
        post: Option<Box<StmtSyntax>>,
        body: BlockSyntax,
    },
    Branch {
        token: Token,
        has_label: bool,
    },
    Unsupported(&'static str),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeclSyntax {
    pub(crate) source: SyntaxSource,
    pub(crate) token: Token,
    pub(crate) specs: Arc<[ValueSpecSyntax]>,
    pub(crate) contains_non_value_spec: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValueSpecSyntax {
    pub(crate) names: Arc<[IdentSyntax]>,
    pub(crate) explicit_type: Option<ExprSyntax>,
    pub(crate) values: Option<Arc<[ExprSyntax]>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExprSyntax {
    pub(crate) source: SyntaxSource,
    pub(crate) kind: ExprSyntaxKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExprSyntaxKind {
    Ident(IdentSyntax),
    Literal {
        token: Token,
        spelling: Arc<str>,
    },
    Paren(Box<ExprSyntax>),
    Unary {
        token: Token,
        expression: Box<ExprSyntax>,
    },
    Binary {
        left: Box<ExprSyntax>,
        token: Token,
        right: Box<ExprSyntax>,
    },
    Call {
        callee: Box<ExprSyntax>,
        arguments: Arc<[ExprSyntax]>,
    },
    Selector {
        base: Box<ExprSyntax>,
        member: IdentSyntax,
    },
    Unsupported(&'static str),
}
