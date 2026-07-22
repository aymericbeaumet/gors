//! Verification for explicit Rust representation, ABI, storage, and control plans.

use std::collections::{BTreeMap, BTreeSet};

use super::*;
use crate::compiler::Diagnostic;
use crate::compiler::ids::SourceSpan;

impl File {
    pub(super) fn verify(&self) -> Result<(), Diagnostic> {
        let entrypoints = self
            .functions
            .iter()
            .filter(|function| function.artifact.entrypoint == EntrypointPlan::Executable)
            .count();
        if entrypoints > 1 {
            return Err(Diagnostic::backend(
                "multiple Rust IR executable entrypoints",
            ));
        }
        if entrypoints == 1 && self.package != "main" {
            return Err(Diagnostic::backend(
                "Rust IR executable entrypoint belongs to a non-main package",
            ));
        }

        let mut signatures = BTreeMap::new();
        let mut names = BTreeSet::new();
        let mut symbols = BTreeSet::new();
        for function in &self.functions {
            function.verify_artifact_plan()?;
            if signatures
                .insert(function.id, function.signature.clone())
                .is_some()
            {
                return Err(Diagnostic::backend(format!(
                    "duplicate Rust IR function DefId {}",
                    function.id.0
                )));
            }
            if !names.insert(function.name.as_str()) {
                return Err(Diagnostic::backend(format!(
                    "duplicate Rust IR function name {}",
                    function.name
                )));
            }
            if !symbols.insert(function.artifact.symbol.as_str()) {
                return Err(Diagnostic::backend(format!(
                    "duplicate Rust IR function symbol {}",
                    function.artifact.symbol.as_str()
                )));
            }
        }
        for function in &self.functions {
            function.verify(&signatures)?;
        }
        Ok(())
    }
}

impl Function {
    fn verify(&self, signatures: &BTreeMap<DefId, Signature>) -> Result<(), Diagnostic> {
        if self.control_flow != ControlFlowPlan::PcDispatchU32 {
            return Err(Diagnostic::backend("unsupported Rust IR control-flow plan"));
        }
        if self.entry.0 as usize >= self.blocks.len() {
            return Err(Diagnostic::backend("Rust IR entry block does not exist"));
        }
        if self.parameters.len() != self.signature.params.len() {
            return Err(Diagnostic::backend(
                "Rust IR parameter list does not match signature",
            ));
        }
        for (index, local) in self.locals.iter().enumerate() {
            if local.id.0 as usize != index {
                return Err(Diagnostic::backend(format!(
                    "Rust IR local IDs are not dense: expected {index}, found {}",
                    local.id.0
                )));
            }
            if local.storage != StorageClass::CheckedOptionSlot {
                return Err(Diagnostic::backend("unsupported Rust IR storage class"));
            }
            let expected_initialization = self
                .parameters
                .iter()
                .position(|parameter| *parameter == local.id)
                .map_or(
                    SlotInitialization::Uninitialized,
                    SlotInitialization::Parameter,
                );
            if local.initialization != expected_initialization {
                return Err(Diagnostic::backend(format!(
                    "Rust IR slot initialization mismatch for local {}",
                    local.id.0
                )));
            }
        }
        let mut parameter_ids = BTreeSet::new();
        for (position, (parameter, expected)) in self
            .parameters
            .iter()
            .zip(&self.signature.params)
            .enumerate()
        {
            if !parameter_ids.insert(*parameter) {
                return Err(Diagnostic::backend(format!(
                    "Rust IR repeats parameter local {}",
                    parameter.0
                )));
            }
            let local = self.local(*parameter)?;
            if local.initialization != SlotInitialization::Parameter(position) {
                return Err(Diagnostic::backend(format!(
                    "Rust IR parameter {position} has the wrong slot initializer"
                )));
            }
            verify_same(local.ty, *expected, "parameter")?;
        }
        for (index, block) in self.blocks.iter().enumerate() {
            if block.id.0 as usize != index {
                return Err(Diagnostic::backend(format!(
                    "Rust IR block IDs are not dense: expected {index}, found {}",
                    block.id.0
                )));
            }
            verify_source_provenance(&block.provenance, "basic block")?;
            for statement in &block.statements {
                self.verify_statement(statement)?;
            }
            self.verify_terminator(&block.terminator, signatures)?;
        }
        self.verify_storage_dataflow()
    }

    fn verify_artifact_plan(&self) -> Result<(), Diagnostic> {
        match self.artifact.entrypoint {
            EntrypointPlan::Executable => {
                let expected = FunctionArtifactPlan::executable_entrypoint();
                if self.artifact.symbol != expected.symbol {
                    return Err(Diagnostic::backend(
                        "Rust IR executable entrypoint must use the canonical main symbol",
                    ));
                }
                if self.artifact.linkage != expected.linkage {
                    return Err(Diagnostic::backend(
                        "Rust IR executable entrypoint must use internal linkage",
                    ));
                }
                if !self.signature.params.is_empty() || !self.signature.results.is_empty() {
                    return Err(Diagnostic::backend(
                        "Rust IR executable entrypoint must have no parameters or results",
                    ));
                }
            }
            EntrypointPlan::None => {
                let expected = FunctionArtifactPlan::public_definition(self.id);
                if self.artifact.symbol != expected.symbol {
                    return Err(Diagnostic::backend(format!(
                        "Rust IR function DefId {} does not use its canonical symbol",
                        self.id.0
                    )));
                }
                if self.artifact.linkage != expected.linkage {
                    return Err(Diagnostic::backend(format!(
                        "Rust IR function DefId {} must use public linkage",
                        self.id.0
                    )));
                }
            }
        }
        Ok(())
    }

    fn verify_statement(&self, statement: &Statement) -> Result<(), Diagnostic> {
        if statement.store != StoreOp::SetSome {
            return Err(Diagnostic::backend("unsupported Rust IR store operation"));
        }
        verify_statement_provenance(&statement.provenance)?;
        let destination = self.place_ty(statement.destination)?;
        let value = self.rvalue_ty(&statement.value)?;
        verify_same(value, destination, "assignment")?;
        verify_effects(
            statement.effects,
            statement_effects(&statement.value),
            "statement",
        )
    }

    fn rvalue_ty(&self, rvalue: &Rvalue) -> Result<RustType, Diagnostic> {
        verify_rvalue_provenance(&rvalue.provenance)?;
        let result = match &rvalue.kind {
            RvalueKind::Use(operand) => self.operand_ty(operand)?,
            RvalueKind::Unary { op, operand } => {
                let operand = self.operand_ty(operand)?;
                match op {
                    UnaryOp::Identity | UnaryOp::IntNeg | UnaryOp::IntBitNot => {
                        verify_same(operand, RustType::I64, "unary operand")?;
                        RustType::I64
                    }
                    UnaryOp::BoolNot => {
                        verify_same(operand, RustType::Bool, "unary operand")?;
                        RustType::Bool
                    }
                }
            }
            RvalueKind::Binary { op, left, right } => {
                let left = self.operand_ty(left)?;
                let right = self.operand_ty(right)?;
                verify_binary(*op, left, right)?
            }
        };
        verify_effects(rvalue.effects, rvalue_effects(&rvalue.kind), "rvalue")?;
        verify_panic(rvalue.effects, rvalue.panic, "rvalue")?;
        Ok(result)
    }

    fn verify_terminator(
        &self,
        terminator: &Terminator,
        signatures: &BTreeMap<DefId, Signature>,
    ) -> Result<(), Diagnostic> {
        verify_terminator_provenance(&terminator.provenance)?;
        match &terminator.kind {
            TerminatorKind::Goto(target) => {
                self.verify_target(*target)?;
            }
            TerminatorKind::SwitchBool {
                condition,
                then_target,
                else_target,
            } => {
                verify_same(
                    self.operand_ty(condition)?,
                    RustType::Bool,
                    "switch condition",
                )?;
                self.verify_target(*then_target)?;
                self.verify_target(*else_target)?;
            }
            TerminatorKind::Call {
                target,
                args,
                destination,
                next,
            } => {
                self.verify_target(*next)?;
                let argument_types = args
                    .iter()
                    .map(|argument| self.operand_ty(argument))
                    .collect::<Result<Vec<_>, _>>()?;
                match target {
                    CallTarget::Function(id) => {
                        let signature = signatures.get(id).ok_or_else(|| {
                            Diagnostic::backend(format!("unknown Rust IR callee DefId {}", id.0))
                        })?;
                        if argument_types != signature.params {
                            return Err(Diagnostic::backend(format!(
                                "Rust IR call arguments do not match DefId {}",
                                id.0
                            )));
                        }
                        self.verify_call_destination(*destination, &signature.results)?;
                    }
                    CallTarget::RuntimePrint { steps } => {
                        verify_print_plan(steps, &argument_types)?;
                        self.verify_call_destination(*destination, &[])?;
                    }
                }
            }
            TerminatorKind::Return(values) => {
                if values.len() != self.signature.results.len() {
                    return Err(Diagnostic::backend(
                        "Rust IR return count does not match signature",
                    ));
                }
                for (value, expected) in values.iter().zip(&self.signature.results) {
                    verify_same(self.operand_ty(value)?, *expected, "return value")?;
                }
            }
            TerminatorKind::Unreachable => {}
        }
        verify_effects(
            terminator.effects,
            terminator_effects(&terminator.kind),
            "terminator",
        )?;
        verify_panic(terminator.effects, terminator.panic, "terminator")
    }

    fn verify_call_destination(
        &self,
        destination: Option<Place>,
        results: &[RustType],
    ) -> Result<(), Diagnostic> {
        match (destination, results) {
            (None, []) => Ok(()),
            (Some(place), [result]) => {
                verify_same(self.place_ty(place)?, *result, "call destination")
            }
            (None, results) => Err(Diagnostic::backend(format!(
                "Rust IR call discards {} result value(s)",
                results.len()
            ))),
            (Some(_), []) => Err(Diagnostic::backend(
                "Rust IR no-result call has a destination",
            )),
            (Some(_), results) => Err(Diagnostic::backend(format!(
                "Rust IR lacks a {}-result destination representation",
                results.len()
            ))),
        }
    }

    fn verify_target(&self, target: BasicBlockId) -> Result<(), Diagnostic> {
        ((target.0 as usize) < self.blocks.len())
            .then_some(())
            .ok_or_else(|| Diagnostic::backend(format!("invalid Rust IR target {}", target.0)))
    }

    fn local(&self, local: LocalId) -> Result<&LocalDecl, Diagnostic> {
        self.locals
            .get(local.0 as usize)
            .ok_or_else(|| Diagnostic::backend(format!("invalid Rust IR local {}", local.0)))
    }

    fn place_ty(&self, place: Place) -> Result<RustType, Diagnostic> {
        self.local(place.local).map(|local| local.ty)
    }

    fn operand_ty(&self, operand: &Operand) -> Result<RustType, Diagnostic> {
        match operand {
            Operand::Read { place, op } => {
                let ty = self.place_ty(*place)?;
                let expected = ty
                    .read_op()
                    .ok_or_else(|| Diagnostic::backend("Rust IR cannot read a unit slot"))?;
                if *op != expected {
                    return Err(Diagnostic::backend(format!(
                        "Rust IR read operation mismatch: expected {expected:?}, found {op:?}"
                    )));
                }
                Ok(ty)
            }
            Operand::Constant(constant) => Ok(constant_type(constant)),
            Operand::Unit => Ok(RustType::Unit),
        }
    }
}

fn verify_binary(op: BinaryOp, left: RustType, right: RustType) -> Result<RustType, Diagnostic> {
    use BinaryOp::*;
    let expected = match op {
        IntAdd | IntSub | IntMul | IntDiv | IntRem | IntBitAnd | IntBitOr | IntBitXor | IntShl
        | IntShr | IntAndNot => (RustType::I64, RustType::I64, RustType::I64),
        BoolEqual | BoolNotEqual => (RustType::Bool, RustType::Bool, RustType::Bool),
        IntEqual | IntNotEqual | IntLess | IntLessEqual | IntGreater | IntGreaterEqual => {
            (RustType::I64, RustType::I64, RustType::Bool)
        }
        StringConcat => (RustType::GoString, RustType::GoString, RustType::GoString),
        StringEqual | StringNotEqual | StringLess | StringLessEqual | StringGreater
        | StringGreaterEqual => (RustType::GoString, RustType::GoString, RustType::Bool),
    };
    if left == expected.0 && right == expected.1 {
        Ok(expected.2)
    } else {
        Err(Diagnostic::backend(format!(
            "invalid Rust IR binary operands for {op:?}: {left:?}, {right:?}"
        )))
    }
}

fn verify_print_plan(steps: &[PrintStep], types: &[RustType]) -> Result<(), Diagnostic> {
    let print = print_plan(types, false)
        .ok_or_else(|| Diagnostic::backend("Rust IR print plan contains a unit argument"))?;
    let println = print_plan(types, true)
        .ok_or_else(|| Diagnostic::backend("Rust IR print plan contains a unit argument"))?;
    if steps != print && steps != println {
        return Err(Diagnostic::backend(
            "Rust IR print plan is not an exact canonical print or println plan",
        ));
    }
    Ok(())
}

fn constant_type(constant: &Constant) -> RustType {
    match constant {
        Constant::Bool(_) => RustType::Bool,
        Constant::I64(_) => RustType::I64,
        Constant::GoString(_) => RustType::GoString,
    }
}

fn verify_effects(actual: Effects, expected: Effects, context: &str) -> Result<(), Diagnostic> {
    (actual == expected).then_some(()).ok_or_else(|| {
        Diagnostic::backend(format!(
            "Rust IR {context} effect mismatch: expected {expected:?}, found {actual:?}"
        ))
    })
}

fn verify_panic(effects: Effects, edge: PanicEdge, context: &str) -> Result<(), Diagnostic> {
    let expected = if effects.may_panic {
        PanicEdge::Propagate
    } else {
        PanicEdge::None
    };
    (edge == expected).then_some(()).ok_or_else(|| {
        Diagnostic::backend(format!(
            "Rust IR {context} panic edge mismatch: expected {expected:?}, found {edge:?}"
        ))
    })
}

fn verify_same(actual: RustType, expected: RustType, context: &str) -> Result<(), Diagnostic> {
    (actual == expected).then_some(()).ok_or_else(|| {
        Diagnostic::backend(format!(
            "Rust IR {context} type mismatch: expected {expected:?}, found {actual:?}"
        ))
    })
}

fn verify_source_provenance(provenance: &Provenance, context: &str) -> Result<(), Diagnostic> {
    match provenance {
        Provenance::Source(span) => verify_span(span, context),
        Provenance::Synthetic(origin) => Err(Diagnostic::backend(format!(
            "synthetic provenance {origin:?} is invalid for {context}"
        ))),
    }
}

fn verify_statement_provenance(provenance: &Provenance) -> Result<(), Diagnostic> {
    match provenance {
        Provenance::Source(span) => verify_span(span, "statement"),
        Provenance::Synthetic(SyntheticOrigin::NamedResultInitialization) => Ok(()),
        Provenance::Synthetic(other) => Err(Diagnostic::backend(format!(
            "synthetic provenance {other:?} is invalid for a statement"
        ))),
    }
}

fn verify_rvalue_provenance(provenance: &Provenance) -> Result<(), Diagnostic> {
    match provenance {
        Provenance::Source(span) => verify_span(span, "rvalue"),
        Provenance::Synthetic(SyntheticOrigin::NamedResultInitialization) => Ok(()),
        Provenance::Synthetic(other) => Err(Diagnostic::backend(format!(
            "synthetic provenance {other:?} is invalid for an rvalue"
        ))),
    }
}

fn verify_terminator_provenance(provenance: &Provenance) -> Result<(), Diagnostic> {
    match provenance {
        Provenance::Source(span) => verify_span(span, "terminator"),
        Provenance::Synthetic(SyntheticOrigin::ImplicitReturn) => Ok(()),
        Provenance::Synthetic(other) => Err(Diagnostic::backend(format!(
            "synthetic provenance {other:?} is invalid for a terminator"
        ))),
    }
}

fn verify_span(span: &SourceSpan, context: &str) -> Result<(), Diagnostic> {
    if span.file.is_empty() || span.line == 0 || span.column == 0 || span.end < span.start {
        Err(Diagnostic::backend(format!(
            "missing or invalid source provenance for Rust IR {context}"
        )))
    } else {
        Ok(())
    }
}
