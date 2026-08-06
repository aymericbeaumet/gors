//! Typed, source-shaped high-level IR.

use super::ids::{ClosureId, DefId, LocalId, NodeId};
use super::provenance::SourceRef;
use super::types::{ConstValue, Signature, Ty};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct File {
    pub package: String,
    pub constants: Vec<Constant>,
    pub functions: Vec<Function>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Constant {
    pub id: DefId,
    pub name: String,
    pub ty: Ty,
    pub value: ConstValue,
    pub source: SourceRef,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Function {
    pub id: DefId,
    pub node: NodeId,
    pub name: String,
    pub signature: Signature,
    pub params: Vec<LocalId>,
    pub named_results: Vec<Option<LocalId>>,
    pub locals: Vec<Local>,
    pub closures: Vec<Closure>,
    pub body: Block,
    pub source: SourceRef,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Closure {
    pub id: ClosureId,
    pub signature: Signature,
    pub params: Vec<LocalId>,
    pub named_results: Vec<Option<LocalId>>,
    pub body: Block,
    pub source: SourceRef,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Local {
    pub id: LocalId,
    pub name: Option<String>,
    pub ty: Ty,
    pub kind: LocalKind,
    pub source: SourceRef,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalKind {
    Parameter,
    NamedResult,
    Variable,
    Temporary,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Block {
    pub node: NodeId,
    pub stmts: Vec<Stmt>,
    pub source: SourceRef,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Stmt {
    pub node: NodeId,
    pub kind: StmtKind,
    pub source: SourceRef,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StmtKind {
    /// All initializers are evaluated before any destination is written.
    Let {
        destinations: Vec<Place>,
        values: Vec<Expr>,
    },
    LetTuple {
        destinations: Vec<Place>,
        value: Expr,
    },
    Assign {
        destinations: Vec<Place>,
        op: AssignOp,
        values: Vec<Expr>,
    },
    AssignTuple {
        destinations: Vec<Place>,
        value: Expr,
    },
    /// Dynamic left-hand-side operands are evaluated before all right-hand
    /// sides, then destinations are written from left to right.
    ParallelAssign {
        destinations: Vec<AssignTarget>,
        values: Vec<Expr>,
    },
    Expr(Expr),
    ClosureBinding(ClosureId),
    Defer {
        parameters: Vec<LocalId>,
        values: Vec<Expr>,
        body: Block,
    },
    Return(Vec<Expr>),
    If {
        init: Option<Box<Stmt>>,
        condition: Expr,
        then_block: Block,
        else_branch: Option<Box<Stmt>>,
    },
    For {
        label: Option<String>,
        init: Option<Box<Stmt>>,
        condition: Option<Expr>,
        post: Option<Box<Stmt>>,
        body: Block,
    },
    Range {
        label: Option<String>,
        key: Option<Place>,
        value: Option<Place>,
        expression: Expr,
        body: Block,
    },
    Block(Block),
    Label {
        name: String,
        statement: Option<Box<Stmt>>,
    },
    Goto(String),
    Break(Option<String>),
    Continue(Option<String>),
    SliceAssign {
        slice: Expr,
        index: Expr,
        op: AssignOp,
        value: Expr,
    },
    MapAssign {
        map: Expr,
        key: Expr,
        value: Expr,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AssignTarget {
    Local(LocalId),
    Discard,
    SliceIndex { slice: Expr, index: Expr },
    MapIndex { map: Expr, key: Expr },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssignOp {
    Set,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    AndNot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Place {
    Local(LocalId),
    Discard,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Expr {
    pub node: NodeId,
    pub kind: ExprKind,
    pub ty: Ty,
    pub category: ValueCategory,
    pub effects: Effects,
    pub source: SourceRef,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExprKind {
    Constant(ConstValue),
    Local(LocalId),
    GlobalConstant(DefId, ConstValue),
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
    },
    Conversion {
        value: Box<Expr>,
    },
    /// Compare the result of a direct `recover()` call with `nil` while
    /// consuming the active panic when one exists.
    RecoverCompareNil {
        equal: bool,
    },
    SliceLiteralI64(Vec<i64>),
    SliceLiteralU8(Vec<u8>),
    MapLiteralStringI64(Vec<(Expr, Expr)>),
    Call {
        callee: Callee,
        args: Vec<Expr>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Callee {
    Function(DefId),
    Closure(ClosureId),
    Builtin(Builtin),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Builtin {
    Print,
    Println,
    Panic,
    SliceI64Index,
    SliceI64Range,
    SliceI64Set,
    SliceI64Make,
    SliceI64Len,
    SliceI64Cap,
    SliceI64Append,
    SliceU8AppendSlice,
    SliceU8AppendString,
    SliceU8CopyString,
    SliceI64Copy,
    SliceI64Clear,
    StringFromSliceU8,
    MapStringI64Nil,
    MapStringI64Make,
    MapStringI64Len,
    MapStringI64Get,
    MapStringI64Set,
    MapStringI64Delete,
    MapStringI64Clear,
    MapStringI64IsNil,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    AndNot,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    LogicalAnd,
    LogicalOr,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnaryOp {
    Positive,
    Negative,
    Not,
    BitNot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValueCategory {
    Value,
    Place,
    Constant,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Effects {
    pub may_read: bool,
    pub may_call: bool,
    pub may_allocate: bool,
    pub may_block: bool,
    pub may_panic: bool,
    pub may_write: bool,
}

impl Effects {
    pub fn union(self, other: Self) -> Self {
        Self {
            may_read: self.may_read || other.may_read,
            may_call: self.may_call || other.may_call,
            may_allocate: self.may_allocate || other.may_allocate,
            may_block: self.may_block || other.may_block,
            may_panic: self.may_panic || other.may_panic,
            may_write: self.may_write || other.may_write,
        }
    }
}
