//! Typed, source-shaped high-level IR.

use super::ids::{ClosureId, DefId, LocalId, NodeId, PackageId, QualifiedDefId};
use super::provenance::SourceRef;
use super::types::{ConstValue, Signature, StaticValue, Ty};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct File {
    pub package_id: PackageId,
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
pub struct Variable {
    pub id: DefId,
    pub name: String,
    pub ty: Ty,
    pub value: StaticValue,
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
        coercions: Vec<ValueCoercion>,
    },
    Assign {
        destinations: Vec<Place>,
        op: AssignOp,
        values: Vec<Expr>,
    },
    AssignTuple {
        destinations: Vec<Place>,
        value: Expr,
        coercions: Vec<ValueCoercion>,
    },
    /// Dynamic left-hand-side operands are evaluated before the tuple-valued
    /// right-hand side, then all result coercions are applied before writes.
    ParallelAssignTuple {
        destinations: Vec<AssignTarget>,
        value: Expr,
        coercions: Vec<ValueCoercion>,
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
    Go {
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
        set: Builtin,
        op: AssignOp,
        value: Expr,
    },
    ArrayAssign {
        array: LocalId,
        index: Expr,
        op: AssignOp,
        value: Expr,
    },
    StructFieldAssign {
        structure: LocalId,
        field: u32,
        op: AssignOp,
        value: Expr,
    },
    /// The map and key operands are evaluated exactly once; a compound
    /// operation reads the current element (a missing key yields the zero
    /// value) before the single write.
    MapAssign {
        map: Expr,
        key: Expr,
        op: AssignOp,
        value: Expr,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ValueCoercion {
    Identity,
    Representation { target: Ty },
    Interface { target: Ty, type_identity: Vec<u8> },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AssignTarget {
    Local(LocalId),
    Discard,
    SliceIndex {
        slice: Expr,
        index: Expr,
        set: Builtin,
    },
    MapIndex {
        map: Expr,
        key: Expr,
    },
    Pointer {
        pointer: Expr,
        set: Builtin,
    },
    StructField {
        structure: LocalId,
        field: u32,
    },
    PointerStructField {
        pointer: Expr,
        field: u32,
        set: Builtin,
    },
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
    AddressOfLocal(LocalId),
    AddressOfValue(Box<Expr>),
    PointerStructValue(Box<Expr>),
    InterfaceValue {
        value: Box<Expr>,
        type_identity: Vec<u8>,
    },
    InterfaceCall {
        receiver: Box<Expr>,
        args: Vec<Expr>,
        candidates: Vec<InterfaceCallCandidate>,
    },
    GlobalConstant(QualifiedDefId, ConstValue),
    GlobalVariable(QualifiedDefId, StaticValue),
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
    /// Return and consume the active panic value for a directly deferred call.
    Recover,
    SliceLiteralI64(Vec<i64>),
    DynamicSliceLiteralI64(Vec<Expr>),
    AggregateSliceLiteral {
        elements: Vec<Expr>,
        type_identity: Vec<u8>,
    },
    AggregateSliceIndex {
        slice: Box<Expr>,
        index: Box<Expr>,
        type_identity: Vec<u8>,
    },
    SliceLiteralU8(Vec<u8>),
    SliceLiteralBool(Vec<bool>),
    SliceLiteralGoString(Vec<Expr>),
    ArrayLiteralI64(Vec<i64>),
    ArrayLiteral(Vec<(u64, Expr)>),
    ArrayIndexI64 {
        array: Box<Expr>,
        index: Box<Expr>,
    },
    ArrayIndex {
        array: Box<Expr>,
        index: Box<Expr>,
    },
    ArrayLen {
        array: Box<Expr>,
        length: u64,
    },
    StructLiteral(Vec<Expr>),
    StructField {
        structure: Box<Expr>,
        field: u32,
    },
    MapLiteralStringI64(Vec<(Expr, Expr)>),
    MapLiteralI64GoString(Vec<(Expr, Expr)>),
    AggregateMapLiteral {
        entries: Vec<(Expr, Expr)>,
        type_identity: Vec<u8>,
    },
    AggregateMapIndex {
        map: Box<Expr>,
        key: Box<Expr>,
        type_identity: Vec<u8>,
    },
    Call {
        callee: Callee,
        args: Vec<Expr>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InterfaceCallCandidate {
    pub type_identity: Vec<u8>,
    pub dynamic_ty: Ty,
    pub receiver_ty: Ty,
    pub function: QualifiedDefId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Callee {
    Function(QualifiedDefId),
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
    SliceI64Nil,
    SliceI64IsNil,
    SliceI64Len,
    SliceI64Cap,
    SliceI64Append,
    SliceU8Make,
    SliceU8Set,
    SliceU8AppendSlice,
    SliceU8AppendString,
    SliceU8Copy,
    SliceU8CopyString,
    SliceU8Len,
    SliceU8Index,
    SliceU8Range,
    SliceU8Nil,
    SliceU8IsNil,
    SliceI64Copy,
    SliceI64Clear,
    SliceBoolIndex,
    SliceBoolSet,
    SliceBoolNil,
    SliceBoolIsNil,
    SliceGoStringIndex,
    SliceGoStringRange,
    SliceGoStringSet,
    SliceGoStringMake,
    SliceGoStringNil,
    SliceGoStringIsNil,
    SliceGoStringLen,
    SliceGoStringCap,
    SliceGoStringAppend,
    SliceGoStringCopy,
    SliceGoStringClear,
    AggregateSliceMake,
    AggregateSliceNil,
    AggregateSliceIsNil,
    AggregateSliceLen,
    AggregateSliceIndexTagged,
    AggregateSliceSetTagged,
    /// A function value proven to return one captured per-iteration integer.
    /// Rust representation lowering stores only that immutable payload.
    SnapshotFunctionSliceAppend,
    /// Invoke one proven capture-only function directly from its slice.
    SnapshotFunctionSliceCall,
    StringFromRune,
    StringFromSliceU8,
    StringFromSliceRunes,
    StringToSliceRunes,
    StringLen,
    StringIndex,
    StringRange,
    StringRangeCount,
    StringRangeIndexAt,
    StringRangeRuneAt,
    MapStringI64Nil,
    MapStringI64Make,
    MapStringI64Len,
    MapStringI64Get,
    /// HIR-only comma-ok lookup expanded into one `get` and one `contains`
    /// call during explicit-order MIR construction.
    MapStringI64Lookup,
    MapStringI64Contains,
    MapStringI64Set,
    MapStringI64Delete,
    MapStringI64Clear,
    MapStringI64IsNil,
    MapStringI64KeyAt,
    MapStringI64RangeKeys,
    MapI64GoStringNil,
    MapI64GoStringMake,
    MapI64GoStringLen,
    MapI64GoStringGet,
    /// HIR-only comma-ok lookup expanded during MIR construction.
    MapI64GoStringLookup,
    MapI64GoStringContains,
    MapI64GoStringSet,
    MapI64GoStringDelete,
    MapI64GoStringClear,
    MapI64GoStringIsNil,
    MapI64GoStringRangeKeys,
    AggregateMapMake,
    AggregateMapLen,
    AggregateMapGetTagged,
    AggregateMapContains,
    AggregateMapSetTagged,
    PointerI64Nil,
    PointerI64New,
    PointerI64Get,
    PointerI64Set,
    PointerI64IsNil,
    PointerStructI64Nil,
    PointerStructI64New,
    PointerStructI64Get,
    PointerStructI64Set,
    PointerStructI64IsNil,
    PointerStructI64Equal,
    AggregatePointerNil,
    AggregatePointerNew,
    AggregatePointerSnapshot,
    AggregatePointerIsNil,
    InterfaceNil,
    InterfaceBoxBool,
    InterfaceBoxI64,
    InterfaceBoxF64,
    InterfaceBoxGoString,
    InterfaceBoxGoSliceGoString,
    InterfaceBoxStructI64,
    InterfaceBoxPointerStructI64,
    InterfaceBoxAggregate,
    InterfaceBoxComparableAggregate,
    InterfaceIsNil,
    InterfaceIsType,
    InterfaceIsRuntimeError,
    InterfaceEqual,
    /// HIR-only type assertion expanded into explicit interface tests and
    /// extraction calls during MIR construction.
    InterfaceAssert,
    /// HIR-only interface-satisfaction assertion expanded against the
    /// package's executable dynamic-type set during MIR construction.
    InterfaceSatisfies,
    /// HIR-only interface-satisfaction assertion that also admits the
    /// implementation-provided dynamic type used for runtime panics.
    InterfaceSatisfiesRuntimeError,
    /// HIR-only interface conversion whose source method set statically
    /// implies the target method set. It succeeds for every non-nil dynamic
    /// value and is expanded into an explicit nil test during MIR construction.
    InterfaceSatisfiesNonNil,
    InterfaceUnboxBool,
    InterfaceUnboxI64,
    InterfaceUnboxF64,
    InterfaceUnboxGoString,
    InterfaceUnboxGoSliceGoString,
    InterfaceStructI64Get,
    InterfaceUnboxPointerStructI64,
    InterfaceUnboxAggregate,
    FunctionNil,
    FunctionIsNil,
    ChannelI64Nil,
    ChannelI64Make,
    ChannelI64Len,
    ChannelI64Cap,
    ChannelI64Send,
    ChannelI64ReceiveValue,
    ChannelI64Receive,
    ChannelI64Close,
    ChannelI64IsNil,
    ChannelI64TrySend,
    ChannelI64TryReceive,
    ChannelGoStringNil,
    ChannelGoStringMake,
    ChannelGoStringLen,
    ChannelGoStringCap,
    ChannelGoStringSend,
    ChannelGoStringReceiveValue,
    ChannelGoStringReceive,
    ChannelGoStringClose,
    ChannelGoStringIsNil,
    ChannelGoStringTrySend,
    ChannelGoStringTryReceive,
    ChannelGoChannelI64Nil,
    ChannelGoChannelI64Make,
    ChannelGoChannelI64Len,
    ChannelGoChannelI64Cap,
    ChannelGoChannelI64Send,
    ChannelGoChannelI64ReceiveValue,
    ChannelGoChannelI64Receive,
    ChannelGoChannelI64Close,
    ChannelGoChannelI64IsNil,
    ChannelGoChannelI64TrySend,
    ChannelGoChannelI64TryReceive,
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
    Min,
    Max,
    Complex,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnaryOp {
    Positive,
    Negative,
    Not,
    BitNot,
    Real,
    Imag,
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
