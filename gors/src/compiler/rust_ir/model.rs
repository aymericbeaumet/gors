//! Explicit Rust representation IR consumed mechanically by syntax emission.

use crate::compiler::ids::{BasicBlockId, DefId, LocalId, SourceSpan};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct File {
    pub package: String,
    pub functions: Vec<Function>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Function {
    pub id: DefId,
    pub name: String,
    pub artifact: FunctionArtifactPlan,
    pub signature: Signature,
    pub parameters: Vec<LocalId>,
    pub locals: Vec<LocalDecl>,
    pub blocks: Vec<BasicBlock>,
    pub entry: BasicBlockId,
    pub control_flow: ControlFlowPlan,
    pub span: SourceSpan,
}

/// Final Rust artifact decisions selected before terminal syntax emission.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FunctionArtifactPlan {
    pub symbol: RustSymbol,
    pub linkage: RustLinkage,
    pub entrypoint: EntrypointPlan,
}

impl FunctionArtifactPlan {
    pub(in crate::compiler) fn public_definition(id: DefId) -> Self {
        Self {
            symbol: RustSymbol {
                spelling: format!("__gors_fn_{id}"),
            },
            linkage: RustLinkage::Public,
            entrypoint: EntrypointPlan::None,
        }
    }

    pub(in crate::compiler) fn executable_entrypoint() -> Self {
        Self {
            symbol: RustSymbol {
                spelling: "main".to_owned(),
            },
            linkage: RustLinkage::Internal,
            entrypoint: EntrypointPlan::Executable,
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct RustSymbol {
    pub spelling: String,
}

impl RustSymbol {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.spelling
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RustLinkage {
    Internal,
    Public,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntrypointPlan {
    None,
    Executable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Signature {
    pub params: Vec<RustType>,
    pub results: Vec<RustType>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalDecl {
    pub id: LocalId,
    pub name: Option<String>,
    pub ty: RustType,
    pub storage: StorageClass,
    pub initialization: SlotInitialization,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageClass {
    CheckedOptionSlot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SlotInitialization {
    Parameter(usize),
    Uninitialized,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlFlowPlan {
    PcDispatchU32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BasicBlock {
    pub id: BasicBlockId,
    pub provenance: Provenance,
    pub statements: Vec<Statement>,
    pub terminator: Terminator,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Statement {
    pub destination: Place,
    pub value: Rvalue,
    pub store: StoreOp,
    pub effects: Effects,
    pub provenance: Provenance,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoreOp {
    SetSome,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Place {
    pub local: LocalId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Rvalue {
    pub kind: RvalueKind,
    pub effects: Effects,
    pub panic: PanicEdge,
    pub provenance: Provenance,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RvalueKind {
    Use(Operand),
    Unary {
        op: UnaryOp,
        operand: Operand,
    },
    Binary {
        op: BinaryOp,
        left: Operand,
        right: Operand,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnaryOp {
    Identity,
    IntNeg,
    BoolNot,
    IntBitNot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BinaryOp {
    IntAdd,
    IntSub,
    IntMul,
    IntDiv,
    IntRem,
    IntBitAnd,
    IntBitOr,
    IntBitXor,
    IntShl,
    IntShr,
    IntAndNot,
    BoolEqual,
    BoolNotEqual,
    IntEqual,
    IntNotEqual,
    IntLess,
    IntLessEqual,
    IntGreater,
    IntGreaterEqual,
    StringConcat,
    StringEqual,
    StringNotEqual,
    StringLess,
    StringLessEqual,
    StringGreater,
    StringGreaterEqual,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Operand {
    Read { place: Place, op: ReadOp },
    Constant(Constant),
    Unit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadOp {
    /// Copy from a slot proven initialized by Rust-IR dataflow verification.
    ProvenInitializedCopy,
    /// Clone from a slot proven initialized by Rust-IR dataflow verification.
    ProvenInitializedClone,
    /// Move from an initialized owned slot proven dead on every continuation.
    ProvenLastUseMove,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Constant {
    Bool(bool),
    I64(i64),
    GoString(Vec<u8>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Terminator {
    pub kind: TerminatorKind,
    pub effects: Effects,
    pub panic: PanicEdge,
    pub provenance: Provenance,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TerminatorKind {
    Goto(BasicBlockId),
    SwitchBool {
        condition: Operand,
        then_target: BasicBlockId,
        else_target: BasicBlockId,
    },
    Call {
        target: CallTarget,
        args: Vec<Operand>,
        destination: Option<Place>,
        next: BasicBlockId,
    },
    Return(Vec<Operand>),
    Unreachable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CallTarget {
    Function(DefId),
    RuntimePrint { steps: Vec<PrintStep> },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrintStep {
    PrintEmpty,
    PrintSpace,
    PrintNewline,
    PrintBool { argument: usize },
    PrintI64 { argument: usize },
    PrintGoString { argument: usize },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RustType {
    Unit,
    Bool,
    I64,
    GoString,
}

impl RustType {
    #[must_use]
    pub fn conservative_read_op(self) -> Option<ReadOp> {
        match self {
            Self::Bool | Self::I64 => Some(ReadOp::ProvenInitializedCopy),
            Self::GoString => Some(ReadOp::ProvenInitializedClone),
            Self::Unit => None,
        }
    }

    pub(super) fn read_op_for_liveness(self, live_after: bool) -> Option<ReadOp> {
        match self {
            Self::Bool | Self::I64 => Some(ReadOp::ProvenInitializedCopy),
            Self::GoString if live_after => Some(ReadOp::ProvenInitializedClone),
            Self::GoString => Some(ReadOp::ProvenLastUseMove),
            Self::Unit => None,
        }
    }

    pub(super) fn supports_read_op(self, op: ReadOp) -> bool {
        matches!(
            (self, op),
            (Self::Bool | Self::I64, ReadOp::ProvenInitializedCopy)
                | (
                    Self::GoString,
                    ReadOp::ProvenInitializedClone | ReadOp::ProvenLastUseMove
                )
        )
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Effects {
    pub may_read: bool,
    pub may_write: bool,
    pub may_call: bool,
    pub may_allocate: bool,
    pub may_block: bool,
    pub may_panic: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PanicEdge {
    None,
    Propagate,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Provenance {
    Source(SourceSpan),
    Synthetic(SyntheticOrigin),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyntheticOrigin {
    NamedResultInitialization,
    ImplicitReturn,
}
