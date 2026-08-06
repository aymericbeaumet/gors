//! Typed MIR data model.

use crate::compiler::hir;
use crate::compiler::ids::{BasicBlockId, DefId, LocalId, PackageId};
use crate::compiler::provenance::SourceRef;
use crate::compiler::types::{ConstValue, Signature, Ty};

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
    pub signature: Signature,
    pub params: Vec<LocalId>,
    pub locals: Vec<LocalDecl>,
    pub blocks: Vec<BasicBlock>,
    pub entry: BasicBlockId,
    pub panic_cleanup: Option<PanicCleanup>,
    pub source: SourceRef,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PanicCleanup {
    pub entry: BasicBlockId,
    pub active: LocalId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalDecl {
    pub id: LocalId,
    pub name: Option<String>,
    pub ty: Ty,
    pub kind: hir::LocalKind,
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
    pub effects: hir::Effects,
    pub provenance: Provenance,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Place {
    pub local: LocalId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Rvalue {
    pub kind: RvalueKind,
    pub effects: hir::Effects,
    pub panic: PanicEdge,
    pub provenance: Provenance,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RvalueKind {
    Use(Operand),
    SliceLiteralI64(Vec<i64>),
    SliceLiteralU8(Vec<u8>),
    ArrayLiteralI64(Vec<i64>),
    ArrayIndexI64 {
        array: Operand,
        index: Operand,
    },
    ArraySetI64 {
        array: Operand,
        index: Operand,
        value: Operand,
    },
    Unary {
        op: hir::UnaryOp,
        operand: Operand,
        ty: Ty,
    },
    Conversion {
        operand: Operand,
        from: Ty,
        ty: Ty,
    },
    RecoverCompareNil {
        state: Place,
        equal: bool,
    },
    Binary {
        op: hir::BinaryOp,
        left: Operand,
        right: Operand,
        ty: Ty,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Operand {
    /// Read a Go value. Rust ownership behavior is selected only during
    /// mandatory Rust representation lowering.
    Read(Place),
    Constant(ConstValue, Ty),
    Unit,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Terminator {
    pub kind: TerminatorKind,
    pub effects: hir::Effects,
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
        callee: hir::Callee,
        args: Vec<Operand>,
        destinations: Vec<Place>,
        target: BasicBlockId,
    },
    Return(Vec<Operand>),
    Unreachable,
}

/// Source ownership for every executable MIR node.
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

/// Explicit exceptional control-flow from an operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PanicEdge {
    None,
    Propagate,
    Cleanup(BasicBlockId),
}
