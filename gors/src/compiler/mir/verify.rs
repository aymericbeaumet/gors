//! Structural, semantic-effect, provenance, and call-ABI MIR verification.

use std::collections::{BTreeMap, BTreeSet};

use super::{
    File, Function, LocalDecl, Operand, PanicEdge, Place, Provenance, Rvalue, RvalueKind,
    Statement, SyntheticOrigin, Terminator, TerminatorKind,
};
use crate::compiler::Diagnostic;
use crate::compiler::hir;
use crate::compiler::ids::{BasicBlockId, DefId, LocalId};
use crate::compiler::provenance::SourceRef;
use crate::compiler::types::{ComplexTy, ConstValue, FloatTy, IntTy, Signature, Ty};

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
        verify_source_ref(self.source, self.id, "function")?;
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
            verify_source_provenance(&block.provenance, self.id, "basic block")?;
            for statement in &block.statements {
                self.verify_statement(statement)?;
            }
            self.verify_terminator(&block.terminator, signatures)?;
        }
        self.verify_definite_initialization()
    }

    fn verify_statement(&self, statement: &Statement) -> Result<(), Diagnostic> {
        verify_statement_provenance(&statement.provenance, self.id)?;
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
        verify_rvalue_provenance(&rvalue.provenance, self.id)?;
        let (ty, intrinsic) = match &rvalue.kind {
            RvalueKind::Use(operand) => (self.operand_ty(operand)?, hir::Effects::default()),
            RvalueKind::SliceLiteralI64(_) => (
                Ty::Slice(Box::new(Ty::Int(IntTy::Int))),
                hir::Effects {
                    may_allocate: true,
                    ..hir::Effects::default()
                },
            ),
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
            RvalueKind::Conversion { operand, from, ty } => {
                let operand_ty = self.operand_ty(operand)?;
                verify_same_type(&operand_ty, from, "conversion operand")?;
                if from.underlying() != ty.underlying() {
                    return Err(Diagnostic::backend(format!(
                        "MIR conversion changes representation from {from:?} to {ty:?}"
                    )));
                }
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
        verify_terminator_provenance(&terminator.provenance, self.id)?;
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
                    hir::Callee::Builtin(builtin) => {
                        let results = match builtin {
                            hir::Builtin::Print | hir::Builtin::Println => {
                                for ty in &argument_types {
                                    if !matches!(ty, Ty::Bool | Ty::Int(IntTy::Int) | Ty::String) {
                                        return Err(Diagnostic::backend(format!(
                                            "print builtin cannot consume MIR operand type {ty:?}"
                                        )));
                                    }
                                }
                                Vec::new()
                            }
                            hir::Builtin::Panic => {
                                if argument_types.len() != 1 {
                                    return Err(Diagnostic::backend(format!(
                                        "panic builtin has {} MIR operands; expected 1",
                                        argument_types.len()
                                    )));
                                }
                                if !matches!(
                                    argument_types.as_slice(),
                                    [Ty::Bool | Ty::Int(IntTy::Int) | Ty::String]
                                ) {
                                    return Err(Diagnostic::backend(
                                        "panic builtin received an unsupported MIR operand type",
                                    ));
                                }
                                Vec::new()
                            }
                            hir::Builtin::SliceI64Index => {
                                verify_slice_call_arguments(&argument_types, 2, "slice index")?;
                                vec![Ty::Int(IntTy::Int)]
                            }
                            hir::Builtin::SliceI64Range => {
                                verify_slice_call_arguments(
                                    &argument_types,
                                    4,
                                    "slice expression",
                                )?;
                                vec![Ty::Slice(Box::new(Ty::Int(IntTy::Int)))]
                            }
                            hir::Builtin::SliceI64Set => {
                                verify_slice_call_arguments(
                                    &argument_types,
                                    3,
                                    "slice assignment",
                                )?;
                                Vec::new()
                            }
                            hir::Builtin::SliceI64Make => {
                                if argument_types != [Ty::Int(IntTy::Int), Ty::Int(IntTy::Int)] {
                                    return Err(Diagnostic::backend(format!(
                                        "invalid MIR slice make argument types: {argument_types:?}"
                                    )));
                                }
                                vec![Ty::Slice(Box::new(Ty::Int(IntTy::Int)))]
                            }
                            hir::Builtin::SliceI64Len | hir::Builtin::SliceI64Cap => {
                                if argument_types != [Ty::Slice(Box::new(Ty::Int(IntTy::Int)))] {
                                    return Err(Diagnostic::backend(format!(
                                        "invalid MIR slice len/cap argument types: {argument_types:?}"
                                    )));
                                }
                                vec![Ty::Int(IntTy::Int)]
                            }
                            hir::Builtin::SliceI64Append => {
                                verify_slice_call_arguments(&argument_types, 2, "slice append")?;
                                vec![Ty::Slice(Box::new(Ty::Int(IntTy::Int)))]
                            }
                        };
                        self.verify_call_destination(destination, &results)?;
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

fn verify_source_provenance(
    provenance: &Provenance,
    owner: DefId,
    context: &str,
) -> Result<(), Diagnostic> {
    match provenance {
        Provenance::Source(source) => verify_source_ref(*source, owner, context),
        Provenance::Synthetic(origin) => Err(Diagnostic::backend(format!(
            "synthetic provenance {origin:?} is invalid for {context}"
        ))),
    }
}

fn verify_statement_provenance(provenance: &Provenance, owner: DefId) -> Result<(), Diagnostic> {
    match provenance {
        Provenance::Source(source) => verify_source_ref(*source, owner, "statement"),
        Provenance::Synthetic(SyntheticOrigin::NamedResultInitialization) => Ok(()),
        Provenance::Synthetic(other) => Err(Diagnostic::backend(format!(
            "synthetic provenance {other:?} is invalid for a statement"
        ))),
    }
}

fn verify_rvalue_provenance(provenance: &Provenance, owner: DefId) -> Result<(), Diagnostic> {
    match provenance {
        Provenance::Source(source) => verify_source_ref(*source, owner, "rvalue"),
        Provenance::Synthetic(SyntheticOrigin::NamedResultInitialization) => Ok(()),
        Provenance::Synthetic(other) => Err(Diagnostic::backend(format!(
            "synthetic provenance {other:?} is invalid for an rvalue"
        ))),
    }
}

fn verify_terminator_provenance(provenance: &Provenance, owner: DefId) -> Result<(), Diagnostic> {
    match provenance {
        Provenance::Source(source) => verify_source_ref(*source, owner, "terminator"),
        Provenance::Synthetic(SyntheticOrigin::ImplicitReturn) => Ok(()),
        Provenance::Synthetic(other) => Err(Diagnostic::backend(format!(
            "synthetic provenance {other:?} is invalid for a terminator"
        ))),
    }
}

fn verify_source_ref(source: SourceRef, owner: DefId, context: &str) -> Result<(), Diagnostic> {
    (source.owner() == owner).then_some(()).ok_or_else(|| {
        Diagnostic::backend(format!(
            "MIR {context} source reference is owned by DefId {}, expected {owner}",
            source.owner()
        ))
    })
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
        RvalueKind::Use(operand)
        | RvalueKind::Unary { operand, .. }
        | RvalueKind::Conversion { operand, .. } => vec![operand],
        RvalueKind::Binary { left, right, .. } => vec![left, right],
        RvalueKind::SliceLiteralI64(_) => Vec::new(),
    }
}

fn verify_slice_call_arguments(
    arguments: &[Ty],
    expected_len: usize,
    context: &str,
) -> Result<(), Diagnostic> {
    let slice = Ty::Slice(Box::new(Ty::Int(IntTy::Int)));
    if arguments.len() != expected_len
        || arguments.first() != Some(&slice)
        || arguments
            .get(1..)
            .unwrap_or_default()
            .iter()
            .any(|ty| ty != &Ty::Int(IntTy::Int))
    {
        return Err(Diagnostic::backend(format!(
            "invalid MIR {context} argument types: {arguments:?}"
        )));
    }
    Ok(())
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
    if *ty == Ty::Unit || ty.is_bootstrap_value() {
        Ok(())
    } else {
        Err(Diagnostic::backend(format!(
            "unsupported type reached {context} MIR: {ty:?}"
        )))
    }
}

fn verify_constant_type(value: &ConstValue, ty: &Ty) -> Result<(), Diagnostic> {
    let underlying = ty.underlying();
    matches!(
        (value, underlying),
        (ConstValue::Bool(_), Ty::Bool)
            | (ConstValue::Int(_), Ty::Int(IntTy::Int))
            | (ConstValue::Float(_), Ty::Float(FloatTy::Float64))
            | (ConstValue::Int(_), Ty::Float(FloatTy::Float64))
            | (
                ConstValue::Complex { .. },
                Ty::Complex(ComplexTy::Complex128)
            )
            | (ConstValue::Int(_), Ty::Complex(ComplexTy::Complex128))
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
    let same_operands = left == right;
    let same_result = result == left;
    let underlying = left.underlying();
    let valid = match op {
        hir::BinaryOp::Add => {
            same_operands
                && same_result
                && matches!(
                    underlying,
                    Ty::Int(IntTy::Int)
                        | Ty::Float(FloatTy::Float64)
                        | Ty::Complex(ComplexTy::Complex128)
                        | Ty::String
                )
        }
        hir::BinaryOp::Sub | hir::BinaryOp::Mul | hir::BinaryOp::Div => {
            same_operands
                && same_result
                && matches!(
                    underlying,
                    Ty::Int(IntTy::Int)
                        | Ty::Float(FloatTy::Float64)
                        | Ty::Complex(ComplexTy::Complex128)
                )
        }
        hir::BinaryOp::Rem
        | hir::BinaryOp::BitAnd
        | hir::BinaryOp::BitOr
        | hir::BinaryOp::BitXor
        | hir::BinaryOp::Shl
        | hir::BinaryOp::Shr
        | hir::BinaryOp::AndNot => {
            same_operands && same_result && *underlying == Ty::Int(IntTy::Int)
        }
        hir::BinaryOp::Equal | hir::BinaryOp::NotEqual => {
            same_operands
                && matches!(
                    underlying,
                    Ty::Bool
                        | Ty::Int(IntTy::Int)
                        | Ty::Float(FloatTy::Float64)
                        | Ty::Complex(ComplexTy::Complex128)
                        | Ty::String
                )
                && result == &Ty::Bool
        }
        hir::BinaryOp::Less
        | hir::BinaryOp::LessEqual
        | hir::BinaryOp::Greater
        | hir::BinaryOp::GreaterEqual => {
            same_operands
                && matches!(
                    underlying,
                    Ty::Int(IntTy::Int) | Ty::Float(FloatTy::Float64) | Ty::String
                )
                && result == &Ty::Bool
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
