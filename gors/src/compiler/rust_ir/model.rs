//! Explicit Rust representation IR consumed mechanically by syntax emission.

use crate::compiler::ids::{BasicBlockId, DefId, LocalId, PackageId, QualifiedDefId};
use crate::compiler::provenance::SourceRef;
use gors_runtime_abi::{PrimitiveOp, RuntimeOp};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct File {
    pub package_id: PackageId,
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
    pub panic_cleanup: Option<PanicCleanup>,
    pub control_flow: ControlFlowPlan,
    pub source: SourceRef,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PanicCleanup {
    pub entry: BasicBlockId,
    pub active: LocalId,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ControlFlowPlan {
    /// A proven straight-line CFG emitted as ordinary sequential Rust.
    StructuredLinear { order: Vec<BasicBlockId> },
    /// General CFG form with an explicit program-counter dispatcher.
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
        op: ValueOp,
        operand: Operand,
    },
    RecoverCompareNil {
        state: Place,
        equal: bool,
    },
    ArrayIndexI64 {
        array: Operand,
        index: Operand,
    },
    ArrayIndex {
        array: Operand,
        index: Operand,
    },
    ArraySetI64 {
        array: Operand,
        index: Operand,
        value: Operand,
    },
    ArraySet {
        array: Operand,
        index: Operand,
        value: Operand,
    },
    ArrayLiteral {
        elements: Vec<Operand>,
        ty: RustType,
    },
    StructLiteral {
        fields: Vec<Operand>,
        ty: RustType,
    },
    StructField {
        structure: Operand,
        field: u32,
    },
    StructSet {
        structure: Operand,
        field: u32,
        value: Operand,
    },
    StructLiteralI64(Vec<Operand>),
    StructFieldI64 {
        structure: Operand,
        field: u32,
    },
    StructSetI64 {
        structure: Operand,
        field: u32,
        value: Operand,
    },
    AggregateEqualI64 {
        left: Operand,
        right: Operand,
        equal: bool,
    },
    Binary {
        op: ValueOp,
        left: Operand,
        right: Operand,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValueOp {
    Primitive(PrimitiveOp),
    Runtime(RuntimeOp),
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
    F64(u64),
    Complex128 { real: u64, imag: u64 },
    StaticI64Array(Vec<i64>),
    RuntimeStaticBytes { op: RuntimeOp, bytes: Vec<u8> },
    RuntimeStaticI64s { op: RuntimeOp, values: Vec<i64> },
    RuntimeStaticBools { op: RuntimeOp, values: Vec<bool> },
    RuntimeStaticU8s { op: RuntimeOp, values: Vec<u8> },
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
        destinations: Vec<Place>,
        next: BasicBlockId,
    },
    Return(Vec<Operand>),
    Unreachable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CallTarget {
    Function(QualifiedDefId),
    Runtime(RuntimeOp),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RustType {
    Unit,
    Bool,
    I64,
    F64,
    Complex128,
    GoString,
    GoSliceI64,
    GoSliceU8,
    GoSliceBool,
    GoSliceInterface,
    GoMapStringI64,
    GoMapStringInterface,
    GoPointerI64,
    GoPointerStructI64,
    GoInterface,
    GoChannelI64,
    ArrayI64(u64),
    ArrayBool(u64),
    ArrayF64(u64),
    ArrayGoString(u64),
    ArrayGoPointerStructI64(u64),
    Struct(Vec<RustType>),
    StructI64(u64),
}

impl RustType {
    #[must_use]
    pub(in crate::compiler) fn scalar_array_parts(&self) -> Option<(u64, Self)> {
        match self {
            Self::ArrayBool(length) => Some((*length, Self::Bool)),
            Self::ArrayI64(length) => Some((*length, Self::I64)),
            Self::ArrayF64(length) => Some((*length, Self::F64)),
            Self::ArrayGoString(length) => Some((*length, Self::GoString)),
            Self::ArrayGoPointerStructI64(length) => Some((*length, Self::GoPointerStructI64)),
            _ => None,
        }
    }

    #[must_use]
    pub fn conservative_read_op(&self) -> Option<ReadOp> {
        match self {
            Self::Struct(fields) if fields.iter().all(Self::is_copy) => {
                Some(ReadOp::ProvenInitializedCopy)
            }
            Self::Struct(_) => Some(ReadOp::ProvenInitializedClone),
            Self::Bool
            | Self::I64
            | Self::F64
            | Self::Complex128
            | Self::ArrayBool(_)
            | Self::ArrayF64(_)
            | Self::ArrayI64(_)
            | Self::StructI64(_) => Some(ReadOp::ProvenInitializedCopy),
            Self::GoString
            | Self::GoSliceI64
            | Self::GoSliceU8
            | Self::GoSliceBool
            | Self::GoSliceInterface
            | Self::GoMapStringI64
            | Self::GoMapStringInterface
            | Self::GoPointerI64
            | Self::GoPointerStructI64
            | Self::GoInterface
            | Self::GoChannelI64
            | Self::ArrayGoString(_) => Some(ReadOp::ProvenInitializedClone),
            Self::ArrayGoPointerStructI64(_) => Some(ReadOp::ProvenInitializedClone),
            Self::Unit => None,
        }
    }

    pub(super) fn read_op_for_liveness(&self, live_after: bool) -> Option<ReadOp> {
        match self {
            Self::Struct(fields) if fields.iter().all(Self::is_copy) => {
                Some(ReadOp::ProvenInitializedCopy)
            }
            Self::Struct(_) if live_after => Some(ReadOp::ProvenInitializedClone),
            Self::Struct(_) => Some(ReadOp::ProvenLastUseMove),
            Self::Bool
            | Self::I64
            | Self::F64
            | Self::Complex128
            | Self::ArrayBool(_)
            | Self::ArrayF64(_)
            | Self::ArrayI64(_)
            | Self::StructI64(_) => Some(ReadOp::ProvenInitializedCopy),
            Self::GoString
            | Self::GoSliceI64
            | Self::GoSliceU8
            | Self::GoSliceBool
            | Self::GoSliceInterface
            | Self::GoMapStringI64
            | Self::GoMapStringInterface
            | Self::GoPointerI64
            | Self::GoPointerStructI64
            | Self::GoInterface
            | Self::GoChannelI64
            | Self::ArrayGoString(_)
            | Self::ArrayGoPointerStructI64(_)
                if live_after =>
            {
                Some(ReadOp::ProvenInitializedClone)
            }
            Self::GoString
            | Self::GoSliceI64
            | Self::GoSliceU8
            | Self::GoSliceBool
            | Self::GoSliceInterface
            | Self::GoMapStringI64
            | Self::GoMapStringInterface
            | Self::GoPointerI64
            | Self::GoPointerStructI64
            | Self::GoInterface
            | Self::GoChannelI64
            | Self::ArrayGoString(_) => Some(ReadOp::ProvenLastUseMove),
            Self::ArrayGoPointerStructI64(_) => Some(ReadOp::ProvenLastUseMove),
            Self::Unit => None,
        }
    }

    pub(super) fn supports_read_op(&self, op: ReadOp) -> bool {
        match self {
            Self::Struct(fields) if fields.iter().all(Self::is_copy) => {
                op == ReadOp::ProvenInitializedCopy
            }
            Self::Struct(_) => matches!(
                op,
                ReadOp::ProvenInitializedClone | ReadOp::ProvenLastUseMove
            ),
            Self::Bool
            | Self::I64
            | Self::F64
            | Self::Complex128
            | Self::ArrayBool(_)
            | Self::ArrayF64(_)
            | Self::ArrayI64(_)
            | Self::StructI64(_) => op == ReadOp::ProvenInitializedCopy,
            Self::GoString
            | Self::GoSliceI64
            | Self::GoSliceU8
            | Self::GoSliceBool
            | Self::GoSliceInterface
            | Self::GoMapStringI64
            | Self::GoMapStringInterface
            | Self::GoPointerI64
            | Self::GoPointerStructI64
            | Self::GoInterface
            | Self::GoChannelI64
            | Self::ArrayGoString(_) => matches!(
                op,
                ReadOp::ProvenInitializedClone | ReadOp::ProvenLastUseMove
            ),
            Self::ArrayGoPointerStructI64(_) => matches!(
                op,
                ReadOp::ProvenInitializedClone | ReadOp::ProvenLastUseMove
            ),
            Self::Unit => false,
        }
    }

    fn is_copy(&self) -> bool {
        matches!(
            self,
            Self::Bool
                | Self::I64
                | Self::F64
                | Self::Complex128
                | Self::ArrayBool(_)
                | Self::ArrayF64(_)
                | Self::ArrayI64(_)
                | Self::StructI64(_)
        ) || matches!(self, Self::Struct(fields) if fields.iter().all(Self::is_copy))
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
    Cleanup(BasicBlockId),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Provenance {
    Source(SourceRef),
    Synthetic(SyntheticOrigin),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyntheticOrigin {
    NamedResultInitialization,
    PanicCleanupInitialization,
    ZeroValueCall,
    PanicCleanupDispatch,
    ImplicitReturn,
}
