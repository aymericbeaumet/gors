//! Typed MIR data model.

use crate::compiler::hir;
use crate::compiler::ids::{BasicBlockId, DefId, LocalId, SourceSpan};
use crate::compiler::types::{ConstValue, Signature, Ty};

#[derive(Clone, Debug)]
pub struct File {
    pub package: String,
    pub functions: Vec<Function>,
}

#[derive(Clone, Debug)]
pub struct Function {
    pub id: DefId,
    pub name: String,
    pub signature: Signature,
    pub params: Vec<LocalId>,
    pub locals: Vec<LocalDecl>,
    pub blocks: Vec<BasicBlock>,
    pub entry: BasicBlockId,
    pub span: SourceSpan,
}

#[derive(Clone, Debug)]
pub struct LocalDecl {
    pub id: LocalId,
    pub name: Option<String>,
    pub ty: Ty,
    pub kind: hir::LocalKind,
}

#[derive(Clone, Debug)]
pub struct BasicBlock {
    pub id: BasicBlockId,
    pub provenance: Provenance,
    pub statements: Vec<Statement>,
    pub terminator: Terminator,
}

#[derive(Clone, Debug)]
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

#[derive(Clone, Debug)]
pub struct Rvalue {
    pub kind: RvalueKind,
    pub effects: hir::Effects,
    pub panic: PanicEdge,
    pub provenance: Provenance,
}

#[derive(Clone, Debug)]
pub enum RvalueKind {
    Use(Operand),
    Unary {
        op: hir::UnaryOp,
        operand: Operand,
        ty: Ty,
    },
    Binary {
        op: hir::BinaryOp,
        left: Operand,
        right: Operand,
        ty: Ty,
    },
}

#[derive(Clone, Debug)]
pub enum Operand {
    /// Read a Go value. Rust ownership behavior is selected only during
    /// mandatory Rust representation lowering.
    Read(Place),
    Constant(ConstValue, Ty),
    Unit,
}

#[derive(Clone, Debug)]
pub struct Terminator {
    pub kind: TerminatorKind,
    pub effects: hir::Effects,
    pub panic: PanicEdge,
    pub provenance: Provenance,
}

#[derive(Clone, Debug)]
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
        destination: Option<Place>,
        target: BasicBlockId,
    },
    Return(Vec<Operand>),
    Unreachable,
}

/// Source ownership for every executable MIR node.
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

/// Explicit exceptional control-flow from an operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PanicEdge {
    None,
    /// Propagate the Go panic to the caller. Recover/defer landing pads will
    /// replace this edge when that frontier is implemented.
    Propagate,
}
