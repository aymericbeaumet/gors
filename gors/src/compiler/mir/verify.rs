//! Structural, semantic-effect, provenance, and call-ABI MIR verification.

use std::collections::{BTreeMap, BTreeSet};

use super::{
    File, Function, LocalDecl, Operand, PanicEdge, Place, Provenance, Rvalue, RvalueKind,
    Statement, SyntheticOrigin, Terminator, TerminatorKind,
};
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::{BasicBlockId, DefId, LocalId, SourceSpan};
use crate::compiler::types::{ConstValue, IntTy, Signature, Ty};

impl File {
    #[cfg(test)]
    pub(super) fn verify(&self) -> Result<(), Diagnostic> {
        let mut signatures = BTreeMap::new();
        let mut names = BTreeSet::new();
        for function in &self.functions {
            if signatures
                .insert(function.id, function.signature.clone())
                .is_some()
            {
                return Err(Diagnostic::backend(format!(
                    "duplicate MIR function DefId {}",
                    function.id
                )));
            }
            if !names.insert(function.name.as_str()) {
                return Err(Diagnostic::backend(format!(
                    "duplicate MIR function name {}",
                    function.name
                )));
            }
        }
        self.verify_with_signatures(&signatures)
    }

    pub(super) fn verify_with_signatures(
        &self,
        signatures: &BTreeMap<DefId, Signature>,
    ) -> Result<(), Diagnostic> {
        let mut names = BTreeSet::new();
        let mut definitions = BTreeSet::new();
        for function in &self.functions {
            if !definitions.insert(function.id) {
                return Err(Diagnostic::backend(format!(
                    "duplicate MIR function DefId {}",
                    function.id
                )));
            }
            if !names.insert(function.name.as_str()) {
                return Err(Diagnostic::backend(format!(
                    "duplicate MIR function name {}",
                    function.name
                )));
            }
            match signatures.get(&function.id) {
                Some(signature) if signature == &function.signature => {}
                Some(_) => {
                    return Err(Diagnostic::backend(format!(
                        "MIR function DefId {} disagrees with the package signature index",
                        function.id
                    )));
                }
                None => {
                    return Err(Diagnostic::backend(format!(
                        "MIR function DefId {} is absent from the package signature index",
                        function.id
                    )));
                }
            }
            function.verify(signatures)?;
        }
        Ok(())
    }
}

impl Function {
    pub(super) fn verify_with_signatures(
        &self,
        signatures: &BTreeMap<DefId, Signature>,
    ) -> Result<(), Diagnostic> {
        match signatures.get(&self.id) {
            Some(signature) if signature == &self.signature => {}
            Some(_) => {
                return Err(Diagnostic::backend(format!(
                    "MIR function DefId {} disagrees with the package signature index",
                    self.id
                )));
            }
            None => {
                return Err(Diagnostic::backend(format!(
                    "MIR function DefId {} is absent from the package signature index",
                    self.id
                )));
            }
        }
        self.verify(signatures)
    }

    fn verify(&self, signatures: &BTreeMap<DefId, Signature>) -> Result<(), Diagnostic> {
        for ty in self.signature.params.iter().chain(&self.signature.results) {
            verify_bootstrap_type(ty, "function signature")?;
        }
        if self.entry.0 as usize >= self.blocks.len() {
            return Err(Diagnostic::backend("MIR entry block does not exist"));
        }
        for (index, local) in self.locals.iter().enumerate() {
            if local.id.0 as usize != index {
                return Err(Diagnostic::backend(format!(
                    "MIR local IDs are not dense: expected {index}, found {}",
                    local.id.0
                )));
            }
            verify_bootstrap_type(&local.ty, "local")?;
        }
        if self.params.len() != self.signature.params.len() {
            return Err(Diagnostic::backend(format!(
                "MIR function {} has {} parameter locals but expects {}",
                self.name,
                self.params.len(),
                self.signature.params.len()
            )));
        }
        let mut parameters = BTreeSet::new();
        for (position, (parameter, expected)) in
            self.params.iter().zip(&self.signature.params).enumerate()
        {
            let declaration = self.local(*parameter)?;
            if !parameters.insert(*parameter) {
                return Err(Diagnostic::backend(format!(
                    "MIR function {} repeats parameter local {}",
                    self.name, parameter.0
                )));
            }
            if declaration.kind != hir::LocalKind::Parameter {
                return Err(Diagnostic::backend(format!(
                    "MIR parameter {position} uses non-parameter local {}",
                    parameter.0
                )));
            }
            verify_same_type(&declaration.ty, expected, "parameter")?;
        }
        for (index, block) in self.blocks.iter().enumerate() {
            if block.id.0 as usize != index {
                return Err(Diagnostic::backend(format!(
                    "MIR block IDs are not dense: expected {index}, found {}",
                    block.id.0
                )));
            }
            verify_source_provenance(&block.provenance, "basic block")?;
            for statement in &block.statements {
                self.verify_statement(statement)?;
            }
            self.verify_terminator(&block.terminator, signatures)?;
        }
        self.verify_definite_initialization()
    }

    fn verify_statement(&self, statement: &Statement) -> Result<(), Diagnostic> {
        verify_statement_provenance(&statement.provenance)?;
        let destination_ty = self.place_ty(statement.destination)?;
        let value_ty = self.verify_rvalue(&statement.value)?;
        verify_same_type(destination_ty, &value_ty, "assignment")?;
        let expected = statement.value.effects.union(hir::Effects {
            may_write: true,
            ..hir::Effects::default()
        });
        verify_effects(statement.effects, expected, "statement")
    }

    fn verify_rvalue(&self, rvalue: &Rvalue) -> Result<Ty, Diagnostic> {
        verify_rvalue_provenance(&rvalue.provenance)?;
        let (ty, intrinsic) = match &rvalue.kind {
            RvalueKind::Use(operand) => (self.operand_ty(operand)?, hir::Effects::default()),
            RvalueKind::Unary { op, operand, ty } => {
                let operand_ty = self.operand_ty(operand)?;
                let expected = match op {
                    hir::UnaryOp::Positive | hir::UnaryOp::Negative | hir::UnaryOp::BitNot => {
                        Ty::Int(IntTy::Int)
                    }
                    hir::UnaryOp::Not => Ty::Bool,
                };
                verify_same_type(&operand_ty, &expected, "unary operand")?;
                verify_same_type(ty, &expected, "unary result")?;
                (ty.clone(), hir::Effects::default())
            }
            RvalueKind::Binary {
                op,
                left,
                right,
                ty,
            } => {
                let left_ty = self.operand_ty(left)?;
                let right_ty = self.operand_ty(right)?;
                verify_binary_types(*op, &left_ty, &right_ty, ty)?;
                (ty.clone(), binary_effects(*op, ty))
            }
        };
        let expected = intrinsic.union(read_effects(rvalue_operands(&rvalue.kind)));
        verify_effects(rvalue.effects, expected, "rvalue")?;
        verify_panic_edge(rvalue.effects, rvalue.panic, "rvalue")?;
        Ok(ty)
    }

    fn verify_terminator(
        &self,
        terminator: &Terminator,
        signatures: &BTreeMap<DefId, Signature>,
    ) -> Result<(), Diagnostic> {
        verify_terminator_provenance(&terminator.provenance)?;
        let intrinsic = match &terminator.kind {
            TerminatorKind::Goto(target) => {
                self.verify_target(*target)?;
                hir::Effects::default()
            }
            TerminatorKind::SwitchBool {
                condition,
                then_target,
                else_target,
            } => {
                verify_same_type(&self.operand_ty(condition)?, &Ty::Bool, "switch condition")?;
                self.verify_target(*then_target)?;
                self.verify_target(*else_target)?;
                hir::Effects::default()
            }
            TerminatorKind::Call {
                callee,
                args,
                destination,
                target,
            } => {
                self.verify_target(*target)?;
                let argument_types = args
                    .iter()
                    .map(|argument| self.operand_ty(argument))
                    .collect::<Result<Vec<_>, _>>()?;
                match callee {
                    hir::Callee::Function(id) => {
                        let signature = signatures.get(id).ok_or_else(|| {
                            Diagnostic::backend(format!("unknown MIR callee DefId {id}"))
                        })?;
                        if argument_types.len() != signature.params.len() {
                            return Err(Diagnostic::backend(format!(
                                "MIR call to DefId {} has {} arguments but expects {}",
                                id,
                                argument_types.len(),
                                signature.params.len()
                            )));
                        }
                        for (actual, expected) in argument_types.iter().zip(&signature.params) {
                            verify_same_type(actual, expected, "call argument")?;
                        }
                        self.verify_call_destination(destination, &signature.results)?;
                    }
                    hir::Callee::Builtin(_) => {
                        for ty in &argument_types {
                            if !matches!(ty, Ty::Bool | Ty::Int(IntTy::Int) | Ty::String) {
                                return Err(Diagnostic::backend(format!(
                                    "print builtin cannot consume MIR operand type {ty:?}"
                                )));
                            }
                        }
                        self.verify_call_destination(destination, &[])?;
                    }
                }
                call_effects()
            }
            TerminatorKind::Return(values) => {
                if values.len() != self.signature.results.len() {
                    return Err(Diagnostic::backend(format!(
                        "MIR return has {} values but signature has {}",
                        values.len(),
                        self.signature.results.len()
                    )));
                }
                for (value, expected) in values.iter().zip(&self.signature.results) {
                    verify_same_type(&self.operand_ty(value)?, expected, "return value")?;
                }
                hir::Effects::default()
            }
            TerminatorKind::Unreachable => hir::Effects::default(),
        };
        let expected = intrinsic.union(read_effects(terminator_operands(&terminator.kind)));
        verify_effects(terminator.effects, expected, "terminator")?;
        verify_panic_edge(terminator.effects, terminator.panic, "terminator")
    }

    fn verify_call_destination(
        &self,
        destination: &Option<Place>,
        results: &[Ty],
    ) -> Result<(), Diagnostic> {
        match (destination, results) {
            (None, []) => Ok(()),
            (Some(place), [result]) => {
                verify_same_type(self.place_ty(*place)?, result, "call destination")
            }
            (Some(place), many @ [_, _, ..]) => verify_same_type(
                self.place_ty(*place)?,
                &Ty::Tuple(many.to_vec()),
                "call destination",
            ),
            (None, results) => Err(Diagnostic::backend(format!(
                "MIR call discards {} result value(s)",
                results.len()
            ))),
            (Some(_), []) => Err(Diagnostic::backend(
                "MIR call with no results has a destination",
            )),
        }
    }

    fn verify_target(&self, target: BasicBlockId) -> Result<(), Diagnostic> {
        ((target.0 as usize) < self.blocks.len())
            .then_some(())
            .ok_or_else(|| Diagnostic::backend(format!("invalid MIR target {}", target.0)))
    }

    fn local(&self, local: LocalId) -> Result<&LocalDecl, Diagnostic> {
        self.locals
            .get(local.0 as usize)
            .ok_or_else(|| Diagnostic::backend(format!("invalid MIR local {}", local.0)))
    }

    fn place_ty(&self, place: Place) -> Result<&Ty, Diagnostic> {
        self.local(place.local).map(|local| &local.ty)
    }

    fn operand_ty(&self, operand: &Operand) -> Result<Ty, Diagnostic> {
        match operand {
            Operand::Read(place) => Ok(self.place_ty(*place)?.clone()),
            Operand::Constant(value, ty) => {
                verify_bootstrap_type(ty, "constant operand")?;
                verify_constant_type(value, ty)?;
                Ok(ty.clone())
            }
            Operand::Unit => Ok(Ty::Unit),
        }
    }
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
            "missing or invalid source provenance for MIR {context}"
        )))
    } else {
        Ok(())
    }
}

fn verify_effects(
    actual: hir::Effects,
    expected: hir::Effects,
    context: &str,
) -> Result<(), Diagnostic> {
    (actual == expected).then_some(()).ok_or_else(|| {
        Diagnostic::backend(format!(
            "MIR {context} effect mismatch: expected {expected:?}, found {actual:?}"
        ))
    })
}

fn verify_panic_edge(
    effects: hir::Effects,
    edge: PanicEdge,
    context: &str,
) -> Result<(), Diagnostic> {
    let expected = if effects.may_panic {
        PanicEdge::Propagate
    } else {
        PanicEdge::None
    };
    (edge == expected).then_some(()).ok_or_else(|| {
        Diagnostic::backend(format!(
            "MIR {context} panic edge mismatch: expected {expected:?}, found {edge:?}"
        ))
    })
}

fn read_effects<'a>(operands: impl IntoIterator<Item = &'a Operand>) -> hir::Effects {
    hir::Effects {
        may_read: operands
            .into_iter()
            .any(|operand| matches!(operand, Operand::Read(_))),
        ..hir::Effects::default()
    }
}

fn rvalue_operands(kind: &RvalueKind) -> Vec<&Operand> {
    match kind {
        RvalueKind::Use(operand) | RvalueKind::Unary { operand, .. } => vec![operand],
        RvalueKind::Binary { left, right, .. } => vec![left, right],
    }
}

fn terminator_operands(kind: &TerminatorKind) -> Vec<&Operand> {
    match kind {
        TerminatorKind::SwitchBool { condition, .. } => vec![condition],
        TerminatorKind::Call { args, .. } | TerminatorKind::Return(args) => args.iter().collect(),
        TerminatorKind::Goto(_) | TerminatorKind::Unreachable => Vec::new(),
    }
}

fn binary_effects(op: hir::BinaryOp, result: &Ty) -> hir::Effects {
    hir::Effects {
        may_allocate: op == hir::BinaryOp::Add && result == &Ty::String,
        may_panic: matches!(
            op,
            hir::BinaryOp::Div | hir::BinaryOp::Rem | hir::BinaryOp::Shl | hir::BinaryOp::Shr
        ),
        ..hir::Effects::default()
    }
}

fn call_effects() -> hir::Effects {
    hir::Effects {
        may_call: true,
        may_allocate: true,
        may_block: true,
        may_panic: true,
        may_write: true,
        ..hir::Effects::default()
    }
}

fn verify_bootstrap_type(ty: &Ty, context: &str) -> Result<(), Diagnostic> {
    if matches!(ty, Ty::Unit | Ty::Bool | Ty::Int(IntTy::Int) | Ty::String) {
        Ok(())
    } else {
        Err(Diagnostic::backend(format!(
            "unsupported type reached {context} MIR: {ty:?}"
        )))
    }
}

fn verify_constant_type(value: &ConstValue, ty: &Ty) -> Result<(), Diagnostic> {
    matches!(
        (value, ty),
        (ConstValue::Bool(_), Ty::Bool)
            | (ConstValue::Int(_), Ty::Int(IntTy::Int))
            | (ConstValue::String(_), Ty::String)
    )
    .then_some(())
    .ok_or_else(|| {
        Diagnostic::backend(format!(
            "MIR constant {value:?} does not have declared type {ty:?}"
        ))
    })
}

fn verify_same_type(actual: &Ty, expected: &Ty, context: &str) -> Result<(), Diagnostic> {
    (actual == expected).then_some(()).ok_or_else(|| {
        Diagnostic::backend(format!(
            "MIR {context} type mismatch: expected {expected:?}, found {actual:?}"
        ))
    })
}

fn verify_binary_types(
    op: hir::BinaryOp,
    left: &Ty,
    right: &Ty,
    result: &Ty,
) -> Result<(), Diagnostic> {
    let int = Ty::Int(IntTy::Int);
    let valid = match op {
        hir::BinaryOp::Add => {
            (left == &int && right == &int && result == &int)
                || (left == &Ty::String && right == &Ty::String && result == &Ty::String)
        }
        hir::BinaryOp::Sub
        | hir::BinaryOp::Mul
        | hir::BinaryOp::Div
        | hir::BinaryOp::Rem
        | hir::BinaryOp::BitAnd
        | hir::BinaryOp::BitOr
        | hir::BinaryOp::BitXor
        | hir::BinaryOp::Shl
        | hir::BinaryOp::Shr
        | hir::BinaryOp::AndNot => left == &int && right == &int && result == &int,
        hir::BinaryOp::Equal | hir::BinaryOp::NotEqual => {
            left == right
                && matches!(left, Ty::Bool | Ty::Int(IntTy::Int) | Ty::String)
                && result == &Ty::Bool
        }
        hir::BinaryOp::Less
        | hir::BinaryOp::LessEqual
        | hir::BinaryOp::Greater
        | hir::BinaryOp::GreaterEqual => {
            left == right && matches!(left, Ty::Int(IntTy::Int) | Ty::String) && result == &Ty::Bool
        }
        hir::BinaryOp::LogicalAnd | hir::BinaryOp::LogicalOr => {
            left == &Ty::Bool && right == &Ty::Bool && result == &Ty::Bool
        }
    };
    valid.then_some(()).ok_or_else(|| {
        Diagnostic::backend(format!(
            "invalid MIR binary operation {op:?}: {left:?}, {right:?} -> {result:?}"
        ))
    })
}
