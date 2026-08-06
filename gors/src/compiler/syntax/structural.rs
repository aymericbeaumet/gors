//! Owned structural syntax consumed by semantic lowering.

use std::sync::Arc;

use crate::token::Token;

/// Source-table region owning one dense structural source index.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SyntaxSourceRegion {
    Header,
    Body,
    Constant,
    Variable,
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
    pub(crate) variadic: bool,
    pub(crate) tag: Option<Arc<str>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FieldListSyntax {
    pub(crate) fields: Arc<[FieldSyntax]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FunctionHeaderSyntax {
    pub(crate) name: IdentSyntax,
    pub(crate) receiver: Option<FieldListSyntax>,
    pub(crate) has_type_parameters: bool,
    pub(crate) type_parameters: Option<FieldListSyntax>,
    pub(crate) params: FieldListSyntax,
    pub(crate) results: Option<FieldListSyntax>,
}

pub fn method_receiver(header: &FunctionHeaderSyntax) -> Option<(&str, bool)> {
    let receiver = header.receiver.as_ref()?;
    let [field] = receiver.fields.as_ref() else {
        return None;
    };
    let mut ty = field.ty.as_ref()?;
    let pointer = if let ExprSyntaxKind::Unary {
        token: Token::MUL,
        expression,
    } = &ty.kind
    {
        ty = expression;
        true
    } else {
        false
    };
    let receiver = match &ty.kind {
        ExprSyntaxKind::Ident(receiver) => receiver,
        ExprSyntaxKind::Index { base, .. } => {
            let ExprSyntaxKind::Ident(receiver) = &base.kind else {
                return None;
            };
            receiver
        }
        _ => return None,
    };
    Some((receiver.name.as_ref(), pointer))
}

pub fn function_is_generic(header: &FunctionHeaderSyntax) -> bool {
    if header.has_type_parameters {
        return true;
    }
    let Some(receiver) = &header.receiver else {
        return false;
    };
    let [field] = receiver.fields.as_ref() else {
        return false;
    };
    let Some(mut ty) = field.ty.as_ref() else {
        return false;
    };
    if let ExprSyntaxKind::Unary {
        token: Token::MUL,
        expression,
    } = &ty.kind
    {
        ty = expression;
    }
    matches!(ty.kind, ExprSyntaxKind::Index { .. })
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
    pub(crate) iota: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VariableSyntax {
    pub(crate) name: IdentSyntax,
    pub(crate) explicit_type: Option<ExprSyntax>,
    pub(crate) value: VariableValueSyntax,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeAliasSyntax {
    pub(crate) name: IdentSyntax,
    pub(crate) type_parameters: Option<FieldListSyntax>,
    pub(crate) target: ExprSyntax,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeDefinitionSyntax {
    pub(crate) name: IdentSyntax,
    pub(crate) type_parameters: Option<FieldListSyntax>,
    pub(crate) underlying: ExprSyntax,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConstantValueSyntax {
    Expression(ExprSyntax),
    ImplicitOrIota,
    ArityMismatch,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VariableValueSyntax {
    Expression(ExprSyntax),
    Zero,
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
    Send {
        channel: ExprSyntax,
        value: ExprSyntax,
    },
    Defer {
        has_type_parameters: bool,
        params: FieldListSyntax,
        results: Option<FieldListSyntax>,
        body: BlockSyntax,
        arguments: Arc<[ExprSyntax]>,
        spread: bool,
    },
    Go {
        has_type_parameters: bool,
        params: FieldListSyntax,
        results: Option<FieldListSyntax>,
        body: BlockSyntax,
        arguments: Arc<[ExprSyntax]>,
        spread: bool,
    },
    Return(Arc<[ExprSyntax]>),
    If {
        init: Option<Box<StmtSyntax>>,
        condition: ExprSyntax,
        then_block: BlockSyntax,
        else_branch: Option<Box<StmtSyntax>>,
    },
    For {
        label: Option<IdentSyntax>,
        init: Option<Box<StmtSyntax>>,
        condition: Option<ExprSyntax>,
        post: Option<Box<StmtSyntax>>,
        body: BlockSyntax,
    },
    Range {
        label: Option<IdentSyntax>,
        key: Option<ExprSyntax>,
        value: Option<ExprSyntax>,
        token: Option<Token>,
        expression: ExprSyntax,
        body: BlockSyntax,
    },
    Switch {
        init: Option<Box<StmtSyntax>>,
        tag: Option<ExprSyntax>,
        cases: Arc<[SwitchCaseSyntax]>,
    },
    TypeSwitch {
        init: Option<Box<StmtSyntax>>,
        binding: Option<IdentSyntax>,
        expression: ExprSyntax,
        cases: Arc<[SwitchCaseSyntax]>,
    },
    Select {
        cases: Arc<[SelectCaseSyntax]>,
    },
    Labeled {
        label: IdentSyntax,
        statement: Box<StmtSyntax>,
    },
    Branch {
        token: Token,
        label: Option<IdentSyntax>,
    },
    Unsupported(&'static str),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SwitchCaseSyntax {
    pub(crate) source: SyntaxSource,
    pub(crate) expressions: Arc<[ExprSyntax]>,
    pub(crate) body: BlockSyntax,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectCaseSyntax {
    pub(crate) source: SyntaxSource,
    pub(crate) communication: Option<Box<StmtSyntax>>,
    pub(crate) body: BlockSyntax,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeclSyntax {
    pub(crate) source: SyntaxSource,
    pub(crate) token: Token,
    pub(crate) specs: Arc<[ValueSpecSyntax]>,
    pub(crate) type_specs: Arc<[LocalTypeSyntax]>,
    pub(crate) contains_import_spec: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValueSpecSyntax {
    pub(crate) names: Arc<[IdentSyntax]>,
    pub(crate) explicit_type: Option<ExprSyntax>,
    pub(crate) values: Option<Arc<[ExprSyntax]>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalTypeSyntax {
    pub(crate) name: IdentSyntax,
    pub(crate) alias: bool,
    pub(crate) has_type_parameters: bool,
    pub(crate) target: ExprSyntax,
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
        spread: bool,
    },
    FunctionLiteral {
        has_type_parameters: bool,
        params: FieldListSyntax,
        results: Option<FieldListSyntax>,
        body: BlockSyntax,
    },
    FunctionType {
        has_type_parameters: bool,
        params: FieldListSyntax,
        results: Option<FieldListSyntax>,
    },
    Selector {
        base: Box<ExprSyntax>,
        member: IdentSyntax,
    },
    TypeAssert {
        value: Box<ExprSyntax>,
        asserted: Option<Box<ExprSyntax>>,
    },
    ArrayType {
        length: Option<Box<ExprSyntax>>,
        element: Box<ExprSyntax>,
    },
    MapType {
        key: Box<ExprSyntax>,
        value: Box<ExprSyntax>,
    },
    ChannelType {
        direction: ChannelDirectionSyntax,
        element: Box<ExprSyntax>,
    },
    StructType {
        fields: FieldListSyntax,
    },
    InterfaceType {
        methods: FieldListSyntax,
    },
    KeyValue {
        key: Box<ExprSyntax>,
        value: Box<ExprSyntax>,
    },
    CompositeLiteral {
        ty: Option<Box<ExprSyntax>>,
        elements: Arc<[ExprSyntax]>,
    },
    Index {
        base: Box<ExprSyntax>,
        index: Box<ExprSyntax>,
    },
    Slice {
        base: Box<ExprSyntax>,
        low: Option<Box<ExprSyntax>>,
        high: Option<Box<ExprSyntax>>,
        max: Option<Box<ExprSyntax>>,
    },
    Unsupported(&'static str),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChannelDirectionSyntax {
    SendReceive,
    SendOnly,
    ReceiveOnly,
}
